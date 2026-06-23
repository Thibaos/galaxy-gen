# Plan 012: GPU-instanced star rendering with soft point sprites

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat cf914ae..HEAD -- src/main.rs src/gpu.rs src/galaxy.rs Cargo.toml src/`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Category**: direction (star rendering quality)
- **Depends on**: 011 (3D density + camera + ray-march)
- **Planned at**: commit `cf914ae`, 2026-06-23

## Why this matters

The current renderer integrates star light into a flat per-pixel accumulator
via `sample_star_grid`.  Stars never appear as individual points — the image
looks like a smooth glow map with no "star field" character.  Real photographs
of galaxies show individual bright stars as soft circular points scattered
throughout the disk, creating a "jewel box" effect.  This plan adds a second
render pass that draws bright stars as instanced billboarded quads with soft
circular profiles, composited additively over the compute shader's smooth
glow output.  The star catalog is generated on the CPU using the same
deterministic hash grid as the compute shader.

## Current state (post-011)

This plan assumes plan 011 is complete.  After 011:

- The compute shader can render in 3D mode (ray-march) or 2D mode
  (column density + star grid)
- `GalaxyUniform` has camera fields and a `render_mode` field
- The star color pipeline (mass→temp→RGB via LUT) is physically accurate
  from plan 010
- `galaxy-shader/src/lib.rs` has: `hash3`, `star_cell`, `sample_imf_from_cell`,
  `cell_star_light`, `mass_to_temp`, `mass_to_lum`, `temperature_to_rgb`, and
  the `COLOR_LUT` table
- `src/gpu.rs` has host-side replicas of `mass_to_temp`, `temperature_to_rgb`,
  and `mass_to_lum` in the `#[cfg(test)] mod tests` block

**Current rendering pipeline:**

```
compute shader → rgba buffer → copy to texture → fullscreen quad (display.wgsl)
```

After this plan:

```
compute shader → rgba buffer → copy to texture
                                    ↓
CPU-generated star catalog → instance buffer → instanced billboarded quads
                                    ↓          (additive blend over texture)
                              display.wgsl → screen
```

**Key constraint to preserve**: The 2D mode and screenshot functionality must
continue working exactly as they do now.  The instanced star pass only runs
when `render_mode == 1` (3D mode).

**Files you'll create:**

- `src/stars.wgsl` — vertex+fragment shader for instanced billboarded quads

**Files you'll modify:**

- `src/gpu.rs` — add `StarInstance`, `generate_star_catalogue`, `GpuStars`,
  and a LUT-sync compile-time check
- `src/main.rs` — integrate star pass into render loop, add egui controls

**Conventions:**

- WGSL for the vertex+fragment billboard shader
- Host-side `StarInstance` uses `#[repr(C)]` with `Pod`/`Zeroable`
- New GPU resources on the `App` struct like existing `GpuCompute`
- `cargo check`, `cargo clippy -- -D warnings`, `cargo test`

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**

- `src/gpu.rs` — `StarInstance`, `generate_star_catalogue`, `GpuStars`,
  host-side star generation function, LUT-sync test
- `src/main.rs` — wire star pass into render loop, star brightness slider
- `src/stars.wgsl` — NEW vertex+fragment billboard shader

**Out of scope:**

- HII regions / nebula rendering
- Bloom/glare post-processing
- Dust rendering
- Changes to `galaxy-shader/src/lib.rs` (no new SPIR-V entry points)
- Changes to the existing `render_scene` or tone-mapping code
- 2D mode star rendering changes (unaffected by this plan)

## Steps

### Step 1: Add host-side star catalog generation to `src/gpu.rs`

Add these items to `src/gpu.rs` — the `StarInstance` struct, the catalogue
generation function, and the `MAX_STARS` constant.  Place them after the
existing `GalaxyUniform`-related code and before the `GpuCompute` impl.

