# Plan 024: Remove 3D glow, unify 2D/3D star rendering

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat e240b75..HEAD -- galaxy-shader/src/lib.rs src/gpu.rs src/app.rs src/render.rs src/input.rs src/ui.rs src/camera.rs src/stars.wgsl`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MED (multi-file restructuring; SPIR-V staleness risk)
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `e240b75`, 2026-06-26

## Why this matters

The 3D ray-march glow path (`ray_march_galaxy`, 3D density functions, dust
extinction) is heavy on the GPU and serves no gameplay purpose — the project
is a star map, not a volumetric renderer.  Two different star-sampling
strategies (shader hash-grid in 2D vs CPU catalogue in 3D) mean 2D and 3D
views disagree on star positions.  Removing the 3D glow path, unifying both
modes on the same star catalogue buffer, and stripping the sidebar to
view-only controls shrinks the uniform struct by 10 fields, eliminates ~400
lines of dead shader code, and makes the star rendering consistent between
modes.

## Current state

**Relevant files:**

- `galaxy-shader/src/lib.rs` — SPIR-V compute shader; `render_scene` branches
  on `render_mode` (2D via `sample_star_grid`, 3D via `ray_march_galaxy`)
- `src/gpu.rs` — `GalaxyUniform` (28 fields, 112 bytes), `from_params()`,
  `GpuCompute`, `GpuStars`, `generate_star_catalogue()`, host-side replicas
- `src/stars.wgsl` — instanced star vertex/fragment shader; `StarParams { brightness, aspect, star_size }`
- `src/app.rs` — `App` struct fields: `render_mode`, `show_glow`, `dust_tau`,
  `exposure`, `contrast`, `star_brightness`, `star_size`
- `src/render.rs` — per-frame dispatch: compute + display + instanced stars
- `src/input.rs` — 2D pan/zoom, 3D orbit, arrow-key exposure/contrast
- `src/ui.rs` — egui sidebar: preset dropdown, ~30 parameter sliders, mode toggle
- `src/camera.rs` — `Camera` with orbit, zoom, `view_proj_matrix` (perspective only)

**GalaxyUniform (galaxy-shader/src/lib.rs, lines 15–41) — 10 fields to remove:**

```rust
#[derive(Copy, Clone)]
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
    // ── 3D camera / render mode ──        ← these 10 go
    pub render_mode: u32,
    pub camera_x: f32,
    pub camera_y: f32,
    pub camera_z: f32,
    pub camera_target_x: f32,
    pub camera_target_y: f32,
    pub camera_target_z: f32,
    pub fov_y_deg: f32,
    pub dust_tau: f32,
}
```

**render_scene dispatch (galaxy-shader/src/lib.rs, ~lines 360–390):**

```rust
let rgb = if params.render_mode == 0 {
    // ── 2D path ──
    let col_dens = column_density(wx, wz, params);
    sample_star_grid(wx, wz, pixel_w, pixel_h, col_dens)
} else {
    // ── 3D ray-march path ──   ← this entire branch goes
    ray_march_galaxy(px, py, params)
};
```

**StarParams in stars.wgsl (lines 5–11):**

```wgsl
struct StarParams {
    brightness: f32,
    aspect: f32,
    star_size: f32,
}
@group(0) @binding(2) var<uniform> star_params: StarParams;
```

**Sidebar parameter sections (src/ui.rs, lines 46–115) — all sliders to remove:**

```rust
ui.separator();
ui.label("Disk");
let mut changed = false;

changed |= ui.add(egui::Slider::new(&mut app.params.disk_scale_length, …)).changed();
changed |= ui.add(egui::Slider::new(&mut app.params.disk_scale_height, …)).changed();
changed |= ui.add(egui::Slider::new(&mut app.params.disk_central_density, …)).changed();

ui.separator();
ui.label("Spiral Arms");
changed |= ui.add(egui::Slider::new(&mut app.params.arm_count, …)).changed();
changed |= ui.add(egui::Slider::new(&mut app.params.arm_pitch, …)).changed();
changed |= ui.add(egui::Slider::new(&mut app.params.arm_concentration, …)).changed();
changed |= ui.add(egui::Slider::new(&mut app.params.arm_strength, …)).changed();

