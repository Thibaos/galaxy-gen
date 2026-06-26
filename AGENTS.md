# AGENTS.md — Agent guidance for galaxy-gen

## Project

A Rust desktop app that renders a galaxy as a 3D point cloud of deterministic stars
+ a 2D column-density glow for shape overview.  Two rendering modes:
- **2D**: compute shader column-density glow + instanced star pass (ortho top-down)
- **3D**: instanced star pass only (orbit camera)

Single binary, no server, no network, no database.

The end goal is a massive deterministic star map — every spatial position has a
well-defined star-or-not decision — consumed by a separate game layer ("find the
correct star").

## Commands

| Purpose   | Command                    |
|-----------|----------------------------|
| Check     | `cargo check`              |
| Lint      | `cargo clippy -- -D warnings` |
| Test      | `cargo test`               |
| Build     | `cargo build`              |
| Run       | `cargo run`      |

## Conventions

- Rust edition 2024, nightly toolchain (see `rust-toolchain.toml`)
- GPU resources stored as `Option<T>` on `App` struct, created in `App::init()`
- Per-frame resources recreated only when dimensions change (see `recreate_texture` pattern)
- `unwrap()` is acceptable for GPU resource access after the init guard clause
- **SPIR-V compute shader**: `galaxy-shader/src/lib.rs` (spirv-std, `#[spirv(compute(threads(8,8,1)))]`)
- **WGSL shaders**: `src/stars.wgsl` (instanced star sprites) and `src/display.wgsl` (fullscreen quad)
- Host–shader struct correspondence:
  - `src/gpu.rs:GalaxyUniform` ↔ `galaxy-shader/src/lib.rs:GalaxyUniform`
  - `src/gpu.rs:StarInstance` ↔ `src/stars.wgsl` star stride/layout (7 u32 per star)
- Tests live in `#[cfg(test)] mod tests` at the bottom of each source file
- Avoid using the `cargo clean` command, unless absolutely necessary
- Presets are the **only** way to change galaxy parameters (no live slider tweaking).
  Sidebar contains view controls only — not physics sliders.

## Galaxy presets

- Presets live in `src/galaxy.rs` as `GalaxyParams::*()` constructors
- Reference data for each galaxy target is stored in `galaxies/*.toml` — read this before working on presets or tuning parameters; no web searching needed
- Available presets: `milky_way()`, `ngc628()` (M74), `m31()` (Andromeda), `m51()` (Whirlpool), `m101()` (Pinwheel)
- Switching presets: use the egui sidebar dropdown. This regenerates the star catalogue and resets the camera.

### Adding a new galaxy preset

1. Choose a real galaxy target and research its physical parameters (photometric decomposition, stellar mass, distance, inclination)
2. Save all measurements, sources, and derived values to `galaxies/<target>.toml`
3. Add a `GalaxyParams::<target>()` constructor with documented comment blocks for each field
4. Add host-side column profile tests in both `src/galaxy.rs` and `src/gpu.rs` that validate:
   - All fields are finite and positive (where appropriate)
   - Disk column follows exponential drop: Σ(hr)/Σ(0) ≈ 1/e
   - Bulge column follows Plummer profile: Σ(a)/Σ(0) ≈ ¼
   - Bulge central density is physically consistent with bulge flux fraction
   - Disk scale length matches the literature measurement
5. For face-on galaxies, disk scale height must be estimated from hz/hr relations (~0.1–0.15 for late-type spirals); note the uncertainty
6. Run `cargo clippy -- -D warnings` and `cargo test` before declaring done

## Rendering modes

- **2D**: compute shader (column-density glow) → display pass → instanced stars (ortho top-down camera)
- **3D**: instanced stars only (orbit camera, black background)
- Mode toggle in sidebar. Switching preserves view position (2D↔3D maps camera ↔ center/extent).
- In 2D, star instances are AABB-culled to the ortho viewport before upload to reduce vertex work.

## Star catalogue

- Generated on CPU by `src/gpu.rs:generate_star_catalogue()` — a stepping stone toward GPU compute-shader generation
- Covers the galaxy disc out to 8× scale_length (clamped 50–80 kLY)
- Max 524,288 stars (`MAX_STARS`), selected via A-Res weighted reservoir sampling
- Regenerated on preset switch only
- Uploaded to a storage buffer read by `stars.wgsl` in both 2D and 3D modes
- Star colour: temperature from mass via piecewise power-law fit, RGB from vendian.org spectrum LUT

## Sidebar controls (plan 024)

After the parameter-slider strip-down, the sidebar contains only view controls:
- Preset dropdown
- 2D star brightness slider (0–2, default 1.0)
- 2D/3D toggle
- Star size slider (0.01–0.5, default 0.05)
- 3D camera distance & FOV sliders
- Screenshot button, Reset Camera button

## Gotchas

- The `build.rs` compiles `galaxy-shader` to SPIR-V and embeds it via `env!("galaxy_shader.spv")`
- Changing `galaxy-shader/src/lib.rs` requires a clean rebuild (`cargo clean` or touch `build.rs`)
- The SPIR-V compute shader uses 8×8 thread groups; image dimensions must be divisible by 8 or the shader handles out-of-bounds via early-return
- The `image` crate is used for PNG export via `save_snapshot()` in `src/app.rs`
- 2D controls: mouse drag pans (gated behind egui focus), mouse wheel zooms (centered on cursor)
- 3D controls: mouse drag orbits, mouse wheel zooms camera distance
- Exposure/contrast keyboard shortcuts (arrow keys) have been removed