```rust
/// Maximum number of stars in the catalogue buffer.
pub const MAX_STARS: u32 = 65536;

/// Coarse cell size for catalogue scan (light-years).
/// Scanning a 100 LY grid over the galaxy disk (~50 000 LY radius)
/// yields ~1 000 000 cells; only a fraction contain a star above the
/// mass threshold, giving roughly 20 000–50 000 stars.
pub const CATALOGUE_CELL_SIZE: f32 = 50.0;

/// Minimum stellar mass to include in the catalogue (M☉).
pub const CATALOGUE_MASS_THRESHOLD: f32 = 0.8;

/// A single star in the instance buffer.
///
/// Layout (28 bytes, must match `stars.wgsl`):
///   pos_x, pos_y, pos_z, mass, temp, lum, _pad
#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
pub struct StarInstance {
    pub pos_x: f32,
    pub pos_y: f32,
    pub pos_z: f32,
    pub mass: f32,
    pub temp: f32,
    pub lum: f32,
    pub _pad: u32,
}

/// Generate a deterministic star catalogue using the same hash grid as the
/// compute shader.  Scans a coarse 2D grid over the galaxy's XZ plane,
/// uses the hash-based IMF sampler and 2D column density to decide whether
/// each coarse cell contains at least one bright star, and if so assigns a
/// vertical offset from the disk's sech² profile.
///
/// Returns up to `max_stars` instances.
pub fn generate_star_catalogue(
    params: &GalaxyParams,
    max_stars: usize,
) -> Vec<StarInstance> {
    // Reuse the host-side replicas already in `mod tests`
    let disc_radius = 50_000.0_f64;
    let half_side = (disc_radius / CATALOGUE_CELL_SIZE as f64).ceil() as i32;
    let mut stars = Vec::with_capacity(max_stars);

    // Deterministic RNG — seed with params-derived state for reproducibility
    let mut rng_state = (params.disk_scale_length.to_bits() as u64) ^ 0xDEAD_BEEF;

    for ix in -half_side..=half_side {
        for iz in -half_side..=half_side {
            let wx = ix as f64 * CATALOGUE_CELL_SIZE as f64;
            let wz = iz as f64 * CATALOGUE_CELL_SIZE as f64;
            let r = (wx * wx + wz * wz).sqrt();
            if r > disc_radius {
                continue;
            }

            // Compute column density at this coarse position
            let col = disk_column_host(r, params) * arm_modulation_host(wx, wz, r, params)
                + bulge_column_host(r, params)
                + halo_column_host(r, params);

            if col <= 0.0 {
                continue;
            }

            // Expected number of stars in this cell
            let cell_area = CATALOGUE_CELL_SIZE as f64 * CATALOGUE_CELL_SIZE as f64;
            let lambda = col * cell_area;
            let prob = 1.0 - (-lambda).exp();

            // Deterministic hash check
            let cx = star_cell_host(wx as f32);
            let cz = star_cell_host(wz as f32);
            let h = hash3_host(cx, cz, 77u32);
            let rnd = (h >> 8) as f64 / 16777215.0;
            if rnd >= prob {
                continue;
            }

            // Generate star within the cell
            let mass = sample_imf_host(cx, cz);
            if (mass as f64) < CATALOGUE_MASS_THRESHOLD as f64 {
                continue;
            }

            let temp = mass_to_temp(mass);
            let lum = mass_to_lum(mass);

            // Position within cell (use remaining hash bits for jitter)
            let jx = ((h & 0xFF) as f64 / 255.0 - 0.5) * CATALOGUE_CELL_SIZE as f64;
            let jz = (((h >> 16) & 0xFF) as f64 / 255.0 - 0.5) * CATALOGUE_CELL_SIZE as f64;

            // Vertical offset from sech² profile (deterministic)
            // Inverse CDF: y = 2·hz · atanh(2u−1) where u ∈ (0,1)
            let hy = hash3_host(cx, cz, 31337u32);
            let u_y = ((hy >> 8) as f64 / 16777215.0).clamp(0.001, 0.999);
            let y_offset = 2.0 * params.disk_scale_height
                * (2.0 * u_y - 1.0).atanh();

            stars.push(StarInstance {
                pos_x: (wx + jx) as f32,
                pos_y: y_offset as f32,
                pos_z: (wz + jz) as f32,
                mass,
                temp,
                lum,
                _pad: 0,
            });

            if stars.len() >= max_stars {
                return stars;
            }
        }
    }
    stars
}
```

**The host-side replica functions** (`star_cell_host`, `hash3_host`,
`sample_imf_host`, `disk_column_host`, `arm_modulation_host`,
`bulge_column_host`, `halo_column_host`) must be moved out of the
`#[cfg(test)] mod tests` block and into the main `src/gpu.rs` module
so `generate_star_catalogue` can call them.  The test functions in
`mod tests` can continue to use them via `super::`.  Specifically:

- `star_cell`, `hash3`, `mass_to_temp`, `mass_to_lum`, `temperature_to_rgb`,
  and `sample_imf_from_cell` all already exist as host replicas in tests.
  Move each one (with its constants like `STAR_CELL_SIZE`, `STAR_OFFSET`,
  the IMF constants, `COLOR_LUT`) out of `mod tests` into the parent module.
- The column functions (`disk_column`, `bulge_column`, `halo_column`) and
  `arm_modulation_2d` may not have host replicas yet.  If they don't, add
  minimal `f64` versions suffixed `_host` that replicate the shader math.

After moving the helpers, add a `use super::*;` inside `mod tests` to keep
existing tests compiling.

**Verify**: `cargo check` → exit 0 (there will be missing host column replicas
at first — add them and re-check).

### Step 2: Create the billboard quad WGSL shader

Create `src/stars.wgsl`:

```wgsl
// ── Bindings ─────────────────────────────────────────────────
struct CameraUniform {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(0) @binding(1) var<storage, read> stars: array<u32>;

// Each star uses 7 u32 words (packed as f32 bit patterns)
const STAR_STRIDE: u32 = 7u;

// ── Vertex ───────────────────────────────────────────────────

struct VertexInput {
    @builtin(vertex_index) corner: u32,
    @builtin(instance_index) instance: u32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
}

// Unit quad corners (triangle strip order to avoid index buffer)
const CORNERS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 1.0, -1.0),
    vec2<f32>(-1.0,  1.0),
    vec2<f32>( 1.0, -1.0),
    vec2<f32>( 1.0,  1.0),
    vec2<f32>(-1.0,  1.0),
);

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let base = 2u + in.instance * STAR_STRIDE;

    // Read star data from storage buffer
    let px = bitcast<f32>(stars[base + 0u]);
    let py = bitcast<f32>(stars[base + 1u]);
    let pz = bitcast<f32>(stars[base + 2u]);
    let mass = bitcast<f32>(stars[base + 3u]);
    let temp = bitcast<f32>(stars[base + 4u]);
    let lum  = bitcast<f32>(stars[base + 5u]);

    let world_pos = vec3<f32>(px, py, pz);

    // Extract camera right/up from the view-proj matrix rows
    // For an orthographic-like billboard that faces the camera:
    //   right = normalize(cross(view_dir, world_up))
    // Simplified: use the view-proj inverse or extract from camera
    // columns.  For robustness, compute the billboard in clip space:
    // 1. Project world position to clip space
    // 2. Offset in clip space by corner * size * screen_extent
    let clip_center = camera.view_proj * vec4<f32>(world_pos, 1.0);

    // Point size in clip-space units: scale with distance and mass
    let size_clip = 0.004 + 0.012 * log2(max(mass, 0.1) + 0.5);
    let offset = CORNERS[in.corner] * size_clip;

    // Star color from temperature (piecewise LUT)
    let col = temperature_to_rgb(temp);

    // Alpha from luminosity (log-compressed)
    let alpha = clamp(lum * 0.5 / (lum * 0.5 + 1.0), 0.0, 1.0);

    var out: VertexOutput;
    out.position = vec4<f32>(
        clip_center.xy + offset * clip_center.w,
        clip_center.z,
        clip_center.w,
    );
    out.color = vec4<f32>(col.r, col.g, col.b, alpha);
    out.uv = CORNERS[in.corner];
    return out;
}

// ── Temperature→RGB LUT ─────────────────────────────────────

const LUT_LEN: u32 = 16u;

const LUT_DATA: array<vec4<f32>, 16> = array<vec4<f32>, 16>(
    vec4<f32>(2300.0, 1.000, 0.745, 0.424),
    vec4<f32>(2600.0, 1.000, 0.765, 0.427),
    vec4<f32>(3060.0, 1.000, 0.800, 0.435),
    vec4<f32>(3400.0, 1.000, 0.808, 0.506),
    vec4<f32>(3750.0, 1.000, 0.765, 0.545),
    vec4<f32>(4400.0, 1.000, 0.847, 0.710),
    vec4<f32>(5240.0, 1.000, 0.933, 0.867),
    vec4<f32>(5770.0, 1.000, 0.961, 0.949),
    vec4<f32>(6540.0, 0.973, 0.969, 1.000),
    vec4<f32>(7220.0, 0.878, 0.898, 1.000),
    vec4<f32>(8180.0, 0.792, 0.843, 1.000),
    vec4<f32>(9700.0, 0.725, 0.788, 1.000),
    vec4<f32>(15200.0, 0.667, 0.749, 1.000),
    vec4<f32>(26500.0, 0.612, 0.698, 1.000),
    vec4<f32>(41400.0, 0.608, 0.690, 1.000),
    vec4<f32>(50000.0, 0.608, 0.690, 1.000),
);

fn temperature_to_rgb(t_kelvin: f32) -> vec3<f32> {
    var t = clamp(t_kelvin, LUT_DATA[0].x, LUT_DATA[LUT_LEN - 1u].x);
    if t <= LUT_DATA[0].x {
        return vec3<f32>(LUT_DATA[0].y, LUT_DATA[0].z, LUT_DATA[0].w);
    }
    for (var i = 0u; i < LUT_LEN - 1u; i++) {
        if t <= LUT_DATA[i + 1u].x {
            let t_lo = LUT_DATA[i].x;
            let t_hi = LUT_DATA[i + 1u].x;
            let frac = (t - t_lo) / (t_hi - t_lo);
            let r = LUT_DATA[i].y + frac * (LUT_DATA[i + 1u].y - LUT_DATA[i].y);
            let g = LUT_DATA[i].z + frac * (LUT_DATA[i + 1u].z - LUT_DATA[i].z);
            let b = LUT_DATA[i].w + frac * (LUT_DATA[i + 1u].w - LUT_DATA[i].w);
            return vec3<f32>(r, g, b);
        }
    }
    let last = LUT_DATA[LUT_LEN - 1u];
    return vec3<f32>(last.y, last.z, last.w);
}

// ── Fragment ─────────────────────────────────────────────────

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Soft circular profile: alpha fades from center to edge
    let dist = length(in.uv);
    let falloff = 1.0 - smoothstep(0.0, 1.0, dist);
    return vec4<f32>(in.color.rgb, in.color.a * falloff);
}
```

