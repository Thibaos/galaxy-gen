# Plan 011: 3D density profiles, camera model, and ray-march rendering path

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat cf914ae..HEAD -- galaxy-shader/src/lib.rs src/gpu.rs src/main.rs Cargo.toml`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Category**: direction (3D rendering foundation)
- **Depends on**: none (010 is DONE)
- **Planned at**: commit `cf914ae`, 2026-06-23

## Why this matters

The current renderer is strictly 2D — each pixel maps to a flat world-space
rectangle via orthographic projection.  To support an orbit camera and 3D
views, the renderer needs 3D density profiles, a camera model, and a
ray-marching path that integrates emissivity along camera rays through the
volume.  This plan establishes all three without breaking the existing 2D
mode.

## Current state

**Relevant files:**

- `galaxy-shader/src/lib.rs` — compute shader; `render_scene` entry point
  does 2D orthographic projection at lines ~410-440, calls `column_density`
  - `sample_star_grid`, then tone-maps
- `src/gpu.rs` — `GalaxyUniform` struct (19 fields, 76 bytes), `GpuCompute`,
  `compute_galaxy` dispatch function
- `src/main.rs` — `App` struct with `center_x/y`, `extent_ly`, pan/drag/zoom;
  egui sidebar with galaxy param sliders
- `Cargo.toml` — deps: wgpu, winit, egui, bytemuck, pollster, image (no
  CPU-side math library)

**Current `render_scene` (galaxy-shader/src/lib.rs, ~lines 410-460):**

```rust
#[spirv(compute(threads(8, 8, 1)))]
pub fn render_scene(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] params: &GalaxyUniform,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] rgba: &mut [u32],
) {
    // ... computes wx, wz from pixel index via orthographic projection
    let wx = (px as f32 / params.image_width as f32 - 0.5) * extent_x + params.center_x;
    let wz = -(py as f32 / params.image_height as f32 - 0.5) * extent_y + params.center_y;

    let col_dens = column_density(wx, wz, params);
    let rgb = sample_star_grid(wx, wz, pixel_w, pixel_h, col_dens);

    // ... tone-maps and writes rgba[idx]
}
```

**Current `column_density` integrates 2D column density (not 3D):**

```rust
fn column_density(wx: f32, wz: f32, p: &GalaxyUniform) -> f32 {
    let r = (wx * wx + wz * wz).sqrt();
    disk_column(r, p) * arm_modulation_2d(wx, wz, r, p)
        + bulge_column(r, p)
        + halo_column(r, p)
}
```

The column functions (`disk_column`, `bulge_column`, `halo_column`) are
analytic line-of-sight integrals through symmetric 3D profiles — they already
represent 3D structure projected to 2D.  For 3D ray-marching we need the
un-projected 3D density instead.

**Current GalaxyUniform (src/gpu.rs, ~lines 82-100):**

```rust
#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
pub struct GalaxyUniform {
    pub disk_scale_length: f32,
    pub disk_scale_height: f32,
    pub disk_central_density: f32,
    pub arm_count: u32,
    pub arm_pitch: f32,
    pub arm_concentration: f32,
    pub arm_strength: f32,
    pub bulge_radius: f32,
    pub bulge_central_density: f32,
    pub halo_radius: f32,
    pub halo_central_density: f32,
    pub halo_slope: f32,
    pub image_width: u32,
    pub image_height: u32,
    pub extent: f32,
    pub center_x: f32,
    pub center_y: f32,
    pub exposure: f32,
    pub log_contrast: f32,
}
```

**Conventions to follow:**

- Shader uses `spirv-std` + `glam`; all floats are `f32`
- `GalaxyUniform` is `#[repr(C)]` with `Pod`/`Zeroable` — every new field
  must preserve this
- Every shader change gets a host-side replica in `src/gpu.rs` tests
- The test at `galaxy_uniform_field_offsets_match_shader_layout` checks exact
  byte offsets — it MUST be updated when fields are added
