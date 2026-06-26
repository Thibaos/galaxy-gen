# Plan 025: Remove 2D compute glow, keep SPIR-V scaffolding

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat 0687985..HEAD -- galaxy-shader/src/lib.rs src/gpu.rs src/app.rs src/render.rs src/display.wgsl`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW (pure deletion; no logic changes; SPIR-V build stays)
- **Depends on**: 024 (DONE)
- **Category**: direction
- **Planned at**: commit `0687985`, 2026-06-26

## Why this matters

The 2D column-density compute shader (column-density glow → display pass) is
no longer needed — star-cloud point density shows disk + spiral structure at
overview zoom.  Removing it eliminates `GalaxyUniform`, column-density host
code, the `display.wgsl` fullscreen-quad pass, and the display-pass texture
pipeline.

**But**: the SPIR-V compute-shader infrastructure (`galaxy-shader/`, `build.rs`,
`rust-toolchain.toml`, nightly Rust) stays in place.  The compute shader
becomes an empty entry-point stub.  When on-GPU star generation is added
later, the author fills in the stub — no build-system re-setup, no nightly
re-introduction, no Cargo.toml churn.  Only the glow-specific code goes.

## Current state

**Relevant files:**

- `galaxy-shader/src/lib.rs` — compute shader with `GalaxyUniform`, `column_density`,
  `disk_column`, `bulge_column`, `halo_column`, `arm_modulation_2d`, tone-mapping,
  `render_scene` entry point
- `src/gpu.rs` — `GalaxyUniform` (18 fields), `GpuCompute`, `compute_galaxy()`,
  column-density host replicas
- `src/app.rs` — `gpu_compute`, `uniform_buffer`, `rgba_buffer`, display-pass fields
- `src/render.rs` — compute dispatch + display pass in `do_compute` block
- `src/display.wgsl` — fullscreen-quad fragment shader (samples glow texture, BGR swap)

**galaxy-shader/src/lib.rs entry point (line 125):**

```rust
pub fn render_scene(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] params: &GalaxyUniform,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] rgba: &mut [u32],
) {
    // ... computes wx, wz via ortho projection
    // calls column_density(wx, wz, params)
    // tone-maps to rgba[idx]
}
```

**App compute/display fields (src/app.rs, lines ~82–98 in `App::default()`):**

```rust
pub gpu_compute: Option<gpu::GpuCompute>,
pub uniform_buffer: Option<wgpu::Buffer>,
pub rgba_buffer: Option<wgpu::Buffer>,
pub rgba_buf_w: u32,
pub rgba_buf_h: u32,
pub texture: Option<wgpu::Texture>,
pub bind_group: Option<wgpu::BindGroup>,
pub bind_group_layout: Option<wgpu::BindGroupLayout>,
pub sampler: Option<wgpu::Sampler>,
pub display_params_buffer: Option<wgpu::Buffer>,
pub tex_w: u32,
pub tex_h: u32,
```

**App::init() display-pipeline setup (src/app.rs, lines ~174–281):**

- Lines ~174–178: display shader module creation
- Lines ~180–241: bind group layout + pipeline layout + render pipeline creation
- Lines ~243–245: `self.render_pipeline` / `bind_group_layout` / `sampler` assignment
- Lines ~248–258: `display_params_buffer` creation + write
- Line ~261: `self.gpu_compute = gpu::GpuCompute::new(...)`
- Line ~281: `self.recreate_texture()` call

`recreate_texture()` itself at lines ~285–332.

**Note: `needs_render` after this plan**: The field `app.needs_render` is written in `App::default()`, `src/ui.rs` (out of scope), and `render.rs` (after frame), but after the compute block is removed it has no reader.  This will trigger `dead_code` under `cargo clippy -- -D warnings`.  See step 3 for the fix (`#[allow(dead_code)]`).

**render.rs guard clause (lines ~7–18):**

```rust
if app.surface.is_none()
    || app.device.is_none()
    || app.queue.is_none()
    || app.config.is_none()
    || app.render_pipeline.is_none()
    || app.bind_group_layout.is_none()
    || app.sampler.is_none()
    || app.gpu_compute.is_none()
{
    return;
}
```

**do_compute block (render.rs, ~lines 92–118):**

```rust
let do_compute = !is_3d;
if app.needs_render && do_compute {
    // creates uniform_buffer, writes GalaxyUniform,
    // dispatches compute_galaxy(), runs display pass
}
```

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Check | `cargo check` | exit 0 |
| Lint | `cargo clippy -- -D warnings` | exit 0, no warnings |
| Test | `cargo test` | all pass |
| SPIR-V rebuild | `cargo clean && cargo build` | exit 0 (only needed after shader changes in step 1) |