**Verify**: No compile check possible until wired into `main.rs`.  Check that
the file exists and proceed.

### Step 3: Add `GpuStars` resource manager to `src/gpu.rs`

Add after the `generate_star_catalogue` function:

```rust
/// Holds all GPU resources for the instanced star rendering pass.
pub struct GpuStars {
    pub instance_buffer: wgpu::Buffer,
    pub instance_count: u32,
    pub bind_group: wgpu::BindGroup,
    pub pipeline: wgpu::RenderPipeline,
}

impl GpuStars {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let stars_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stars.wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("stars.wgsl").into(),
            ),
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("stars"),
                entries: &[
                    // Camera uniform (view_proj matrix)
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // Star catalog (storage, read-only)
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("stars"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("stars"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &stars_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[], // no vertex buffer — fully instanced
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &stars_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::SrcAlpha,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // Placeholder buffer and bind group — rewritten each frame
        let buf_size = (2 + MAX_STARS as u64 * 7) * 4;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("star_instances"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stars-bind"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &instance_buffer, // placeholder, set per-frame
                        offset: 0,
                        size: None,
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: instance_buffer.as_entire_binding(),
                },
            ],
        });

        Self {
            instance_buffer,
            instance_count: 0,
            bind_group,
            pipeline,
        }
    }
}
```

**Verify**: `cargo check` → exit 0 (will need a camera buffer in `main.rs`
for the bind group — expect errors that resolve in step 4).

### Step 4: Wire star rendering into `main.rs`

**Fields to add** to the `App` struct (after the 3D camera fields from
plan 011):

```rust
    gpu_stars: Option<gpu::GpuStars>,
    camera_buffer: Option<wgpu::Buffer>,
    show_stars: bool,
    star_catalogue: Vec<gpu::StarInstance>,
    star_catalogue_dirty: bool,
```

**Initialize** in `App::new`:

```rust
    gpu_stars: None,
    camera_buffer: None,
    show_stars: true,
    star_catalogue: Vec::new(),
    star_catalogue_dirty: true,
```

**In `App::init`**, after the existing `GpuCompute` initialization, add:

```rust
        self.gpu_stars = Some(gpu::GpuStars::new(
            self.device.as_ref().unwrap(),
            self.config.as_ref().unwrap().format,
        ));

        // Camera uniform buffer (64 bytes = mat4)
        let camera_buffer = self.device.as_ref().unwrap().create_buffer(
            &wgpu::BufferDescriptor {
                label: Some("camera"),
                size: 64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        );
        self.camera_buffer = Some(camera_buffer);
```