- `cargo check`, `cargo clippy -- -D warnings`, `cargo test` as verification
- The `spirv-std` `GalaxyUniform` in `galaxy-shader/src/lib.rs` and the host
  `GalaxyUniform` in `src/gpu.rs` must stay field-for-field identical

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**

- `galaxy-shader/src/lib.rs` — add 3D density functions, camera fields to
  uniform, ray-march code path, `render_mode` branch
- `src/gpu.rs` — extend `GalaxyUniform` with camera + render-mode fields,
  update `from_params`, update offset tests, add 3D density tests
- `src/main.rs` — add `render_mode` field to `App`, add `camera_*` fields,
  add camera control inputs (mouse/keyboard), add camera sliders to egui
  panel, add 2D/3D toggle
- `Cargo.toml` — add `glam = { version = "0.29", features = ["libm"] }`
  for CPU-side view matrix construction

**Out of scope:**

- Instanced star rendering — plan 012
- Bloom/glare post-processing
- HII regions
- Dust rendering
- Changes to `mass_to_temp`, `temperature_to_rgb`, or `mass_to_lum`

## Steps

### Step 0: Add `glam` dependency

Add to `Cargo.toml` under `[dependencies]`:

```toml
glam = { version = "0.29", features = ["libm"] }
```

The `libm` feature ensures consistency with spirv-std's no_std math.

**Verify**: `cargo check` → exit 0 (dependency resolves, no usage yet)

### Step 1: Add 3D density functions to the shader

In `galaxy-shader/src/lib.rs`, add these functions after the existing
`arm_modulation_2d` function (~line 98, after the closing `}` of that
function):

```rust
// ═══════════════════════════════════════════════════════════
//  3D density profiles (used by ray-march path)
// ═══════════════════════════════════════════════════════════

/// Disk 3D density: exponential radial × sech² vertical.
///
/// ρ(R, z) = ρ₀ × exp(−R/h_r) × sech²(z / (2 × h_z))
///
/// sech² is a common fit for isothermal self-gravitating disks
/// (Spitzer 1942, van der Kruit & Searle 1981).
fn disk_density_3d(x: f32, y: f32, z: f32, p: &GalaxyUniform) -> f32 {
    if p.disk_scale_length <= 0.0 || p.disk_scale_height <= 0.0 {
        return 0.0;
    }
    let r = (x * x + z * z).sqrt();
    let sech = 1.0 / (y / (2.0 * p.disk_scale_height)).cosh();
    p.disk_central_density * 0.5 * (-r / p.disk_scale_length).exp() * sech * sech
}

/// Bulge 3D density (Plummer sphere).
///
/// ρ(r) = ρ₀ × (1 + r²/a²)^(−2.5)
fn bulge_density_3d(x: f32, y: f32, z: f32, p: &GalaxyUniform) -> f32 {
    if p.bulge_radius <= 0.0 {
        return 0.0;
    }
    let r2 = x * x + y * y + z * z;
    let x2 = r2 / (p.bulge_radius * p.bulge_radius);
    p.bulge_central_density * (1.0 + x2).powf(-2.5)
}

/// Halo 3D density (power-law sphere).
///
/// ρ(r) = ρ₀ × (1 + r/R_h)^s
fn halo_density_3d(x: f32, y: f32, z: f32, p: &GalaxyUniform) -> f32 {
    if p.halo_radius <= 0.0 {
        return 0.0;
    }
    let r = (x * x + y * y + z * z).sqrt();
    p.halo_central_density * (1.0 + r / p.halo_radius).powf(p.halo_slope)
}
```

The existing `column_density` and its 2D column functions (`disk_column`,
`bulge_column`, `halo_column`) are **NOT modified or removed** — they remain
the 2D code path.

**Verify**: `cargo check` → exit 0

### Step 2: Add camera and render-mode fields to both halves of GalaxyUniform

**Shader-side** (`galaxy-shader/src/lib.rs`): Locate the `GalaxyUniform`
struct definition (should be near the top, ~line 20-45).  Add these fields
at the end, before the closing `}`:

```rust
    // ── 3D camera / render mode ──
    pub render_mode: u32,    // 0 = 2D (current), 1 = 3D ray-march
    pub camera_x: f32,       // world-space camera position
    pub camera_y: f32,
    pub camera_z: f32,
    pub camera_target_x: f32,// look-at point
    pub camera_target_y: f32,
    pub camera_target_z: f32,
    pub fov_y_deg: f32,      // vertical field of view in degrees
```

**Host-side** (`src/gpu.rs`): Add the same fields in the same order at the end
of the struct, after `log_contrast`:

```rust
    // ── 3D camera / render mode ──
    pub render_mode: u32,
    pub camera_x: f32,
    pub camera_y: f32,
    pub camera_z: f32,
    pub camera_target_x: f32,
    pub camera_target_y: f32,
    pub camera_target_z: f32,
    pub fov_y_deg: f32,
```

**Update `GalaxyUniform::from_params`** — add new parameters:

```rust
pub fn from_params(
    params: &GalaxyParams,
    image_w: u32,
    image_h: u32,
    extent: f64,
    center_x: f64,
    center_y: f64,
    exposure: f32,
    contrast: f32,
    render_mode: u32,
    camera_pos: (f32, f32, f32),
    camera_target: (f32, f32, f32),
    fov_y_deg: f32,
) -> Self {
    Self {
        // ... existing fields unchanged ...
        render_mode,
        camera_x: camera_pos.0,
        camera_y: camera_pos.1,
        camera_z: camera_pos.2,
        camera_target_x: camera_target.0,
        camera_target_y: camera_target.1,
        camera_target_z: camera_target.2,
        fov_y_deg,
    }
}
```

**Update the offset test** in `galaxy_uniform_field_offsets_match_shader_layout`:
after the `assert_eq!(offset_of!(GalaxyUniform, log_contrast), 72);` line, add:

```rust
        assert_eq!(offset_of!(GalaxyUniform, render_mode), 76);
        assert_eq!(offset_of!(GalaxyUniform, camera_x), 80);
        assert_eq!(offset_of!(GalaxyUniform, camera_y), 84);
        assert_eq!(offset_of!(GalaxyUniform, camera_z), 88);
        assert_eq!(offset_of!(GalaxyUniform, camera_target_x), 92);
        assert_eq!(offset_of!(GalaxyUniform, camera_target_y), 96);
        assert_eq!(offset_of!(GalaxyUniform, camera_target_z), 100);
        assert_eq!(offset_of!(GalaxyUniform, fov_y_deg), 104);
```

Also update `uniform_struct_size_matches_fields`:

- Update the expected size computation.  The original struct had 16 f32 + 3 u32 = 76 bytes.
  Adding 7 f32 (camera fields) + 1 u32 (render_mode) gives 23 f32 + 4 u32 = 108 bytes.

Update the test:

```rust
fn uniform_struct_size_matches_fields() {
    let expected = std::mem::size_of::<f32>() * 23 + std::mem::size_of::<u32>() * 4;
    assert_eq!(std::mem::size_of::<GalaxyUniform>(), expected);
}
```

**Update all existing callers** of `GalaxyUniform::from_params` — they're in
the test functions and in `main.rs`.  For tests, pass `0` for `render_mode`,
`(0.0, 5000.0, 0.0)` for camera_pos, `(0.0, 0.0, 0.0)` for camera_target,
and `45.0` for `fov_y_deg` as defaults.  For `main.rs`, see Step 5.

**Verify**: `cargo check` → exit 0 (will report errors at all call sites of
`from_params` in `main.rs` — fix them with the default values listed above,
then re-check)

### Step 3: Add the 3D ray-march code path to `render_scene`

In `galaxy-shader/src/lib.rs`, locate the `render_scene` function.  The
existing code computes `wx`, `wz` from 2D orthographic projection, then
calls `column_density` + `sample_star_grid`.  We wrap this in a branch on
`params.render_mode`.

Replace the body of `render_scene` with the code below — it wraps the
existing 2D path in a `render_mode` branch and adds the 3D ray-march path.
The tone-map code at the end is unchanged (shared between both paths).

