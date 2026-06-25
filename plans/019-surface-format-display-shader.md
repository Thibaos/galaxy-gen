# Plan 019: Handle surface format in display shader

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat 1647567..HEAD -- src/display.wgsl src/main.rs src/gpu.rs`
> If `display.wgsl`, the surface format selection in `App::new`, or
> `DisplayPipeline` struct changed since this plan was written, treat it as
> a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Category**: correctness
- **Depends on**: none
- **Planned at**: commit `1647567`, 2026-06-25

## Why this matters

`display.wgsl` hardcodes an R↔B color channel swap (`return vec4(color.b,
color.g, color.r, color.a)`) with the comment "Compensate for Bgra8Unorm
surface format". This assumes the surface format is always `Bgra8Unorm`.

On backends where `surface_caps.formats[0]` returns `Rgba8Unorm` (e.g. DX12
on some drivers, WebGPU on some browsers), this swap produces inverted red
and blue channels — galaxies appear blue-tinted instead of warm.
The current code takes `surface_caps.formats[0]` blindly, so the format
depends on the platform.

The fix: pass a `surface_is_bgra: bool` uniform to the display shader and
conditionally swap channels.

**Note**: This is low-confidence — on most Windows setups (Vulkan backend),
the format is Bgra8Unorm and the current code is correct. On DX12 or some
Linux configs, it could be wrong. The fix is defensive.

## Current state

**Display shader** — `src/display.wgsl:33-35`:

```wgsl
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(tex, smp, in.uv);
    // Compensate for Bgra8Unorm surface format — swap R↔B.
    return vec4(color.b, color.g, color.r, color.a);
}
```

**Surface format selection** — `src/main.rs:105-113` (in `App::new`):

```rust
let surface_caps = surface.get_capabilities(adapter);
let surface_format = surface_caps
    .formats
    .first()
    .copied()
    .expect("surface has no supported formats");
```

**DisplayPipeline** — defined in `src/main.rs` as a struct with fields:
`bind_group_layout`, `bind_group`, `pipeline`, `texture`, `sampler`.
The `params_buffer` will be a new field on this struct alongside the
existing fields.

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**
- `src/display.wgsl` — add a uniform buffer binding with a `surface_is_bgra`
  flag; conditionally swap or not
- `src/main.rs` — determine whether the selected format is BGRA; add
  `params_buffer` field to `DisplayPipeline` struct + bind group wiring

**Out of scope:**
- `src/gpu.rs` — `DisplayPipeline` is defined in `main.rs`, not `gpu.rs`

## Design decision

Two approaches:

**A: Uniform buffer** — Add a binding for a tiny uniform (4 bytes) with the
flag. Requires a new bind group entry and buffer. More code but clean.

**B: Push constant** — Use a push constant range on the display pipeline.
Simpler, one `set_push_constants` call per draw. But push constants have
size limits and aren't universally available on all backends.

**C: Compile-time shader variant (rejected)** — Two shader files, pick one at
runtime. Duplicates the shader for a 1-line difference.

→ **Use option A** (uniform buffer). The display pipeline already has a bind
group layout with texture + sampler (bindings 0 and 1). Add binding 2 with
a uniform buffer for `surface_is_bgra`.

## Steps

### Step 1: Add uniform to display.wgsl

Edit `src/display.wgsl`. Add at the top (after `@group(0) @binding(1) var smp: sampler;`):

```wgsl
struct DisplayParams {
    surface_is_bgra: u32,
}

@group(0) @binding(2) var<uniform> params: DisplayParams;
```

Replace the fragment shader return statement to use the flag:

```wgsl
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(tex, smp, in.uv);
    // Compensate for Bgra8Unorm surface format — swap R↔B only when needed.
    if params.surface_is_bgra == 1u {
        return vec4(color.b, color.g, color.r, color.a);
    }
    return color;
}
```

**Verify**: `cargo check` → fail (shader compiles but Rust bindings are missing — expected, fixed next step)

### Step 2: Add params_buffer to DisplayPipeline

In `src/main.rs`, add a field to the `DisplayPipeline` struct:

```rust
pub params_buffer: wgpu::Buffer,
```

In the part of `App::new()` where `DisplayPipeline` is created, add the
buffer creation and flag write right after the existing sampler creation:

```rust
let display_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("display_params"),
    size: 4,
    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    mapped_at_creation: false,
});
let surface_is_bgra = surface_format == wgpu::TextureFormat::Bgra8Unorm;
queue.write_buffer(&display_params_buffer, 0, &(surface_is_bgra as u32).to_ne_bytes());
```

Add `params_buffer: display_params_buffer` to the `DisplayPipeline` struct
literal returned at the end of init.

**Verify**: `cargo check` → fail (bind group layout entry missing — expected)

### Step 3: Update bind group layout

In `src/main.rs`, find the `bind_group_layout` creation for `DisplayPipeline`
(look for `display_bind_group_layout` or the `create_bind_group_layout` call
with entries for texture + sampler). Add a third entry at `binding: 2`
WITHIN the existing `BindGroupLayoutEntry` array:

```rust
wgpu::BindGroupLayoutEntry {
    binding: 2,
    visibility: wgpu::ShaderStages::FRAGMENT,
    ty: wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    },
    count: None,
},
```

In the `create_bind_group` call for DisplayPipeline, add a third entry:

```rust
wgpu::BindGroupEntry {
    binding: 2,
    resource: display_params_buffer.as_entire_binding(),
},
```

**Verify**: `cargo check` → exit 0 (all bindings match)

### Step 4: Full validation

```bash
cargo clippy -- -D warnings
cargo test
```

All 45 tests pass.

## Git workflow

- Branch: `advisor/019-surface-format-display-shader`
- Commit message: `fix: handle surface format in display shader via uniform`
- Do NOT push or open a PR unless instructed.

## Test plan

- All 45 existing tests pass
- Manual test: launch app on current platform, verify colors look the same
  as before (no regression). If possible, test on a platform with
  Rgba8Unorm surface format — verify red/blue are not swapped.

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0 (all 45 pass)
- [ ] Display shader conditionally swaps R↔B based on uniform flag
- [ ] Flag is set once at init based on actual surface format
- [ ] Visual output unchanged on Bgra8Unorm platforms
- [ ] `plans/README.md` updated

## STOP conditions

- The code at the locations in "Current state" doesn't match the excerpts
  (the codebase has drifted since this plan was written).
- Display pipeline creation fails (bind group layout mismatch)
- Visual regression on current platform
- Any existing test fails

## Maintenance notes

- `surface_is_bgra` is written once at init. It never changes for the
  lifetime of the surface. If surface format changes (e.g. window resize
  triggers reconfigure with a different format), the buffer must be
  re-uploaded alongside surface reconfiguration.
- The WGSL uses an `if` branch, not `mix()`. This is intentional — the
  branch is uniform (same value for all fragments in a draw call), so no
  shader divergence occurs.