ui.separator();
ui.label("Bulge");   // 2 sliders
ui.separator();
ui.label("Halo");    // 3 sliders
```

**App fields to remove (src/app.rs, lines ~58–68):**

```rust
pub render_mode: u32,
// ...
pub show_stars: bool,
pub show_glow: bool,
pub star_brightness: f32,
pub star_size: f32,
pub dust_tau: f32,
```

**Exposure/contrast on App (src/app.rs, line ~47):**

```rust
pub exposure: f32,
pub contrast: f32,
```

**do_compute guard (src/render.rs, lines ~97–98):**

```rust
let do_compute = render_mode != 1 || show_glow;
// returns: render_mode == 0 → true, render_mode == 1 && show_glow → true
```

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Check | `cargo check` | exit 0 |
| Lint | `cargo clippy -- -D warnings` | exit 0, no warnings |
| Test | `cargo test` | all pass |
| Clean rebuild (SPIR-V) | `cargo clean && cargo build` | exit 0 (only when shader changes) |

## Scope

**In scope** (only files you may modify):
- `galaxy-shader/src/lib.rs`
- `src/gpu.rs`
- `src/stars.wgsl`
- `src/app.rs`
- `src/render.rs`
- `src/input.rs`
- `src/ui.rs`
- `src/camera.rs`

**Out of scope** (do NOT touch):
- `galaxy-shader/Cargo.toml` — no dependency changes
- `Cargo.toml` — no crate dependency changes
- `build.rs` — spirv-builder config stays (SPIR-V compute shader still needed for 2D glow)
- `src/display.wgsl` — unchanged
- `src/galaxy.rs` — preset constructors unchanged
- `plans/README.md` — reviewer updates this after acceptance

## Git workflow

- Branch: none — work directly on `master` per project convention
- Commit message style: `"<imperative>: <short summary>"` (e.g. `"refactor: remove 3D glow, unify 2D/3D star rendering"`)
- Do NOT push or open a PR

## Steps

### Step 1: Remove 3D code from compute shader

Edit `galaxy-shader/src/lib.rs`:

1. Remove from `GalaxyUniform` (lines 31–41): `render_mode`, `camera_x/y/z`,
   `camera_target_x/y/z`, `fov_y_deg`, `dust_tau` — 10 fields. The struct
   ends after `log_contrast`.

2. Remove the `render_mode` branch in `render_scene` (~line 365). Replace:

   ```rust
   let rgb = if params.render_mode == 0 {
       let col_dens = column_density(wx, wz, params);
       sample_star_grid(wx, wz, pixel_w, pixel_h, col_dens)
   } else {
       ray_march_galaxy(px, py, params)
   };
   ```

   with just the 2D path (no star sampling — only column density):

   ```rust
   let col_dens = column_density(wx, wz, params);
   ```

   Then tone-map `col_dens` — but since `col_dens` is a scalar (not `Vec3`),
   create a gray `Vec3` from it:

   ```rust
   let gray = col_dens * 0.5; // emissivity factor
   let rgb = Vec3::new(gray, gray, gray);
   ```

3. Delete these functions and their constants:
   - `disk_density_3d()`, `bulge_density_3d()`, `halo_density_3d()`
   - `ray_march_galaxy()` + `RAY_STEPS`, `RAY_MAX_DIST`, `EMISSIVITY`, `DUST_R/G/B`
   - `sample_star_grid()` + `cell_has_star()`, `cell_star_light()`,
     `sample_imf_from_cell()`, `mass_to_lum()`, `mass_to_temp()`,
     `temperature_to_rgb()`, `COLOR_LUT`/`COLOR_LUT_LEN`, `star_cell()`,
     `WINDOW_SIDE`, `STAR_CELL_SIZE`, `INV_STAR_CELL_SIZE`, `STAR_OFFSET`,
     `hash3()`, `IMF_SEG_THRESH`/`IMF1_*`/`IMF2_*`, `T_SOLAR`, `M_SOLAR`,
     `L_BREAK`, `rem_euclid()`

4. Keep (do NOT delete):
   - `column_density()`, `disk_column()`, `bulge_column()`, `halo_column()`,
     `arm_modulation_2d()` — these produce the glow
   - `PI`, `TAU` — used by column functions
   - Tone-mapping code after the `rgb` computation

**Verify**: `cargo check` — will fail because host-side code still references removed fields. This is expected; continue to step 2.

### Step 2: Update host-side GalaxyUniform and from_params()

Edit `src/gpu.rs`:

1. Remove the same 10 fields from `GalaxyUniform` struct. After cutoff, the
   struct is 18 fields (12 f32s + 4 u32s + 2 f32s = 72 bytes).

2. Update `from_params()` — remove `render_mode`, `camera_pos`, `camera_target`,
   `fov_y_deg`, `dust_tau` parameters. The signature becomes:

   ```rust
   pub fn from_params(
       params: &GalaxyParams,
       image_w: u32, image_h: u32,
       extent: f64, center_x: f64, center_y: f64,
       exposure: f32, contrast: f32,
   ) -> Self
   ```

   The function body removes the 10 field assignments.

3. Delete host-side replicas:
   - `disk_column_host()`, `bulge_column_host()`, `halo_column_host()`,
     `arm_modulation_host()` — these are already duplicated in the test module
     (`host_disk_column` etc.); remove the non-test duplicates
   - `cell_has_star_host()`, `cell_star_light_host()`, `sample_imf_host()`,
     `mass_to_temp_host()`, `mass_to_lum_f64()`, `hash3_host()`,
     `star_cell_host()` — these mirrored shader functions that no longer exist

4. Keep:
   - `generate_star_catalogue()` and its helpers (`CATALOGUE_CELL_SIZE`,
     `CATALOGUE_MASS_THRESHOLD`, internal `disk_column_host`/`bulge_column_host`/
     `halo_column_host`/`arm_modulation_host` — these are local to the function)
   - `StarInstance`, `GpuCompute`, `GpuStars`, `compute_galaxy()`
   - All test functions that still compile (update failing ones in step 7)

**Verify**: `cargo check` → exit 0 (or only errors in app.rs/render.rs/input.rs — proceed to steps 3–5 to resolve)

### Step 3: Update App struct

Edit `src/app.rs`:

1. Remove fields: `render_mode: u32`, `show_glow: bool`, `dust_tau: f32`,
   `exposure: f32`, `contrast: f32`, `star_brightness: f32`

2. Add field: `pub is_3d: bool` (default `false`)

3. Rename/keep: `star_brightness_2d: f32` (default `1.0`; replaces old `star_brightness`).

4. Change `star_size` default from `1.0` to `0.05`.

5. Remove `DEFAULT_EXPOSURE` and `DEFAULT_CONTRAST` constants (now fixed values
   inside the compute shader — the uniforms still carry `exposure` and
   `log_contrast` fields, written with fixed values in step 4).

6. Simplify `update_title()` — remove exposure/contrast from the format string:

   ```rust
   pub fn update_title(&self) {
       if let Some(window) = &self.window {
           window.set_title("Galaxy Gen");
       }
   }
   ```

**Verify**: `cargo check` → exit 0 (errors may remain in render.rs/ui.rs/input.rs)

### Step 4: Update render.rs dispatch

Edit `src/render.rs`:

1. Replace the `do_compute` guard:

   Old: `let do_compute = render_mode != 1 || show_glow;`

   New: `let do_compute = !app.is_3d;` (compute only runs in 2D mode)

2. Remove `render_mode`, `show_glow`, `fov_y` from the destructured locals.
   Replace with `let is_3d = app.is_3d;`.

3. Update `GalaxyUniform::from_params()` call — pass fixed tonemap values and
   remove the 3D-only args:

   ```rust
   let uniform_data = gpu::GalaxyUniform::from_params(
       &app.params,
       app.render_w, app.render_h,
       app.extent_ly,
       app.center_x, app.center_y,
       0.25,  // fixed exposure
       0.04,  // fixed log_contrast
   );
   ```

4. Write the star-camera matrix per mode:
   - 2D: compute an orthographic matrix from `app.center_x`, `app.center_y`,
     `app.extent_ly`, then call `write_view_proj_matrix` with it
   - 3D: use `app.camera.view_proj_matrix(aspect)` as before

   Add a helper on `Camera` (in step 5) and call it here.

5. Write `StarParams` per mode in `write_view_proj_matrix`:
   - 2D: `brightness = app.star_brightness_2d`
   - 3D: `brightness = 1.0`

6. In 2D mode, the display pass runs before the stars pass (glow texture →
   frame, then stars on top). In 3D mode, no display pass, stars draw on
   black. This is already gated by `do_compute` for the display pass;
   the stars pass already runs unconditionally when catalogue is non-empty.

**Verify**: `cargo check` → exit 0

### Step 5: Add ortho matrix to Camera

Edit `src/camera.rs`:

Add a method that builds an orthographic projection matrix for the 2D top-down
view. The camera looks down the Y axis at the XZ plane:

```rust
/// Orthographic projection for 2D top-down mode.
/// Maps world XZ coords to clip space.  Y (height) is unused in 2D.
pub fn ortho_proj_matrix(
    extent_x: f32,
    extent_y: f32,
    center_x: f32,
    center_y: f32,
    near: f32,
    far: f32,
) -> glam::Mat4 {
    let left = center_x - extent_x * 0.5;
    let right = center_x + extent_x * 0.5;
    let bottom = center_y - extent_y * 0.5;
    let top = center_y + extent_y * 0.5;
    // Camera looks down Y axis (from +Y toward XZ plane)
    let eye = glam::Vec3::new(0.0, far * 0.5, 0.0);
    let target = glam::Vec3::new(0.0, 0.0, 0.0);
    let up = glam::Vec3::NEG_Z; // Z = up on screen
    let view = glam::Mat4::look_at_rh(eye, target, up);
    let proj = glam::Mat4::orthographic_rh(left, right, bottom, top, near, far);
    proj * view
}
```

Use `CAMERA_NEAR` and `CAMERA_FAR` for near/far.

**Verify**: `cargo check` → exit 0

### Step 6: Update input.rs

Edit `src/input.rs`:

1. Replace `app.render_mode == 1` checks with `app.is_3d` (two locations:
   the `MouseInput` handler for `orbit_dragging`, and the `MouseWheel` handler
   for orbit zoom vs 2D zoom).

2. Remove the `KeyCode::ArrowLeft/Right/Up/Down` handler block (exposure/contrast
   keyboard shortcuts — those fields are removed):

   ```rust
   KeyCode::ArrowLeft => app.exposure -= EXPOSURE_STEP,
   KeyCode::ArrowRight => app.exposure += EXPOSURE_STEP,
   KeyCode::ArrowUp => app.contrast += CONTRAST_STEP,
   KeyCode::ArrowDown => app.contrast -= CONTRAST_STEP,
   ```

   Remove the `_ => return,` line that was the fallback for keys not matching.

3. Remove `app.update_title()` calls (title is now static, no exposure/contrast).
   Remove `EXPOSURE_STEP` and `CONTRAST_STEP` constants.

**Verify**: `cargo check` → exit 0

### Step 7: Update tests

In `src/gpu.rs` `#[cfg(test)] mod tests`:

1. Update `galaxy_uniform_field_offsets_match_shader_layout` — change the
   `size_of::<GalaxyUniform>()` assertion from 112 to the new size (72).
   Recalculate and assert each remaining field's offset.
   The remaining fields in order are: disk_scale_length(0), disk_scale_height(4),
   disk_central_density(8), arm_count(12), arm_pitch(16), arm_concentration(20),
   arm_strength(24), bulge_radius(28), bulge_central_density(32),
   halo_radius(36), halo_central_density(40), halo_slope(44), image_width(48),
   image_height(52), extent(56), center_x(60), center_y(64), exposure(68),
   log_contrast(72).

   Wait — 19 fields × 4 bytes = 76. Recalculate. The final offset after
   `log_contrast` is 72, making the struct 76 bytes. So the assertion becomes
   `assert_eq!(size_of::<GalaxyUniform>(), 76);` and the last offset is
   `offset_of!(GalaxyUniform, log_contrast) == 72`.

2. Update `from_params_preserves_values` and `from_params_preserves_values_ngc628`
   — remove assertions for the 10 deleted fields. Remove the camera/fov/dust
   arguments from the `from_params` calls (signature changed in step 2).

3. Remove test functions that validated deleted functions:
   - `temperature_to_rgb_sun_is_white`
   - `temperature_to_rgb_no_green_stars`
   - `no_channel_is_ever_the_maximum_alone`
   - `temperature_to_rgb_m_dwarfs_are_orange`
   - `temperature_to_rgb_o_stars_are_blue`
   - `temperature_to_rgb_monotonic_channels`
   - `cell_star_light_with_new_teff_gives_plausible_colors`
   - `lut_entries_are_sorted_by_temperature`
   - `mass_to_temp_produces_correct_teff`
   - `mass_to_temp_monotonic`
   - Any test referencing `sample_star_grid`, `cell_has_star`, `cell_star_light`, `disk_density_3d`, `ray_march_galaxy`