The new structure:

```rust
#[spirv(compute(threads(8, 8, 1)))]
pub fn render_scene(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] params: &GalaxyUniform,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] rgba: &mut [u32],
) {
    let px = id.x;
    let py = id.y;
    if px >= params.image_width || py >= params.image_height {
        return;
    }

    let buf_stride = params.image_width.div_ceil(64u32) * 64u32;
    let idx = (py * buf_stride + px) as usize;

    // ── compute rgb based on render mode ──
    let rgb = if params.render_mode == 0 {
        // ── 2D path (current behaviour) ──
        let extent_x = params.extent;
        let extent_y = params.extent * (params.image_height as f32 / params.image_width as f32);
        let pixel_w = extent_x / params.image_width as f32;
        let pixel_h = extent_y / params.image_height as f32;
        let wx = (px as f32 / params.image_width as f32 - 0.5) * extent_x + params.center_x;
        let wz = -(py as f32 / params.image_height as f32 - 0.5) * extent_y + params.center_y;

        let col_dens = column_density(wx, wz, params);
        sample_star_grid(wx, wz, pixel_w, pixel_h, col_dens)
    } else {
        // ── 3D ray-march path ──
        ray_march_galaxy(px, py, params)
    };

    // ── tone-map (shared) ──
    let epsilon: f32 = 1e-12;
    let lum = 0.299 * rgb.x + 0.587 * rgb.y + 0.114 * rgb.z;
    let log_lum = (lum + epsilon).ln();
    let t = (log_lum * params.log_contrast + params.exposure).clamp(0.0, 1.0);

    let inv_lum = 1.0 / lum.max(epsilon);
    let r = (rgb.x * inv_lum * t).clamp(0.0, 1.0);
    let g = (rgb.y * inv_lum * t).clamp(0.0, 1.0);
    let b = (rgb.z * inv_lum * t).clamp(0.0, 1.0);

    let r8 = (r * 255.0) as u32;
    let g8 = (g * 255.0) as u32;
    let b8 = (b * 255.0) as u32;
    rgba[idx] = r8 | g8 << 8 | b8 << 16 | 0xff_00_00_00;
}
```

Now add the `ray_march_galaxy` function above `render_scene`:

```rust
/// Number of ray-march steps along the camera ray.
const RAY_STEPS: u32 = 64;

/// Maximum ray-march distance in light-years (≈ galaxy far-field radius).
const RAY_MAX_DIST: f32 = 200_000.0;

/// 3D ray-march: returns accumulated emissivity as linear RGB.
///
/// Each step samples the 3D density field, multiplies by an
/// emissivity-per-unit-density factor, and accumulates.
/// The camera ray is constructed from the uniform's camera position,
/// look-at target, and FOV.
fn ray_march_galaxy(px: u32, py: u32, p: &GalaxyUniform) -> Vec3 {
    let aspect = p.image_width as f32 / p.image_height as f32;

    // Camera basis vectors
    let forward_x = p.camera_target_x - p.camera_x;
    let forward_y = p.camera_target_y - p.camera_y;
    let forward_z = p.camera_target_z - p.camera_z;
    let forward_len = (forward_x * forward_x + forward_y * forward_y + forward_z * forward_z).sqrt();
    if forward_len < 1e-6 {
        return Vec3::new(0.0, 0.0, 0.0);
    }
    let fx = forward_x / forward_len;
    let fy = forward_y / forward_len;
    let fz = forward_z / forward_len;

    // Right = forward × world_up (0, 1, 0)
    let world_up_x: f32 = 0.0;
    let world_up_y: f32 = 1.0;
    let world_up_z: f32 = 0.0;
    let rx = fy * world_up_z - fz * world_up_y;
    let ry = fz * world_up_x - fx * world_up_z;
    let rz = fx * world_up_y - fy * world_up_x;
    let r_len = (rx * rx + ry * ry + rz * rz).sqrt();
    let (rx, ry, rz) = if r_len < 1e-6 {
        // Camera looking straight up/down — use an arbitrary right vector
        (1.0, 0.0, 0.0)
    } else {
        (rx / r_len, ry / r_len, rz / r_len)
    };

    // Up = right × forward
    let ux = ry * fz - rz * fy;
    let uy = rz * fx - rx * fz;
    let uz = rx * fy - ry * fx;

    // Screen-space to ray direction
    let half_h = (0.5 * p.fov_y_deg).to_radians().tan();
    let half_w = half_h * aspect;
    let sx = (px as f32 / p.image_width as f32 - 0.5) * 2.0;
    let sy = -(py as f32 / p.image_height as f32 - 0.5) * 2.0;

    let dir_x = fx + sx * half_w * rx + sy * half_h * ux;
    let dir_y = fy + sx * half_w * ry + sy * half_h * uy;
    let dir_z = fz + sx * half_w * rz + sy * half_h * uz;
    let dir_len = (dir_x * dir_x + dir_y * dir_y + dir_z * dir_z).sqrt();
    let dx = dir_x / dir_len;
    let dy = dir_y / dir_len;
    let dz = dir_z / dir_len;

    // ── Ray-march ──
    let dt = RAY_MAX_DIST / RAY_STEPS as f32;
    let mut acc = Vec3::new(0.0, 0.0, 0.0);

    // Start at t = 0 (camera position) or t_near
    let mut t = 0.0;
    for _ in 0..RAY_STEPS {
        let sx_pos = p.camera_x + t * dx;
        let sy_pos = p.camera_y + t * dy;
        let sz_pos = p.camera_z + t * dz;
        t += dt;

        let r = (sx_pos * sx_pos + sz_pos * sz_pos).sqrt();

        // Skip if outside galaxy bounds
        if r > RAY_MAX_DIST * 0.5 {
            continue;
        }

        let dens = disk_density_3d(sx_pos, sy_pos, sz_pos, p)
            * arm_modulation_2d(sx_pos, sz_pos, r, p)
            + bulge_density_3d(sx_pos, sy_pos, sz_pos, p)
            + halo_density_3d(sx_pos, sy_pos, sz_pos, p);

        if dens <= 0.0 {
            continue;
        }

        // Emissivity per unit density: model the average star light from
        // the IMF at this radius.  Use the 2D star grid at (sx_pos, sz_pos)
        // as a representative colour sample scaled by density.
        // The 0.03 factor is a heuristic emissivity calibrating
        // density × dt × factor → reasonable pixel brightness.
        let star_col = cell_star_light(
            star_cell(sx_pos),
            star_cell(sz_pos),
        );
        acc.x += dens * star_col.x * dt * 0.03;
        acc.y += dens * star_col.y * dt * 0.03;
        acc.z += dens * star_col.z * dt * 0.03;
    }

    acc
}
```

Note: `cell_star_light` uses `mass_to_lum` which returns solar-luminosity
units.  The 0.03 factor is an empirically-chosen emissivity constant —
this will be adjusted after visual testing.

**Verify**: `cargo check` → exit 0

### Step 4: Add host-side 3D density tests to `src/gpu.rs`

In the `mod tests` block, after the existing column profile tests, add:

```rust
    // ── 3D density tests ──────────────────────────────

    fn host_disk_density_3d(x: f64, y: f64, z: f64, p: &GalaxyParams) -> f64 {
        if p.disk_scale_length <= 0.0 || p.disk_scale_height <= 0.0 {
            return 0.0;
        }
        let r = (x * x + z * z).sqrt();
        let sech = 1.0 / (y / (2.0 * p.disk_scale_height)).cosh();
        p.disk_central_density * 0.5 * (-r / p.disk_scale_length).exp() * sech * sech
    }

    fn host_bulge_density_3d(x: f64, y: f64, z: f64, p: &GalaxyParams) -> f64 {
        if p.bulge_radius <= 0.0 {
            return 0.0;
        }
        let r2 = x * x + y * y + z * z;
        let x2 = r2 / (p.bulge_radius * p.bulge_radius);
        p.bulge_central_density * (1.0 + x2).powf(-2.5)
    }

    #[test]
    fn disk_density_3d_midplane_matches_2d_profile() {
        let p = GalaxyParams::milky_way();
        // Normalization check: ∫ ρ₃D(R,y) dy must equal the existing column(R).
        //   column(0) = 2·hz·ρ₀  (disk_column at r=0)
        //   ∫ ρ₃D(0,y) dy = ∫ ρ₀·0.5·sech²(y/2hz) dy = ρ₀·0.5·4hz = 2·hz·ρ₀ ✓
        // Therefore midplane density = column(0) / (4·hz)
        let col = 2.0 * p.disk_scale_height * p.disk_central_density; // disk_column(0) simplified
        let dens = host_disk_density_3d(0.0, 0.0, 0.0, &p);
        let expected = col / (4.0 * p.disk_scale_height);
        assert!((dens - expected).abs() < 1e-10,
            "midplane density {dens} != column/(4hz) = {expected}");
    }

    #[test]
    fn disk_density_3d_decays_vertically() {
        let p = GalaxyParams::milky_way();
        let d0 = host_disk_density_3d(0.0, 0.0, 0.0, &p);
        let d1 = host_disk_density_3d(0.0, p.disk_scale_height, 0.0, &p);
        let d2 = host_disk_density_3d(0.0, 2.0 * p.disk_scale_height, 0.0, &p);
        assert!(d0 > 0.0);
        assert!(d1 < d0, "density should decrease with |y|");
        assert!(d2 < d1, "density should continue decreasing");
        // sech²(0.5) ≈ 0.786 at y = hz
        let sech_at_hz = 1.0 / (0.5_f64.cosh());
        let expected_ratio = sech_at_hz * sech_at_hz;
        let actual_ratio = d1 / d0;
        assert!((actual_ratio - expected_ratio).abs() < 0.001,
            "density at y=hz / d0 = {actual_ratio}, expected {expected_ratio}");
    }

    #[test]
    fn disk_density_3d_sech_squared_profile() {
        // Verify sech² shape: density(y) / density(0) = sech²(y/2hz)
        let p = GalaxyParams::milky_way();
        for &y in &[0.0_f64, 500.0, 1000.0, 2000.0] {
            let dens = host_disk_density_3d(0.0, y, 0.0, &p);
            let expected = p.disk_central_density * 0.5 * (1.0 / (y / (2.0 * p.disk_scale_height)).cosh()).powi(2);
            assert!((dens - expected).abs() < 1e-12,
                "density at y={y} = {dens}, expected {expected}");
        }
    }

    #[test]
    fn bulge_density_3d_plummer_profile() {
        let p = GalaxyParams::milky_way();
        let d0 = host_bulge_density_3d(0.0, 0.0, 0.0, &p);
        let da = host_bulge_density_3d(p.bulge_radius, 0.0, 0.0, &p);
        assert!(d0 > 0.0);
        // Plummer 3D: ρ(R) ∝ (1 + R²/a²)^(-2.5). At R=a: (1+1)^(-2.5) = 2^(-2.5) ≈ 0.1768
        let expected_ratio = 2.0_f64.powf(-2.5);
        let actual_ratio = da / d0;
        assert!((actual_ratio - expected_ratio).abs() < 0.001,
            "bulge 3D at R=a / d0 = {actual_ratio}, expected {expected_ratio}");
    }

    #[test]
    fn uniform_new_fields_have_reasonable_defaults() {
        let params = GalaxyParams::milky_way();
        let uniform = GalaxyUniform::from_params(
            &params, 800, 600, 100_000.0, 0.0, 0.0, 0.5, 0.05,
            0, (0.0, 5000.0, 0.0), (0.0, 0.0, 0.0), 45.0,
        );
        assert_eq!(uniform.render_mode, 0);
        assert_eq!(uniform.fov_y_deg, 45.0);
        assert!(uniform.camera_y > 0.0, "camera should be above the disk");
    }
```

