# Plan 001: Cache GPU pipeline and shader resources across frames

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 5a0cdfc..HEAD -- src/gpu.rs src/main.rs`
> If either file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Category**: perf
- **Depends on**: none
- **Planned at**: commit `5a0cdfc`, 2026-06-14
- **Issue**: —

## Why this matters

Every time the user pans, zooms, or adjusts exposure/contrast, `compute_galaxy()` in `gpu.rs` re-creates the SPIR-V shader module, bind group layout, pipeline layout, and compute pipeline from scratch. GPU pipeline compilation is a driver-level operation that can take tens to hundreds of milliseconds — on lower-end GPUs this causes visible stutter on every interaction. These resources are identical across frames (only the uniform buffer and rgba buffer contents change), so they should be created once and cached. Moving resource creation to init eliminates this wasted GPU work entirely.

## Current state

The function `compute_galaxy()` in `src/gpu.rs` allocates these resources on every call (lines 87–191):

```rust
// src/gpu.rs:87-98 — Shader module recreated every call
let spirv_bytes = include_bytes!(env!("galaxy_shader.spv"));
assert!(
    spirv_bytes.len().is_multiple_of(4),
    "SPIR-V binary not word-aligned"
);
let spirv_words: Vec<u32> = spirv_bytes
    .chunks_exact(4)
    .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
    .collect();