## Scope

**In scope** (files you may modify):

- `galaxy-shader/src/lib.rs` — replace with empty entry point
- `src/gpu.rs` — remove `GalaxyUniform`, `GpuCompute`, `compute_galaxy()`, column-density host code, column-density tests
- `src/app.rs` — remove compute/display fields, `recreate_texture()`, display-bind-group init
- `src/render.rs` — remove compute dispatch, display pass, texture resize; simplify guard clause

**Deleted files**:
- `src/display.wgsl`

**Out of scope** (do NOT touch):

- `galaxy-shader/Cargo.toml` — keep (SPIR-V crate scaffold)
- `build.rs` — keep (spirv-builder build script)
- `Cargo.toml` — keep `[workspace]`, `spirv-builder` build dep, `wgpu` spirv feature, profile overrides
- `rust-toolchain.toml` — keep (nightly stays)
- `src/stars.wgsl` — unchanged
- `src/camera.rs` — unchanged
- `src/input.rs` — unchanged
- `src/ui.rs` — unchanged
- `src/galaxy.rs` — unchanged
- `src/main.rs` — unchanged (may need import cleanup)
- `GpuStars` and `generate_star_catalogue()` — unchanged

## Git workflow

- Branch: none — work directly on `master`
- Commit message style: imperative, short (`"refactor: remove 2D glow, keep SPIR-V scaffolding"`)
- Do NOT push or open a PR

## Steps

### Step 1: Replace compute shader with empty entry point

Edit `galaxy-shader/src/lib.rs` — replace entire contents:

```rust
#![cfg_attr(target_arch = "spirv", no_std)]

use spirv_std::glam::UVec3;
use spirv_std::spirv;

/// Compute shader entry point — staging for on-GPU star generation.
/// Currently does nothing; the 2D column-density glow has been removed.
#[spirv(compute(threads(8, 8, 1)))]
pub fn main_scene(
    #[spirv(global_invocation_id)] _id: UVec3,
) {
    // Placeholder — will be populated with instance-buffer-filling logic
}
```

All previous contents removed: `GalaxyUniform`, `column_density`, `disk_column`,
`bulge_column`, `halo_column`, `arm_modulation_2d`, `PI`, `render_scene`,
tone-mapping — everything.

**Verify**: `cargo check` — will fail because host-side code still references removed types/functions. This is expected; continue to step 2.

**After all host-side changes are done** (step 5), build SPIR-V from the new shader:

```
cargo clean
cargo build
```

### Step 2: Remove GPU compute types and functions from gpu.rs

Edit `src/gpu.rs`:

1. Delete `GalaxyUniform` struct and its `#[derive]` + `#[repr(C)]` (18 fields).

2. Delete `GalaxyUniform::from_params()` and any associated helper functions.

3. Delete `GpuCompute` struct and all its methods:
   - `new()`
   - `invalidate_scene_bind_group()`
   - `ensure_scene_bind_group()`
   - `layout()`
   - `pipeline_from()`
   - Any other methods or associated types

4. Delete `compute_galaxy()` function.

5. Delete host-side column-density replicas:
   - `disk_column_host()`
   - `bulge_column_host()`
   - `halo_column_host()`
   - `arm_modulation_host()`
   - Any constants only used by these (`TAU` — check if referenced elsewhere first; if only used by column code, remove)

6. Remove imports made dead by the deletions (e.g., `use std::borrow::Cow;` if only used by `GpuCompute`; `use wgpu::ShaderModuleDescriptor` if only used for SPIR-V loading).

7. Keep (do NOT delete):
   - `GpuStars` struct and all its methods
   - `StarInstance` struct
   - `MAX_STARS` constant
   - `generate_star_catalogue()` and all its internal helpers
   - `cull_stars_to_viewport()` (from plan 024)
   - All `#[cfg(test)]` functions for catalogue + LUT (step 4 handles test cleanup)

**Verify**: `cargo check` → errors only in app.rs/render.rs (they reference deleted types). Expected; continue.

### Step 3: Remove compute/display fields from App

Edit `src/app.rs`:

1. Remove these fields from `App`:
   ```rust
   pub gpu_compute: Option<gpu::GpuCompute>,
   pub uniform_buffer: Option<wgpu::Buffer>,
   pub rgba_buffer: Option<wgpu::Buffer>,
   pub rgba_buf_w: u32,
   pub rgba_buf_h: u32,
   pub texture: Option<wgpu::Texture>,
   pub bind_group: Option<wgpu::BindGroup>,
   pub bind_group_layout: Option<wgpu::BindGroupLayout>,
   pub sampler: Option<wgpu::Sampler>,
   pub display_params_buffer: Option<wgpu::Buffer>,
   pub tex_w: u32,
   pub tex_h: u32,
   ```