**Verify**: `cargo test` → all tests pass (existing + new)

### Step 5: Update `main.rs` — add camera state, controls, and egui toggle

In `main.rs`, add these fields to the `App` struct (after `saved_startup_image`):

```rust
    // 3D mode
    render_mode: u32,         // 0 = 2D, 1 = 3D
    camera_dist: f32,         // distance from target (ly)
    camera_azimuth: f32,      // orbit azimuth (radians, 0 = +Z axis)
    camera_elevation: f32,    // orbit elevation (radians, π/2 = top-down)
    camera_target_x: f32,
    camera_target_y: f32,
    camera_target_z: f32,
    fov_y: f32,               // vertical FOV in degrees
    orbit_dragging: bool,     // right-click orbit drag
```

Initialize them in `App::new`:

```rust
    render_mode: 0,
    camera_dist: 100_000.0,
    camera_azimuth: 0.0,
    camera_elevation: std::f32::consts::FRAC_PI_2, // top-down
    camera_target_x: 0.0,
    camera_target_y: 0.0,
    camera_target_z: 0.0,
    fov_y: 45.0,
    orbit_dragging: false,
```

Add a helper function on `App`:

```rust
fn camera_position(&self) -> (f32, f32, f32) {
    let horiz = self.camera_dist * self.camera_elevation.cos();
    let x = self.camera_target_x + horiz * self.camera_azimuth.sin();
    let y = self.camera_target_y + self.camera_dist * self.camera_elevation.sin();
    let z = self.camera_target_z + horiz * self.camera_azimuth.cos();
    (x, y, z)
}
```

In `redraw`, update the `GalaxyUniform::from_params` call to pass the new
fields.  Replace the existing call:

```rust
let uniform_data = gpu::GalaxyUniform::from_params(
    &self.params,
    self.render_w,
    self.render_h,
    self.extent_ly,
    self.center_x,
    self.center_y,
    self.exposure,
    self.contrast,
);
```

with:

```rust
let cam_pos = self.camera_position();
let uniform_data = gpu::GalaxyUniform::from_params(
    &self.params,
    self.render_w,
    self.render_h,
    self.extent_ly,
    self.center_x,
    self.center_y,
    self.exposure,
    self.contrast,
    self.render_mode,
    cam_pos,
    (self.camera_target_x, self.camera_target_y, self.camera_target_z),
    self.fov_y,
);
```

In the egui sidebar, add a 3D controls section after the "Brightness" section
and before the "Actions" section:

```rust
// 3D controls
ui.separator();
ui.label("3D View");
let mode_changed = ui
    .add(egui::Checkbox::new(&mut (self.render_mode == 1), "3D Mode"))
    .changed();
if mode_changed {
    self.render_mode = if self.render_mode == 0 { 1 } else { 0 };
    self.needs_render = true;
}
if self.render_mode == 1 {
    let dist_changed = ui
        .add(egui::Slider::new(&mut self.camera_dist, 5_000.0..=500_000.0)
            .text("Distance (ly)"))
        .changed();
    let az_changed = ui
        .add(egui::Slider::new(&mut self.camera_azimuth, 0.0..=std::f32::consts::TAU)
            .text("Azimuth"))
        .changed();
    let el_changed = ui
        .add(egui::Slider::new(&mut self.camera_elevation, 0.05..=std::f32::consts::FRAC_PI_2)
            .text("Elevation"))
        .changed();
    let fov_changed = ui
        .add(egui::Slider::new(&mut self.fov_y, 5.0..=120.0).text("FOV"))
        .changed();
    if dist_changed || az_changed || el_changed || fov_changed {
        self.needs_render = true;
    }
}
```

**In 3D mode, warp the 2D pan/zoom inputs to camera controls:**

- Still allow mouse wheel for zoom (adjust `camera_dist` instead of `extent`)
- When not-dragging (orbit_dragging not active) and in 3D mode, left-click adjusts azimuth/elevation

