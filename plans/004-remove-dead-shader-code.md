# Plan 004: Remove dead shader entry points

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 5a0cdfc..HEAD -- galaxy-shader/src/lib.rs src/gpu.rs`
> If either file changed, compare "Current state" excerpts against live code.

## Status

- **Priority**: P2
- **Effort**: S
- **Category**: tech-debt
- **Depends on**: none (independent of other plans)
- **Planned at**: commit `5a0cdfc`, 2026-06-14
- **Issue**: —

## Why this matters

The galaxy shader (`galaxy-shader/src/lib.rs`) contains three old entry points — `render_density`, `normalize_rgba`, and `render_stars` — that were part of a three-pass render pipeline. They were superseded by the unified `render_scene` entry point (which handles density, stars, and tone mapping in a single compute dispatch). These functions and their supporting `NormUniform` struct are dead code: never called from `gpu.rs`, never referenced elsewhere. Dead code in a shader isn't just clutter — it increases SPIR-V binary size, shader compile time, and risks someone accidentally calling the wrong entry point. The removal is a pure deletion with zero behavioral change.

## Current state

The shader `galaxy-shader/src/lib.rs` has these unused items:

- **Lines 165–178**: `render_density` entry point — old first pass (writes f32 density per pixel)
- **Lines 189–194**: `NormUniform` struct — used only by the dead `normalize_rgba`
- **Lines 195–222**: `normalize_rgba` entry point — old second pass (density→RGBA with tone mapping)
- **Lines 330–399**: `render_stars` entry point — old third pass (adds star light to RGBA buffer)

The host-side `gpu.rs` only calls `render_scene` (line 167: `entry_point: Some("render_scene")`). No reference to `render_density`, `normalize_rgba`, or `render_stars` exists in the host code.

```rust
// galaxy-shader/src/lib.rs:163-178 — dead
#[spirv(compute(threads(8, 8, 1)))]
pub fn render_density(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] params: &GalaxyUniform,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] output: &mut [f32],
) { ... }
```

```rust
// galaxy-shader/src/lib.rs:189-194 — dead
pub struct NormUniform { ... }
```

```rust
// galaxy-shader/src/lib.rs:195-222 — dead
#[spirv(compute(threads(8, 8, 1)))]
pub fn normalize_rgba( ... ) { ... }
```

```rust
// galaxy-shader/src/lib.rs:328-399 — dead
#[spirv(compute(threads(8, 8, 1)))]
pub fn render_stars( ... ) { ... }
```

The `gpu.rs` side only references `render_scene`:

```rust
// src/gpu.rs:167
entry_point: Some("render_scene"),
```

## Commands you will need

| Purpose   | Command               | Expected on success |
|-----------|-----------------------|---------------------|
| Check     | `cargo check`         | exit 0              |
| Clippy    | `cargo clippy`        | exit 0              |
| Build     | `cargo build`         | exit 0              |
| Test      | `cargo test`          | all pass            |

## Scope

**In scope** (files to modify):
- `galaxy-shader/src/lib.rs` — delete `render_density`, `normalize_rgba`, `render_stars` functions and `NormUniform` struct

**Out of scope** (do NOT touch):
- `src/gpu.rs` — already only uses `render_scene`
- `src/main.rs` — no changes
- `build.rs` — no changes
- Any Rust source file other than the shader

## Git workflow

- Branch: `advisor/004-remove-dead-shader-code`
- Single commit; message: `remove dead shader entry points (render_density, normalize_rgba, render_stars)`
- Do NOT push or open a PR unless instructed

## Conventions

The shader code uses `spirv-std` with `#[spirv(compute(threads(8, 8, 1)))]` entry points. The unified `render_scene` entry point (line ~410) is the only one used. The helper functions called by `render_scene` (`column_density`, `poisson`, `sample_imf`, `mass_to_lum`, `mass_to_temp`, `temperature_to_rgb`, `hash3`, `randf`, `rem_euclid`) must NOT be deleted — they are used by `render_scene`.