let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
    label: Some("galaxy-shader"),
    source: wgpu::ShaderSource::SpirV(std::borrow::Cow::Owned(spirv_words)),
});
```

```rust
// src/gpu.rs:119-163 — Bind group layout, pipeline layout, compute pipeline recreated every call
let scene_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { ... });
let scene_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { ... });
let scene_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { ... });
```

The `App` struct in `src/main.rs` already caches the display pipeline's resources the correct way — `render_pipeline`, `bind_group_layout`, and `sampler` are created once in `init()` (lines 127–190) and reused in `redraw()`. The compute resources need the same treatment.

The `compute_galaxy` function signature (`src/gpu.rs:72-84`) takes `device` and `queue` plus many parameters. It has 11 arguments (clippy warns about this). A refactored version will take pre-built resources instead.

## Commands you will need

| Purpose   | Command               | Expected on success |
|-----------|-----------------------|---------------------|
| Check     | `cargo check`         | exit 0              |
| Clippy    | `cargo clippy`        | exit 0, 0 warnings  |
| Build     | `cargo build`         | exit 0              |

(No test command exists yet — see plan 002.)

## Scope

**In scope** (files you may modify):
- `src/gpu.rs` — split into init and per-frame functions; cache shader module, pipeline, pipeline layout, bind group layout
- `src/main.rs` — call the new init function during `App::init()`, call the streamlined per-frame function in `redraw()`

**Out of scope** (do NOT touch):
- `galaxy-shader/src/lib.rs` — the shader source is correct; this plan only changes host-side resource management
- Any change to the rendering behavior or output — images must be pixel-identical before and after
- The `display.wgsl` shader or the display pipeline — unrelated

## Git workflow

- Branch: `advisor/001-cache-gpu-resources`
- Commit per step; message style matches repo: imperative, lowercase, e.g. `cache shader module across frames`
- Do NOT push or open a PR unless instructed

## Conventions

The repo uses the `App` struct pattern for state management (see `src/main.rs:37-65`). GPU resources are stored as `Option<wgpu::Thing>` and unwrapped in `redraw()` after a guard clause checks they're all `Some`. Match this pattern: store the new cached resources on `App` as `Option<...>` fields.

The `App::init()` method (`src/main.rs:99-208`) is where one-time GPU resources are created (display pipeline, sampler, bind group layout). The `App::recreate_texture()` (`src/main.rs:229-265`) is where per-resize resources are recreated. The compute pipeline resources are one-time (they don't depend on window size), so they go in `init()`.

## Steps

### Step 1: Create a `GpuCompute` struct in `gpu.rs` to hold cached resources

Add a new struct that holds the resources currently re-created every frame. Place it above `GalaxyUniform`:

```rust
pub struct GpuCompute {
    pub module: wgpu::ShaderModule,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::ComputePipeline,
}
```

Expose a constructor that takes `&wgpu::Device` and returns `Self`. Move the shader module creation (lines 87-98), bind group layout creation (lines 119-133), pipeline layout creation (lines 155-159), and compute pipeline creation (lines 161-169) into this constructor. Only keep the code from lines 87-169 that creates these four resources — discard the uniform buffer, rgba buffer, and bind group creation (those remain per-frame).

**Verify**: `cargo check` → exit 0

### Step 2: Add `GpuCompute` storage to `App` and initialize it

In `src/main.rs`, add a field to the `App` struct:

```rust
gpu_compute: Option<gpu::GpuCompute>,
```

Initialize it to `None` in `App::new()`. In `App::init()`, after the display pipeline resources are created (around line 190), add:

```rust
self.gpu_compute = Some(gpu::GpuCompute::new(self.device.as_ref().unwrap()));
```

**Verify**: `cargo check` → exit 0

### Step 3: Slim down `compute_galaxy` to take `&GpuCompute` instead of recreating resources

Change `compute_galaxy` to accept the pre-built resources. Replace the `device` and `queue` parameters with a `compute: &GpuCompute` parameter. Remove lines 87-169 (the shader module, bind group layout, pipeline layout, and compute pipeline creation). Keep lines 101-115 (uniform buffer), lines 171-191 (rgba buffer, bind group, dispatch, copy), but update them to use `compute.bind_group_layout` and `compute.pipeline`.

The new signature should be:

```rust
pub fn compute_galaxy(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    compute: &GpuCompute,
    params: &GalaxyParams,
    image_w: u32,
    image_h: u32,
    galaxy_extent_ly: f64,
    center_x_ly: f64,
    center_y_ly: f64,
    exposure: f32,
    contrast: f32,
    target_texture: &wgpu::Texture,
)
```

(This is still 11 args — the clippy warning will persist but is pre-existing. Fixing it is outside scope.)

Update the call site in `src/main.rs:307` (inside `redraw()`) to pass `self.gpu_compute.as_ref().unwrap()` as the new argument.

**Verify**: `cargo check` → exit 0; `cargo clippy` → exit 0 (pre-existing too_many_arguments warnings are acceptable; no new warnings)

### Step 4: Also cache the rgba buffer

The rgba buffer (`src/gpu.rs:173-178`) is also re-created every frame. Since it's sized to `image_w × image_h` and those dimensions can change on resize, it needs to be recreated only when dimensions change — not every render. Move its creation into a separate function `recreate_rgba_buffer` that stores it on a new `App` field:

```rust
rgba_buffer: Option<wgpu::Buffer>,
```

In `compute_galaxy`, accept the buffer as a parameter instead of creating it. In `redraw()`, recreate the buffer if dimensions changed (similar to how `recreate_texture()` works).

**Verify**: `cargo build` → exit 0

### Step 5: Manual smoke test

Run `cargo build` and launch the binary. Pan, zoom, and adjust exposure/contrast rapidly — the app should render smoothly with no visual difference from before. The console output from `compute_galaxy`'s `println!` will still show timing per frame; verify that total time is reduced (the old pipeline compilation overhead is gone).

**Verify**: `cargo build` → exit 0; visual inspection shows same galaxy image, responsive interaction

## Test plan

No existing test infrastructure. Plan 002 will add tests; do not add tests for this plan. The smoke test (step 5) is the verification gate.

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy` exits 0 (pre-existing `too_many_arguments` warnings acceptable; no new warnings)
- [ ] `GpuCompute` struct exists in `src/gpu.rs` holding shader module, bind group layout, and compute pipeline
- [ ] `App` stores `gpu_compute: Option<GpuCompute>` and `rgba_buffer: Option<wgpu::Buffer>`
- [ ] `compute_galaxy` no longer calls `device.create_shader_module`, `device.create_bind_group_layout`, `device.create_pipeline_layout`, or `device.create_compute_pipeline`
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] Manual smoke test passes (app runs, renders, responds to input)

## STOP conditions

Stop and report back if:

- The code at the locations in "Current state" doesn't match the excerpts (the codebase has drifted).
- `cargo check` fails after any step and the fix isn't obvious from the error message.
- The refactored `compute_galaxy` produces visually different output from before (color shift, missing stars, etc.).
- A step's verification fails twice.

## Maintenance notes

- If new shader entry points are added to `galaxy-shader/src/lib.rs`, the `GpuCompute` struct may need additional compute pipelines (one per entry point). The current shader only uses `render_scene`.
- The `rgba_buffer` recreation logic mirrors `recreate_texture()` — if the texture recreation pattern changes, update the buffer recreation to match.
