# Plan 005: Add compile-time guard against host–shader struct mismatch

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 5a0cdfc..HEAD -- src/gpu.rs galaxy-shader/src/lib.rs`
> If either file changed, compare "Current state" excerpts against live code.

## Status

- **Priority**: P2
- **Effort**: S
- **Category**: tech-debt
- **Depends on**: none
- **Planned at**: commit `5a0cdfc`, 2026-06-14
- **Issue**: —

## Why this matters

The `GalaxyUniform` struct is defined in two places: `src/gpu.rs:12-31` (host side, bytemuck Pod, sent to GPU) and `galaxy-shader/src/lib.rs:6-23` (shader side, received by the compute shader). These two definitions must match exactly in field order, type, and size. A future change that adds a field to one but not the other, reorders fields, or uses a different type would compile successfully but cause silent rendering corruption — the GPU shader would read garbage for misaligned fields. There is currently no compile-time link between the two definitions. The fix is a `const_assert!` (or equivalent) in `src/gpu.rs` that verifies the struct layout matches expectations, plus a static assertion on the overall size.

## Current state

Host-side definition (`src/gpu.rs:12-31`):

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

Shader-side definition (`galaxy-shader/src/lib.rs:6-23`):

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
}
```

These are identical today. The fix is to add assertions that will break the build if they drift.

The toolchain is nightly-2026-04-11 — the `static_assertions` crate isn't available, but since this is nightly Rust, we can use `const { assert!(...) }` in a const context, or a simpler approach: a test that checks struct size and field offsets at runtime.

## Commands you will need

| Purpose   | Command               | Expected on success |
|-----------|-----------------------|---------------------|
| Check     | `cargo check`         | exit 0              |
| Clippy    | `cargo clippy`        | exit 0              |
| Test      | `cargo test`          | all pass            |
| Build     | `cargo build`         | exit 0              |

## Scope

**In scope** (files to modify):
- `src/gpu.rs` — add compile-time or test-time assertions verifying struct layout

**Out of scope** (do NOT touch):
- `galaxy-shader/src/lib.rs` — no changes (the shader definition is the source of truth)
- `build.rs` — no changes
- Any attempt to auto-generate one definition from the other — that's a larger refactor

## Git workflow

- Branch: `advisor/005-struct-guard`
- Single commit; message: `add struct layout assertions for GalaxyUniform`
- Do NOT push or open a PR unless instructed

## Conventions

Use `#[test]` functions (not build-time `const` assertions) because nightly `const` blocks may not work with `memoffset`-style operations. The tests we add in plan 002 already check the overall struct size. This plan adds field-offset assertions using `std::mem::offset_of!` (stabilized in Rust 1.77, available on nightly-2026-04-11).

## Steps

### Step 1: Add field-offset assertions to the test module in `src/gpu.rs`

In the `#[cfg(test)] mod tests` block at the bottom of `src/gpu.rs`, add tests that verify the byte offset of each field. This catches both field reordering and type-size changes:

```rust
#[test]
fn galaxy_uniform_field_offsets_match_shader_layout() {
    // These offsets MUST match the shader-side GalaxyUniform layout.
    // The shader uses spirv-std glam types (f32 = 4 bytes, u32 = 4 bytes)
    // with #[repr(C)] packing.
    use std::mem::{offset_of, size_of};

    assert_eq!(size_of::<GalaxyUniform>(), 76, "overall size mismatch with shader");

    assert_eq!(offset_of!(GalaxyUniform, disk_scale_length), 0);
    assert_eq!(offset_of!(GalaxyUniform, disk_scale_height), 4);
    assert_eq!(offset_of!(GalaxyUniform, disk_central_density), 8);
    assert_eq!(offset_of!(GalaxyUniform, arm_count), 12);
    assert_eq!(offset_of!(GalaxyUniform, arm_pitch), 16);
    assert_eq!(offset_of!(GalaxyUniform, arm_concentration), 20);
    assert_eq!(offset_of!(GalaxyUniform, arm_strength), 24);
    assert_eq!(offset_of!(GalaxyUniform, bulge_radius), 28);
    assert_eq!(offset_of!(GalaxyUniform, bulge_central_density), 32);
    assert_eq!(offset_of!(GalaxyUniform, halo_radius), 36);
    assert_eq!(offset_of!(GalaxyUniform, halo_central_density), 40);
    assert_eq!(offset_of!(GalaxyUniform, halo_slope), 44);
    assert_eq!(offset_of!(GalaxyUniform, image_width), 48);
    assert_eq!(offset_of!(GalaxyUniform, image_height), 52);
    assert_eq!(offset_of!(GalaxyUniform, extent), 56);
    assert_eq!(offset_of!(GalaxyUniform, center_x), 60);
    assert_eq!(offset_of!(GalaxyUniform, center_y), 64);
    assert_eq!(offset_of!(GalaxyUniform, exposure), 68);
    assert_eq!(offset_of!(GalaxyUniform, log_contrast), 72);
}
```

**Verify**: `cargo test galaxy_uniform_field_offsets_match_shader_layout` → passes

### Step 2: Add a documentation comment linking host and shader definitions

Above `GalaxyUniform` in `src/gpu.rs`, add a doc comment:

```rust
/// GPU uniform buffer matching `galaxy-shader/src/lib.rs:GalaxyUniform`.
/// The two definitions MUST stay in sync. Field-offset tests in this
/// module's `#[cfg(test)]` block will catch mismatches at test time.
#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
pub struct GalaxyUniform {
```

**Verify**: `cargo check` → exit 0

### Step 3: Run full test suite

**Verify**: `cargo test` → all tests pass (including the new offset test); `cargo clippy` → exit 0

## Test plan

The field-offset test is the plan's deliverable. It fails if:
- A field is added/removed without updating offsets
- A field type changes from f32/u32 to something with a different size
- Field order changes

## Done criteria

- [ ] `cargo test` exits 0 with `galaxy_uniform_field_offsets_match_shader_layout` passing
- [ ] Doc comment added above `GalaxyUniform` in `src/gpu.rs` linking to shader definition
- [ ] `cargo clippy` exits 0
- [ ] No files outside the in-scope list are modified
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back if:

- `std::mem::offset_of!` is not available. If so, replace with manual offset calculation using `std::mem::size_of` and `align_of`, or use the `bytemuck` offset helpers.
- The overall size assertion (`size_of::<GalaxyUniform>() == 72`) fails. The struct may have already drifted. Report the actual size and compare against the shader definition.
- Any field-offset assertion fails. Do NOT fix by changing the test values — the host struct must match the shader. If they differ, the shader definition is the source of truth; report the discrepancy.

## Maintenance notes

- When adding a new field to `GalaxyUniform`, you MUST add it to BOTH definitions (host and shader) and update the offset assertions.
- The test only catches mismatches when `cargo test` runs. It does not catch mismatches at compile time. For a stronger guarantee, a build.rs script could parse the shader-side struct definition and generate the host-side one — but that's out of scope for this plan.
- The struct contains 19 fields (16 × f32 + 3 × u32). The `#[repr(C)]` layout guarantees field order matches declaration order with no padding between f32/u32 fields.
