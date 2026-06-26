# 024 — Remove 3D glow, unify 2D/3D star rendering

**Decision**: Cut the 3D ray-march glow path entirely. Keep 2D compute shader for shape overview, but replace `sample_star_grid()` with the same star catalogue used in the instanced pass. Both modes read the same deterministic star buffer. Mode-switching preserves view position.

**Priority**: P1 | **Effort**: L | **Depends on**: — | **Status**: TODO

---

## Motivation

- 3D ray-march glow is heavy on the GPU and serves no gameplay purpose
- Two different star sampling strategies (shader hash grid vs CPU catalogue) means 2D and 3D views disagree on star positions
- The long-term goal is a GPU compute shader that writes star instances directly; unifying on the catalogue buffer makes that transition seamless — one buffer, all consumers read from it
- Removing 10 camera/dust/render_mode fields shrinks the uniform struct, shader complexity, and host-side plumbing

---

## Cuts (remove)

### `galaxy-shader/src/lib.rs`
- `render_mode`, `camera_x/y/z`, `camera_target_x/y/z`, `fov_y_deg`, `dust_tau` — 10 fields from `GalaxyUniform`
- `disk_density_3d()`, `bulge_density_3d()`, `halo_density_3d()`
- `ray_march_galaxy()` + constants `RAY_STEPS`, `RAY_MAX_DIST`, `EMISSIVITY`, `DUST_R/G/B`
- `sample_star_grid()` + `cell_has_star()`, `cell_star_light()`, `sample_imf_from_cell()`, `mass_to_lum()`, `mass_to_temp()`, `temperature_to_rgb()`, `COLOR_LUT`, `star_cell()`, `WINDOW_SIDE`/`STAR_CELL_SIZE`/etc, `hash3()`
- `render_scene` branch — remove the `if render_mode == 0 … else …`; keep only the 2D path (column density + tone-map)

### `src/gpu.rs`
- Those same 10 fields from `GalaxyUniform`, and their arguments in `from_params()`
- `disk_density_3d`, `bulge_density_3d`, `halo_density_3d` host-side replicas (if any)
- Tests that validate removed fields/offsets — update `galaxy_uniform_field_offsets_match_shader_layout` to new struct size
- Host-side replicas of shader star functions (`cell_has_star_host`, `cell_star_light_host`, etc.) — these only existed to mirror shader code we deleted

### `src/app.rs`
- `render_mode: u32` field
- `show_glow: bool` field
- `dust_tau: f32` field
- `exposure: f32` field
- `contrast: f32` field
- `star_brightness: f32` field (was 3D-only; replaced by `star_brightness_2d: f32` for 2D mode)
- Add `star_brightness_2d: f32` (default `1.0`, range `[0.0, 2.0]`) — only exposed as slider in 2D mode
- Change `star_size` default from `1.0` to `0.05` (current default is too large)
- Add `is_3d: bool` for the 2D/3D mode toggle
- Simplify `update_title()` — remove exposure/contrast from window title text

### `src/ui.rs`
- Strip sidebar to view controls only (see "Sidebar strip-down" below)
- Remove all parameter sliders: disk (3), spiral arms (4), bulge (2), halo (3)
- Remove "Show Glow" checkbox
- Remove "Dust τ" slider
- Replace "3D Mode" checkbox with "2D / 3D" toggle (preserves view position)
- Keep: preset dropdown, exposure + contrast, star size, camera distance + FOV, screenshot, reset camera
- `render_mode` references → use `app.is_3d`

### `src/render.rs`
- The `render_mode != 1 || show_glow` conditional — replace with: run compute only in 2D, never in 3D
- The display pass only runs in 2D mode (since 3D has no glow texture to display)
- `write_view_proj_matrix` writes brightness from `app.star_brightness_2d` in 2D mode, `1.0` in 3D mode

### `stars.wgsl`
- Keep `brightness` field in `StarParams` — used for 2D star brightness (3D always sets it to 1.0)
- Alpha stays: `clamp(lum * 0.5 / (lum * 0.5 + 1.0), 0.0, 1.0) * star_params.brightness`

### `GpuStars` (`src/gpu.rs`)
- `brightness_buffer` stays — it now carries `{brightness_2d, aspect, star_size}`
- 2D mode writes brightness from slider; 3D mode writes `1.0`

### `src/input.rs`
- The `if app.render_mode == 1` branches → `if app.is_3d`
- 2D pan/drag logic stays (only active in 2D mode)
- 3D orbit logic stays (only active in 3D mode)
- Remove the `KeyCode::ArrowLeft/Right/Up/Down` handlers for exposure/contrast (those sliders are gone)

### Tests
- Remove tests that validate removed functions: `temperature_to_rgb_*`, `mass_to_temp_*`, `no_channel_is_ever_the_maximum_alone`, `cell_star_light_with_new_teff_*`, `lut_entries_are_sorted_by_temperature`
- Update `galaxy_uniform_field_offsets_match_shader_layout` for reduced struct size

---

## Additions / changes

### 1. 2D mode uses instanced stars pass with orthographic projection

In 2D mode, run the instanced stars pass (`stars.wgsl`) with an orthographic (top-down) camera matrix instead of the orbit perspective matrix. The stars.wgsl shader already reads `camera.view_proj` — it works with any matrix. No shader changes needed.

- `Camera` gains `ortho_proj_matrix(extent_x, extent_y, center_x, center_y, near, far) -> Mat4`
- 2D rendering order: compute (glow → texture) → display pass (texture → frame) → stars pass (ortho matrix → on top)
- 3D rendering order: no compute, no display pass → stars pass (orbit matrix → on black)

### 2. Mode-switch preserves view position