4. Keep: `mass_to_temp_host`, `temperature_to_rgb_host`, `COLOR_LUT_HOST` and
   their tests — these validate the LUT used by `stars.wgsl`.

5. Keep all column-profile tests (`disk_column_*`, `bulge_column_*`,
   `halo_column_*`, `arm_modulation_*`, `ngc628_*`, `from_params_*`,
   `uniform_*`) — update only those referencing removed fields.

**Verify**: `cargo test` → all tests pass, correct count of removed tests

### Step 8: AABB cull filter for 2D mode

Add a function in `src/gpu.rs`:

```rust
/// Filter star instances to those within the 2D ortho viewport rectangle.
/// Returns a subset of `catalogue` whose (pos_x, pos_z) fall within the
/// given world-space XZ bounds.
pub fn cull_stars_to_viewport(
    catalogue: &[StarInstance],
    min_x: f64, max_x: f64,
    min_z: f64, max_z: f64,
) -> Vec<StarInstance> {
    catalogue.iter()
        .filter(|s| {
            let x = s.pos_x as f64;
            let z = s.pos_z as f64;
            x >= min_x && x <= max_x && z >= min_z && z <= max_z
        })
        .copied()
        .collect()
}
```

In `src/render.rs`, before the 2D star upload, compute the viewport bounds
and filter:

```rust
if !app.is_3d {
    let extent_y = app.extent_ly * (app.render_h as f64 / app.render_w as f64);
    let half_x = app.extent_ly * 0.5;
    let half_z = extent_y * 0.5;
    let visible = gpu::cull_stars_to_viewport(
        &app.star_catalogue,
        app.center_x - half_x, app.center_x + half_x,
        app.center_y - half_z, app.center_y + half_z,
    );
    // Write visible subset instead of full catalogue
    // (header: count = visible.len(), capacity = MAX_STARS)
    let mut header = [0u32; 2];
    header[0] = visible.len() as u32;
    header[1] = gpu::MAX_STARS;
    queue.write_buffer(&gpu_stars.instance_buffer, 0, bytemuck::cast_slice(&header));
    let data_bytes = bytemuck::cast_slice(&visible);
    queue.write_buffer(&gpu_stars.instance_buffer, 8, data_bytes);
}
```

For 3D mode, continue uploading the full catalogue as before (when
`star_catalogue_uploaded` is false). Remove the `star_catalogue_uploaded`
guard for 2D mode — it re-uploads every frame.

**Verify**: `cargo check` + `cargo clippy -- -D warnings` → exit 0

### Step 9: Mode-switch view preservation

In `src/ui.rs`, in the 2D/3D toggle handler:

```rust
if ui.add(egui::Checkbox::new(&mut app.is_3d, "3D Mode")).changed() {
    if app.is_3d {
        // 2D → 3D: map center/extent to camera
        app.camera.target = glam::Vec3::new(
            app.center_x as f32, 0.0, app.center_y as f32
        );
        let fov_rad = app.camera.fov_y_deg.to_radians();
        app.camera.dist = (app.extent_ly as f32 / (2.0 * (0.5 * fov_rad).tan()))
            .clamp(crate::camera::CAMERA_DIST_MIN,
                   crate::camera::CAMERA_DIST_MAX);
        app.camera.azimuth = 0.0;
        app.camera.elevation = std::f32::consts::FRAC_PI_2 - 0.05;
    } else {
        // 3D → 2D: project camera FOV onto XZ plane
        let fov_rad = app.camera.fov_y_deg.to_radians();
        app.extent_ly = (2.0 * app.camera.dist * (0.5 * fov_rad).tan()) as f64
            .clamp(MIN_EXTENT_LY, MAX_EXTENT_LY);
        app.center_x = app.camera.target.x as f64;
        app.center_y = app.camera.target.z as f64;
    }
    app.needs_render = true;
}
```

**Verify**: `cargo check` → exit 0

### Step 10: Strip sidebar to view controls

Edit `src/ui.rs`:

Remove all parameter slider sections:
- "Disk" section (3 sliders: scale_length, scale_height, central_density)
- "Spiral Arms" section (4 sliders: arm_count, arm_pitch, arm_concentration, arm_strength)
- "Bulge" section (2 sliders: bulge_radius, bulge_central_density)
- "Halo" section (3 sliders: halo_radius, halo_central_density, halo_slope)
- Exposure and contrast sliders in "Brightness" section
- "Show Glow" checkbox in "3D View" section
- "Dust τ" slider in "3D View" section
- `let mut changed = false;` + `if changed { app.needs_render = true; }` — no more slider-driven changes

Replace "3D Mode" checkbox with the toggle from step 9.

The sidebar after strip-down:

| Section | Controls |
|---------|----------|
| Preset | Dropdown (Milky Way, NGC 628, M31, M51, M101) |
| Brightness | Star brightness slider (2D only, 0.0–2.0, default 1.0) — `app.star_brightness_2d` |
| 3D View | 2D/3D toggle, Distance slider (3D only, 5k–500k), FOV slider (3D only, 5–120°), Star size slider (0.01–0.5, default 0.05) |
| Actions | Screenshot button, Reset Camera button |
| Footer | Frame time display |

Star size slider applies to both modes (same `star_params.star_size` uniform).

Remove the `"Catalogue: N stars (synced/dirty)"` debug label (no longer useful).

**Verify**: `cargo check` + `cargo clippy -- -D warnings` → exit 0 (warnings for unused imports from removed code are acceptable — clean them up)

### Step 11: Cleanup and final verification

1. Run `cargo clippy -- -D warnings` — fix any warnings introduced by the changes.
2. Run `cargo test` — confirm all tests pass. Count the test output to ensure
   no unexpected test removals.
3. After all host-side changes, **the SPIR-V binary is stale**. Run:

   ```
   cargo clean
   cargo build
   ```

   This recompiles `galaxy-shader/src/lib.rs` to SPIR-V via `spirv-builder`.
   Without this step, the compute shader still contains the deleted functions
   and the old `GalaxyUniform` layout — the app will render with the old
   behavior or crash on uniform size mismatch.

**Verify**: `cargo run` → window opens, 2D mode shows glow + stars, 3D mode
shows stars on black. Toggle works, preset switch works, star size slider works.

## Test plan

- Existing column-profile tests (`disk_column_*`, `bulge_column_*`,
  `arm_modulation_*`, `ngc628_*`, `m31_*`, `m51_*`, `m101_*`) — must all pass
- `galaxy_uniform_field_offsets_match_shader_layout` — updated for 76-byte struct
- `from_params_preserves_values` / `from_params_preserves_values_ngc628` — updated signatures
- `uniform_is_pod` — stays, updated struct
- `uniform_struct_size_matches_fields` — update expected field count
- `generate_star_catalogue` tests (`src/gpu.rs`, plan 018 tests) — must all pass
  (deterministic, no_nan, bounds, extent, bulge_enriched, presets_differ, ngc_extent)