2. Remove `render_pipeline: Option<wgpu::RenderPipeline>` — this pipeline was the display pass (`display.wgsl`), now deleted. (The stars pipeline lives in `GpuStars.pipeline`.)

3. Remove the `recreate_texture()` method entirely — it only created the display-pass texture + bind group.

4. Remove from `App::init()`:
   - Display shader module creation (lines ~174–178)
   - Bind group layout + pipeline layout + render pipeline creation (lines ~180–241)
   - `self.render_pipeline` / `bind_group_layout` / `sampler` assignment (lines ~243–245)
   - `display_params_buffer` creation + write (lines ~248–258)
   - `self.gpu_compute = gpu::GpuCompute::new(...)` (line ~261)
   - `self.recreate_texture()` call (line ~281)

5. Delete the `recreate_texture()` method entirely (lines ~285–332) — it only created the display-pass texture + bind group.

6. Remove `use galaxy_gen::gpu::{GpuCompute, GalaxyUniform}` imports if present (keep `gpu::GpuStars`, `gpu::StarInstance`).

7. **`needs_render` dead_code fix**: The field is written but no longer read after removing the compute block.  Annotate it with `#[allow(dead_code)]` so `cargo clippy -- -D warnings` passes:

   ```rust
   #[allow(dead_code)]
   pub needs_render: bool,
   ```

   The field is still written by `src/ui.rs` (out of scope) and the frame-end write in `render.rs` — both harmless.

**Verify**: `cargo check` → errors only in render.rs (step 4 fixes them).

### Step 4: Simplify render.rs — stars-only dispatch

Edit `src/render.rs`:

1. **Guard clause** — replace with minimal version:

   ```rust
   if app.surface.is_none()
       || app.device.is_none()
       || app.queue.is_none()
       || app.config.is_none()
   {
       return;
   }
   ```

   Remove checks for `render_pipeline`, `bind_group_layout`, `sampler`, `gpu_compute`.

2. **Remove texture resize block** (~lines 19–24):

   ```rust
   if app.tex_w != app.render_w || app.tex_h != app.render_h {
       app.recreate_texture();
   }
   ```

3. **Remove rgba_buffer resize block** — the entire `if app.rgba_buf_w != app.render_w` block.

4. **Remove the `do_compute` block** — the entire `if app.needs_render && do_compute { ... }` block.

5. **Simplify destructuring** — `render_pipeline`, `bind_group`, `texture` no longer exist:

   ```rust
   let (surface, device, queue, config, pipeline, bind_group, texture) = (
   ```
   becomes:
   ```rust
   let (surface, device, queue, config) = (
   ```

   Remove `pipeline`, `bind_group`, and `texture` from the destructure.

6. **Remove the display pass from the render pass** (lines ~265–268):

   ```rust
   if !is_3d {
       rpass.set_pipeline(pipeline);
       rpass.set_bind_group(0, bind_group, &[]);
       rpass.draw(0..3, 0..1);
   }
   ```

   The `pipeline` and `bind_group` locals are already removed by item 5 above.
   The render pass should contain only:
   - `rpass.set_pipeline(&star_draw_data.0)` + `rpass.set_bind_group(0, &star_draw_data.1, ...)` + `rpass.draw(0..4, 0..instance_count)` — the stars pass
   - The egui pass at the end

7. **Remove `render_mode` local** and `show_glow` local if they still exist (should already be gone from plan 024).

8. **Remove `is_3d` local** if no longer needed for compute gating (the `do_compute` block used it, which is now removed). Keep `is_3d` only if the stars pass still references it — check: the stars pass writes ortho vs perspective based on `is_3d`, so keep it.

9. Remove the `display_params_buffer` field from wherever it appears (guard clause, init — already handled in step 3).

**Verify**: `cargo check` → exit 0 (first clean check).

### Step 5: Remove column-density tests

Edit `src/gpu.rs` `#[cfg(test)] mod tests`:

Delete all test functions that reference removed types or column-density functions:

1. `galaxy_uniform_field_offsets_match_shader_layout`
2. `uniform_is_pod`
3. `uniform_struct_size_matches_fields`
4. `from_params_preserves_values`
5. `from_params_preserves_values_ngc628`
6. All `disk_column_*` tests
7. All `bulge_column_*` tests
8. All `halo_column_*` tests
9. All `arm_modulation_*` tests
10. All `ngc628_column_profile_*` tests

Keep:
- All `generate_star_catalogue_*` tests
- All `cull_stars_to_viewport_*` tests
- All `mass_to_temp_*`, `temperature_to_rgb_*`, `COLOR_LUT_*` tests
- All galaxy preset tests in `src/galaxy.rs` (unchanged)

