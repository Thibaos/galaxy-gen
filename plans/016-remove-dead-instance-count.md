# Plan 016: Remove dead `instance_count` field from `GpuStars`

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat HEAD~1..HEAD -- src/gpu.rs src/main.rs`
> If `GpuStars` struct or `instance_count` usage changed since this plan was
> written, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
- **Category**: tech debt
- **Depends on**: none
- **Planned at**: commit `1647567`, 2026-06-25

## Why this matters

`GpuStars::instance_count` is initialized to 0 in `GpuStars::new()` and
never written to or read from anywhere in the codebase. The actual instance
count used for the instanced draw call comes from `star_catalogue.len()`
at render time.

Dead fields mislead future readers, add noise to struct inspection, and
can cause bugs if someone accidentally wires them up expecting real data.

## Current state

**Declaration** — `src/gpu.rs:405`:

```rust
pub struct GpuStars {
    pub instance_buffer: wgpu::Buffer,
    pub instance_count: u32,    // <-- dead
    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::RenderPipeline,
    pub camera_buffer: wgpu::Buffer,
    pub brightness_buffer: wgpu::Buffer,
}
```

**Initialization** — `src/gpu.rs:573` (in `GpuStars::new`):

```rust
Self {
    instance_buffer,
    instance_count: 0,
    // ...
}
```

**Usage**: `rg "instance_count" src/` returns only the two lines above.
No reads. No writes. The draw call in `main.rs` uses
`star_catalogue.len()` directly.

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**
- `src/gpu.rs` — remove field from struct and initialization

## Steps

### Step 1: Remove the field

In `src/gpu.rs`, delete the `pub instance_count: u32,` line from the
`GpuStars` struct.

### Step 2: Remove initialization

In `GpuStars::new()`, delete the `instance_count: 0,` line from the
`Self { ... }` literal.

### Step 3: Validate

```bash
cargo check
cargo clippy -- -D warnings
cargo test
```

## Test plan

- All 45 existing tests pass (no behavioral change — field was never used)
- `rg "instance_count"` returns zero results

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0 (all 45 pass)
- [ ] `rg "instance_count"` returns zero results in src/
- [ ] `plans/README.md` updated

## STOP conditions

- Any compilation error
- Any existing test fails