- All galaxy preset tests in `src/galaxy.rs` — must all pass (unchanged)
- Add: test for `cull_stars_to_viewport` — empty catalogue returns empty, all
  stars inside bounds, all stars outside bounds, mix of in/out
- Add: `Camera::ortho_proj_matrix` — sanity test: center (0,0) with extent 100
  maps screen corners to (±50, ±50) in world space

**Verify**: `cargo test` → all tests pass (including new ones)

## Done criteria

Machine-checkable — ALL must hold:

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0; test count ≥ (old count − removed + 2 new)
- [ ] `grep -rn "render_mode" src/ galaxy-shader/src/` returns zero matches
- [ ] `grep -rn "ray_march_galaxy" src/ galaxy-shader/src/` returns zero matches
- [ ] `grep -rn "show_glow" src/` returns zero matches
- [ ] `grep -rn "dust_tau" src/ galaxy-shader/src/` returns zero matches
- [ ] `grep -rn "exposure" src/ui.rs` returns zero matches
- [ ] `grep -rn "contrast" src/ui.rs` returns zero matches
- [ ] `grep -rn "disk_scale_length" src/ui.rs` returns zero matches (no param slider)
- [ ] `grep -rn "star_brightness\b" src/ui.rs` returns at most a `star_brightness_2d` match
- [ ] `git status` shows only files in the in-scope list are modified
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

1. **SPIR-V staleness**: After modifying `galaxy-shader/src/lib.rs`, running
   `cargo check && cargo test` passes but the glow appearance is unchanged when
   running the app. The SPIR-V binary is stale. Run `cargo clean && cargo build`
   and retest.

2. **Struct mismatch**: If `galaxy_uniform_field_offsets_match_shader_layout`
   test fails, the host struct and shader struct are out of sync. Update both
   sides to match exactly (same fields, same order, same types — f32 vs u32
   must match).

3. **stars.wgsl compilation error**: If the `stars.wgsl` shader fails to compile
   after changing `StarParams`, check the buffer size matches (buffer created
   in `GpuStars::new`).

4. **SPIR-V build failure**: If `cargo build` fails during SPIR-V compilation
   (errors from `spirv-builder`), check that `galaxy-shader/src/lib.rs` has no
   references to deleted functions/types. Check that `#[allow(unused)]` items
   are not deleted if they're needed by live code.

5. **Gallery render test failure**: If the app runs but the 2D glow is not
   visible, check the fixed exposure/contrast values (0.25 / 0.04). The
   tonemapping formula is `log_lum * log_contrast + exposure` — if both are
   too low, glow may be invisible. Also check the emissivity factor
   `col_dens * 0.5` — too low and the glow vanishes.

6. **Step verification fails twice** after a reasonable fix attempt.

7. **A file modification is needed but is in the out-of-scope list.**

## Maintenance notes

- The `exposure` and `log_contrast` fields remain in `GalaxyUniform` (shader
  and host) but are now hardcoded to 0.25 and 0.04. If the glow tonemapping
  needs adjustment, change these constants in `src/render.rs` (the `from_params`
  call) and in `galaxy-shader/src/lib.rs` (the tone-map math).
- The `star_brightness_2d` field maps to `StarParams.brightness` (WGSL field
  name differs from Rust field name). If adding a 3D brightness control later,
  keep this mapping in mind — both modes share the same `StarParams` struct.
- The ortho matrix in `Camera::ortho_proj_matrix` uses `look_at_rh` looking
  down the Y axis with `up = NEG_Z`. If the star cloud appears flipped in 2D,
  check the screen-coordinate-to-world-coordinate mapping between the compute
  shader (wx/wz) and the ortho matrix.
- AABB culling is per-frame in 2D mode — the instance buffer is re-uploaded
  every frame. When the catalogue moves to on-GPU generation, the CPU cull
  filter becomes unnecessary (the compute shader writes only visible stars).
- The `SPIR-V` binary staleness problem (step 11, stop condition 1) will
  persist as long as `spirv-builder` is used. Any future shader change must
  include `cargo clean` in the build step.