**Verify**: `cargo test` → all remaining tests pass.

### Step 6: Delete display.wgsl

Delete `src/display.wgsl`.

**Verify**: `ls src/display.wgsl` → "No such file or directory".

### Step 7: Rebuild SPIR-V and final verification

1. Rebuild SPIR-V from the new empty shader:

   ```
   cargo clean
   cargo build
   ```

   This recompiles `galaxy-shader/src/lib.rs` (empty entry point) to SPIR-V.
   The app compiles without `GalaxyUniform` or `compute_galaxy()` references.

2. Run `cargo clippy -- -D warnings` → exit 0. Fix any warnings (unused imports from removed code).

3. Run `cargo test` → all pass.

4. Confirm SPIR-V infrastructure intact:

   ```
   grep -rn "spirv-builder\|spirv-unknown" build.rs Cargo.toml
   ```
   → both files still reference spirv-builder.

   ```
   cat rust-toolchain.toml
   ```
   → still shows `nightly-2026-05-22`.

   ```
   ls galaxy-shader/
   ```
   → `Cargo.toml` and `src/lib.rs` exist.

**Verify**: `cargo clippy -- -D warnings` exits 0, `cargo test` all pass,
`cargo run` opens window with stars-only rendering (no glow).

## Test plan

- **Delete**: All column-density, GalaxyUniform, from_params tests
- **Preserve**: Star catalogue tests, LUT tests, cull tests, galaxy preset tests
- **No new tests needed** — this is pure deletion

**Verify**: `cargo test` → all pass. Count ≥ (old - deleted).

## Done criteria

Machine-checkable — ALL must hold:

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0
- [ ] `ls src/display.wgsl` → not found
- [ ] `grep -rn "GalaxyUniform\|GpuCompute\|compute_galaxy\|column_density" src/` returns zero matches
- [ ] `grep -rn "#\[allow(dead_code)\]" src/app.rs` returns a match above `pub needs_render` (dead_code fixed)
- [ ] `grep -rn "spirv-builder" build.rs Cargo.toml` returns matches in both files (scaffolding intact)
- [ ] `cat rust-toolchain.toml` shows `nightly-2026-05-22` (nightly stays)
- [ ] `ls galaxy-shader/Cargo.toml galaxy-shader/src/lib.rs` → both exist
- [ ] `git status` shows only files in the in-scope list are modified/deleted
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

1. **Scaffolding deletion risk**: If any step accidentally modifies or deletes
   `galaxy-shader/Cargo.toml`, `build.rs`, `rust-toolchain.toml`, or the
   `[workspace]`/`spirv-builder` sections of `Cargo.toml`, STOP — these must
   remain for future GPU star generation.

2. **GpuStars broken by deletions**: If deleting `GalaxyUniform`, `GpuCompute`,
   or `compute_galaxy` breaks `GpuStars` compilation, STOP — they may share
   infrastructure. Report the dependency.

3. **SPIR-V build failure after shader replacement**: If `cargo build` fails
   during SPIR-V compilation after step 1's shader replacement, STOP — the
   empty shader may have a syntax error. Verify: `#![no_std]`, correct
   `spirv_std` imports, matching entry-point name.

4. **Test failures in kept tests**: If star catalogue, LUT, or preset tests
   fail after deletions, STOP — a kept function may have a hidden dependency
   on deleted code.

5. **Step verification fails twice** after a reasonable fix attempt.

6. **`needs_render` dead_code persists**: If `cargo clippy -- -D warnings` still complains about `needs_render` after adding `#[allow(dead_code)]`, check that the annotation is on the field declaration in `App`, not on a `let` binding or use-site.

## Maintenance notes

- The `galaxy-shader/src/lib.rs` entry point is intentionally empty.  When
  on-GPU star generation is implemented:
  1. Replace the empty `main_scene` body with instance-buffer-filling logic
  2. Add a uniform struct for camera/frustum params
  3. Wire `GpuCompute` back into `App` and call `compute_galaxy()` from
     `render.rs`
  4. Update `Done criteria` step 4 to expect `GalaxyUniform`/`compute_galaxy`
     back in `src/`
- The SPIR-V pipeline (`build.rs` → `spirv-builder` → `galaxy-shader`) is
  fully functional but produces a no-op shader.  `cargo build` and `cargo check`
  compile it every time — no regressions, just zero work.
- `wgpu` keeps the `spirv` feature — required for loading `.spv` binaries
  when the compute shader is re-enabled.
- Column-density functions (`disk_column`, etc.) are fully deleted from both
  host and shader — re-add them only if a future glow overlay is desired.