When toggling 2D → 3D:
- Set `camera.target` = `(center_x, 0, center_y)` (midplane, world coords)
- Set `camera.dist` so the FOV footprint roughly matches current `extent_ly`
  - `camera.dist ≈ extent_ly / (2 * tan(fov_y/2))`
- Preserve `azimuth = 0`, `elevation ≈ π/2` (looking down)

When toggling 3D → 2D:
- Project the current camera FOV footprint onto the XZ plane at y=0
- `extent_ly ≈ 2 * camera.dist * tan(fov_y/2)`
- `center_x = camera.target.x`, `center_y = camera.target.z`

### 3. Compute shader simplification

`render_scene` in `galaxy-shader/src/lib.rs` becomes:

```
fn render_scene(…) {
    // 2D path only — column density → tone-map
    let wx = …; let wz = …; // screen → world
    let col_dens = column_density(wx, wz, params);
    // tone-map col_dens → rgba (same luminance-based stretch as before)
    // write to rgba buffer
}
```

No star sampling in the compute shader. Just the smooth column-density field. Stars come from the instanced pass in both modes.

### 4. Update struct guard tests

- `GalaxyUniform` size test must match new struct (fewer fields → smaller)
- Offset tests for remaining fields must be updated
- `from_params_preserves_values` tests remove camera/dust/render_mode arguments

### 5. 2D AABB cull filter

Before uploading the star instance buffer in 2D mode, filter the full catalogue to only stars within the current ortho viewport bounds:
- `bounds_x = [center_x - extent/2, center_x + extent/2]`
- `bounds_z = [center_y - extent_y/2, center_y + extent_y/2]` (y = Z in world coords)
- Upload only the filtered subset → fewer vertex shader invocations
- At overview zoom (512k LY extent), most stars pass through. At close zoom (10k LY extent), only ~5-10% pass.
- 3D mode continues uploading all stars for now (GPU handles frustum culling via clip-space rejection).

---

## Sidebar strip-down

Current sidebar: ~30 parameter sliders spread across Disk, Spiral Arms, Bulge, Halo, Brightness, 3D View sections.

After strip-down, the sidebar contains only view controls and preset selection:

| Section | Controls |
|---------|----------|
| Preset | Dropdown (Milky Way, NGC 628, M31, M51, M101) |
| Brightness | Star brightness slider (2D only) |
| 3D View | 2D/3D toggle, Distance slider (3D only), FOV slider (3D only), Star size slider |
| Actions | Screenshot button, Reset Camera button |
| Footer | Frame time display |

Removed: all Disk, Spiral Arms, Bulge, Halo sliders; Show Glow checkbox; Dust τ slider; Exposure and Contrast sliders (both removed — glow is fixed at a good default tonemapping).

Star size default: `0.05`, range `[0.01, 0.5]` (was: default `1.0`, range `[0.01, 1.0]`).

## What stays

| Component | Fate |
|-----------|------|
| `galaxy-shader` crate, `build.rs`, nightly Rust | Kept (2D glow compute shader) |
| `GpuCompute`, `rgba_buffer`, `uniform_buffer` | Kept |
| `display.wgsl`, display pass | Kept (2D only) |
| `stars.wgsl`, `GpuStars`, instance buffer | Kept (both modes) |
| `generate_star_catalogue()`, `StarInstance` | Kept |
| `Camera` struct, orbit controls | Kept (3D only) |
| All presets (`milky_way`, `ngc628`, `m31`, `m51`, `m101`) | Kept |
| `column_density`, `disk_column`, `bulge_column`, `halo_column`, `arm_modulation_2d` (shader + host) | Kept |
| Tone-mapping code | Kept |
| `hash3`, IMF sampling host-side | Kept (used in catalogue generation) |
| Temperature LUT, `mass_to_temp`, `temperature_to_rgb` host-side | Kept (used in star colour tests) |
| PNG snapshot | Kept |

---

## Verification

- `cargo check` + `cargo clippy -- -D warnings` + `cargo test` all pass
- 2D mode renders: smooth glow + star points from catalogue (ortho projection)
- 3D mode renders: star points from catalogue (orbit projection), black background
- Toggling between modes preserves the view region
- No regression in column density visual quality
- Star positions match between 2D and 3D views of the same region

---

## Notes

- The compute shader no longer needs `hash3`, IMF sampling, star colour LUT, or temperature conversion — those become dead shader code. Remove them from `galaxy-shader/src/lib.rs`.
- `stars.wgsl` keeps its own `temperature_to_rgb` and LUT — that's the single source of truth for star colour in both modes now.
- Host-side replicas of removed shader functions can be deleted from `src/gpu.rs` tests, but `mass_to_temp_host`, `temperature_to_rgb_host`, and `COLOR_LUT_HOST` in the test module stay — they validate the colour LUT which is still used in `stars.wgsl`.
- Star brightness in 2D: new `star_brightness_2d` slider controls the `brightness` uniform field in StarParams. Default 1.0, range [0.0, 2.0].
- Star brightness in 3D: always 1.0 (no slider). Stars render at intrinsic luminosity.
- Exposure and contrast sliders removed entirely. The compute shader uses fixed default values for tonemapping.
- 2D AABB cull filter: before writing the instance buffer, filter `star_catalogue` to only stars within the current ortho viewport. This avoids transforming 500k+ off-screen vertices in close-zoom 2D views. Per-frame upload cost is acceptable since the filtered count is much smaller at close zoom.
- If the `brightness_buffer` binding is removed from `GpuStars`, the `StarParams` uniform shrinks to `{aspect, star_size}` (8 bytes, padded to 16 for vec4 alignment).