Actually, for simplicity in this plan, use **right mouse button** for orbit
and keep left button for 2D pan (which becomes target pan in 3D mode).
Add to the `MouseInput` handler:

```rust
WindowEvent::MouseInput {
    state,
    button: MouseButton::Right,
    ..
} => {
    if self.render_mode == 1 && !self.egui_ctx.wants_pointer_input() {
        self.orbit_dragging = state == ElementState::Pressed;
    }
}
```

And in `CursorMoved`, after the existing pan logic, add orbit:

```rust
if self.orbit_dragging && self.render_mode == 1 && !self.egui_ctx.wants_pointer_input() {
    let dx = cx - lx;
    let dy = cy - ly;
    self.camera_azimuth -= dx as f32 * 0.005;
    self.camera_elevation = (self.camera_elevation + dy as f32 * 0.005)
        .clamp(0.05, std::f32::consts::FRAC_PI_2 - 0.01);
    self.needs_render = true;
}
```

Also update the mouse-wheel handler to adjust `camera_dist` in 3D mode:

```rust
if self.render_mode == 1 {
    let factor = if scroll > 0.0 { 1.0 / ZOOM_SPEED } else { ZOOM_SPEED };
    self.camera_dist = (self.camera_dist as f64 * factor)
        .clamp(5_000.0, 500_000.0) as f32;
    self.needs_render = true;
}
```

**Verify**: `cargo check` → exit 0

### Step 6: Run full validation

```bash
cargo clippy -- -D warnings
cargo test
```

Fix any warnings or failures.  If clippy complains about the orbit-drag `dx`
conversion, use `as f32` casts.

## Test plan

- Existing tests: all must continue to pass.  The `from_params` tests have
  been updated to pass the new camera args.
- New tests added in Step 4:
  - `disk_density_3d_midplane_matches_2d_profile` — verifies consistency
    between 2D column and 3D density at y=0
  - `disk_density_3d_decays_vertically` — verifies sech² shape
  - `disk_density_3d_sech_squared_profile` — exact sech² formula check
  - `bulge_density_3d_plummer_profile` — Plummer 3D profile at R=a
  - `uniform_new_fields_have_reasonable_defaults` — sanity check
- The offset test `galaxy_uniform_field_offsets_match_shader_layout` and
  size test `uniform_struct_size_matches_fields` are updated for the new
  struct layout.

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0; all existing + ≥5 new 3D density tests pass
- [ ] `GallaxyUniform` offset test updated and passing
- [ ] `GallaxyUniform` size test updated and passing
- [ ] Both 2D (render_mode=0) and 3D (render_mode=1) code paths compile
- [ ] No files outside in-scope list modified (`git status`)
- [ ] `plans/README.md` updated with row for plan 011

## STOP conditions

- Code at the "Current state" excerpts doesn't match live code
- `cargo check` or SPIR-V compilation fails with a non-trivial error
- An existing test breaks in a way unrelated to the uniform struct change
  (i.e. a column-density test fails — that would mean the density math
  was accidentally modified)
- The offset test fails after adding fields — check alignment, check that
  the shader-side `GalaxyUniform` has fields in the same order

## Maintenance notes

- The `0.03` emissivity factor in `ray_march_galaxy` is a rough calibration.
  It should be adjusted after visual testing — if the 3D view is too dark,
  increase it; if too bright, decrease it.
- The `RAY_STEPS` constant (64) is a performance/quality tradeoff.  Increase
  for smoother images at the cost of GPU time.  Each doubling adds ~1 ms to
  render time at 1920×1080 on a mid-range GPU.
- The camera up-vector is hardcoded to (0, 1, 0).  This is correct for an
  orbit camera around a galaxy sitting in the XZ plane (face-on = looking
  down the Y axis).  If the galaxy orientation changes, the up-vector
  convention must be revisited.
- When plan 012 (instanced stars) is executed, the `ray_march_galaxy`
  function can simplify — it won't need to sample the star grid for
  color at each step, since the instanced pass will provide the stars.
  The smooth glow can use a constant emissivity-per-density.