**Regenerate the star catalogue** when galaxy parameters change or on first
render.  Add a helper method on `App`:

```rust
fn ensure_star_catalogue(&mut self) {
    if self.star_catalogue_dirty {
        self.star_catalogue = gpu::generate_star_catalogue(
            &self.params,
            gpu::MAX_STARS as usize,
        );
        self.star_catalogue_dirty = false;
    }
}
```

Mark `star_catalogue_dirty = true` whenever the galaxy preset changes
(in the egui parameter change handler).  For simplicity in this plan,
just set it to `true` at startup and regenerate once.

**In `redraw`**, after the compute dispatch and before the render pass, when
`self.render_mode == 1`:

```rust
        // ── Star pass (3D mode only) ──
        if self.render_mode == 1 && self.show_stars {
            self.ensure_star_catalogue();
            let gpu_stars = self.gpu_stars.as_ref().unwrap();
            let cam_buf = self.camera_buffer.as_ref().unwrap();

            // Write camera view-proj matrix
            self.write_view_proj_matrix(queue, self.render_w, self.render_h);

            // Upload star catalogue to GPU
            if !self.star_catalogue.is_empty() {
                // Write header: [count: u32, capacity: u32]
                let mut header = [0u32; 2];
                header[0] = self.star_catalogue.len() as u32;
                header[1] = gpu::MAX_STARS;
                queue.write_buffer(
                    &gpu_stars.instance_buffer,
                    0,
                    bytemuck::cast_slice(&header),
                );
                // Write star data at byte offset 8
                let data_bytes = bytemuck::cast_slice(&self.star_catalogue);
                queue.write_buffer(
                    &gpu_stars.instance_buffer,
                    8,
                    data_bytes,
                );
            }
            self.star_catalogue_dirty = false; // reset after upload
        }
```

The `write_view_proj_matrix` helper (shared between this plan and 011's
`main.rs` camera code):

```rust
fn write_view_proj_matrix(&self, queue: &wgpu::Queue, image_w: u32, image_h: u32) {
    use glam::{Mat4, Vec3};

    let cam_pos = self.camera_position();
    let eye = Vec3::new(cam_pos.0, cam_pos.1, cam_pos.2);
    let target = Vec3::new(
        self.camera_target_x,
        self.camera_target_y,
        self.camera_target_z,
    );
    let up = Vec3::Y;

    let view = Mat4::look_at_rh(eye, target, up);
    let aspect = image_w as f32 / image_h as f32;
    let fov_rad = self.fov_y.to_radians();
    let proj = Mat4::perspective_rh(fov_rad, aspect, 100.0, 1_000_000.0);
    let vp = proj * view;

    queue.write_buffer(
        self.camera_buffer.as_ref().unwrap(),
        0,
        bytemuck::cast_slice(&vp.to_cols_array()),
    );
}
```

**In the render pass**, after the fullscreen quad draw, add:

```rust
            // ── Draw instanced stars (3D mode + additive blend) ──
            if self.render_mode == 1
                && self.show_stars
                && !self.star_catalogue.is_empty()
            {
                let gpu_stars = self.gpu_stars.as_ref().unwrap();
                rpass.set_pipeline(&gpu_stars.pipeline);
                rpass.set_bind_group(0, &gpu_stars.bind_group, &[]);
                // 6 vertices per quad (triangle strip), N instances
                rpass.draw(0..6, 0..self.star_catalogue.len() as u32);
            }
```

**Verify**: `cargo check` → exit 0.  Fix any compilation errors (missing
`camera_buffer`, `star_catalogue` field references, etc.).

### Step 5: Add star rendering toggle to egui panel

In the egui sidebar's 3D section (inside the `if self.render_mode == 1` block
from plan 011), add after the camera sliders:

```rust
            let stars_changed = ui
                .add(egui::Checkbox::new(&mut self.show_stars, "Show Stars"))
                .changed();
            if stars_changed {
                self.needs_render = true;
            }
```

Add `star_catalogue_dirty: true` to the galaxy preset change handler so the
catalogue regenerates when switching presets.

**Verify**: `cargo check` → exit 0

### Step 6: Add LUT-sync compile-time test to `src/gpu.rs`

In the `mod tests` block, add a test that verifies the WGSL shader's LUT
matches the Rust `COLOR_LUT` at compile time:

```rust
    #[test]
    fn wgsl_color_lut_matches_rust_lut() {
        let wgsl_source = include_str!("stars.wgsl");
        // Find the LUT_DATA array in the WGSL source
        let start = wgsl_source.find("const LUT_DATA:").unwrap();
        let end = wgsl_source[start..].find(");").unwrap() + start + 2;
        let lut_section = &wgsl_source[start..end];

        // Extract each vec4 line
        for (i, entry) in COLOR_LUT.iter().enumerate() {
            let expected = format!(
                "vec4<f32>({:.1}, {:.3}, {:.3}, {:.3})",
                entry.0, entry.1, entry.2, entry.3
            );
            assert!(
                lut_section.contains(&expected),
                "LUT entry {i} ({expected}) not found in stars.wgsl"
            );
        }
    }
```

**Verify**: `cargo test wgsl_color_lut_matches_rust_lut` → passes

### Step 7: Run full validation

```bash
cargo clippy -- -D warnings
cargo test
```

Fix all warnings and test failures.

**Manual visual test**: Run `cargo run`, switch to 3D mode, and verify that
stars appear as soft circular points over the smooth glow.  Adjust the
emissivity factor in `ray_march_galaxy` (plan 011's `0.03` constant) if the
smooth glow is washed out by the stars — the stars use additive blending
so they will brighten the glow wherever they overlap.

## Test plan

- Existing tests: all must continue to pass
- New test: `wgsl_color_lut_matches_rust_lut` — verifies compile-time LUT
  sync between `stars.wgsl` and Rust `COLOR_LUT`
- If the executor adds host-side column replicas (`disk_column_host`, etc.),
  add a test verifying they match the shader's `disk_column` outputs for a
  few representative radii

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0; all existing + new LUT-sync test pass
- [ ] `src/stars.wgsl` exists and compiles (pipeline creation succeeds at
      `cargo run` time)
- [ ] 2D mode renders identically to before this plan
- [ ] 3D mode shows stars when `show_stars` is enabled
- [ ] No files outside in-scope list modified (`git status`)
- [ ] `plans/README.md` updated with status row for plan 012

## STOP conditions

- The `stars.wgsl` shader fails WGSL validation.  Common issues:
  `bitcast<f32>` not supported in older WGSL — replace with manual
  bit-reinterpretation via `f32(u32_val)` cast if needed.
  Array indexing out of bounds — ensure all indices are within bounds.
- Moving host replica functions out of `mod tests` breaks existing tests.
  Use `use super::*;` inside `mod tests` to keep them accessible.
  If any test relied on the functions being in a specific scope, adjust.
- 2D mode rendering changes visually.  If the existing display.wgsl
  quad or tone-mapping code was accidentally modified, revert it.
- The star catalogue generation takes >100 ms.  If so, increase
  `CATALOGUE_CELL_SIZE` or reduce the scan radius.  The `for` loop over
  `-half_side..=half_side` is O(n²) where n ~ 2000 at 50 LY cells.

## Maintenance notes

- The color LUT in `stars.wgsl` MUST stay in sync with `COLOR_LUT` in
  `galaxy-shader/src/lib.rs`.  The `wgsl_color_lut_matches_rust_lut` test
  catches drift at `cargo test` time.
- The star catalogue is regenerated on the CPU once at startup (and when
  galaxy parameters change).  For ~50 000 stars this takes <10 ms.  If
  performance becomes an issue, the generation can move to a background
  thread or be cached per preset.
- The billboard quads use clip-space offsets rather than true camera-facing
  transforms.  This works well for a narrow FOV (≤90°) but quads may appear
  slightly skewed at extreme FOVs.  For a production-quality billboard,
  compute the camera right/up vectors in the vertex shader from the
  inverse view-proj matrix.
- The star catalogue uses the 2D column density (not 3D) for its existence
  check — this is consistent with the 2D disk profile.  When plan 012 runs,
  stars are placed at their sech² vertical offset, so the 3D distribution
  is correct even though the existence decision uses column density.
- `CATALOGUE_CELL_SIZE = 50.0` is a balance between star count and CPU time.
  Smaller cells → more stars, more CPU time.  The `CATALOGUE_MASS_THRESHOLD`
  of 0.8 M☉ filters ~79% of stars (mainly low-mass Kroupa segment 1 dwarfs).