Specifically, these helper functions are SHARED between the old entry points and `render_scene`:
- `hash3`, `randf` — used by `render_stars` AND `render_scene` → keep
- `poisson`, `sample_imf`, `mass_to_lum`, `mass_to_temp`, `temperature_to_rgb` — used by `render_stars` AND `render_scene` → keep
- `column_density`, `disk_column`, `arm_modulation_2d`, `bulge_column`, `halo_column` — used by `render_stars` AND `render_scene` → keep

Delete ONLY the four entry-point functions and the `NormUniform` struct. Do NOT delete any helper functions.

## Steps

### Step 1: Delete dead code from galaxy-shader

Open `galaxy-shader/src/lib.rs`. Delete these blocks:

1. **`render_density`** (lines 163–178): The `#[spirv(compute(...))]` function and its body. The comment line `// ═══════════════════════════════════════════════════════════` above it and the `//  Stars: per-pixel procedural star rendering` below it should stay (they form the section boundary). Only delete the function.

2. **`NormUniform` struct** (lines 189–194): The struct definition and its `#[derive(Copy, Clone)]` + `#[repr(C)]` attributes.

3. **`normalize_rgba`** (lines 195–222): The `#[spirv(compute(...))]` function and its body.

4. **`render_stars`** (lines 328–399): The `#[spirv(compute(...))]` function and its body, plus the preceding comment block.

After deletion, verify the remaining code has:
- `render_scene` as the only `#[spirv(compute(...))]` entry point
- All helper functions intact
- The section-header comments preserved

**Verify**: `cargo check` → exit 0 (this compiles the shader via build.rs)

### Step 2: Confirm no references remain

**Verify**: `grep -n "render_density\|normalize_rgba\|render_stars\|NormUniform" galaxy-shader/src/lib.rs src/gpu.rs src/main.rs` → should return matches only in comment text (if any), not in code. If `render_scene` appears in results that's fine.

### Step 3: Full build with tests

**Verify**: `cargo build` → exit 0; `cargo test` → all pass; `cargo clippy` → exit 0

## Test plan

No new tests needed — this is pure deletion. Existing tests from plan 002 (if applied) must continue to pass. A manual smoke test: run the binary and confirm the galaxy still renders identically (same entry point, same helpers, same behavior).

## Done criteria

- [ ] `render_density`, `normalize_rgba`, `render_stars` functions removed from `galaxy-shader/src/lib.rs`
- [ ] `NormUniform` struct removed from `galaxy-shader/src/lib.rs`
- [ ] `render_scene` and all helper functions still present
- [ ] `cargo check` exits 0
- [ ] `cargo clippy` exits 0
- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0 (all existing tests pass)
- [ ] `grep "render_density\|normalize_rgba\|render_stars\|NormUniform" galaxy-shader/src/lib.rs` returns no matches
- [ ] No files outside the in-scope list are modified
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back if:

- The code at the locations in "Current state" doesn't match the excerpts.
- Any helper function (`hash3`, `randf`, `poisson`, `sample_imf`, `mass_to_lum`, `mass_to_temp`, `temperature_to_rgb`, `column_density`, `disk_column`, `arm_modulation_2d`, `bulge_column`, `halo_column`, `arm_modulation`, `density`, `disk_density`, `bulge_density`, `halo_density`, `rem_euclid`) is accidentally deleted — restore it immediately.
- `cargo check` fails after deletion. Re-check that only the four items were deleted.
- The shader compiles but the rendered galaxy looks different (indicating `render_scene` was somehow modified).

## Maintenance notes

- If the three-pass pipeline is ever needed again (e.g. for a different rendering mode), the git history preserves the old code at commit `5a0cdfc`. No need to keep it in-tree.
- The unified `render_scene` function already handles the blend between individual stars and analytic mode via `LAMBDA_INDIVIDUAL`/`LAMBDA_ANALYTIC` thresholds. No functionality was lost.
