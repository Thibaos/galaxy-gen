# Plan 013: Cache compute bind group across frames

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat HEAD~1..HEAD -- src/gpu.rs`
> If `compute_galaxy` or `GpuCompute` changed since this plan was written,
> treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Category**: performance
- **Depends on**: none
- **Planned at**: commit `1647567`, 2026-06-25

## Why this matters

Every frame, `compute_galaxy()` creates a new `scene_bg` bind group via
`device.create_bind_group()`. The two bound resources — `uniform_buffer`
and `rgba_buffer` — are stable objects whose GPU addresses don't change
across frames (contents are updated via `write_buffer`, not reallocation).
Recreating the bind group is wasted work: a bind group creation involves
driver-side validation and descriptor set allocation. On some backends
(e.g. DX12/Vulkan) this triggers descriptor pool operations that add
latency.

Caching the bind group once and reusing it every frame eliminates this
overhead — a ~0.5–2ms per-frame win on integrated GPUs.

## Current state

**Relevant code**: `src/gpu.rs:560-583` (inside `compute_galaxy`):

```rust
let scene_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
    label: Some("scene"),
    layout: &compute.bind_group_layout,
    entries: &[
        wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: rgba_buffer.as_entire_binding(),
        },
    ],
});
```

`uniform_buffer` and `rgba_buffer` are created once (via `get_or_insert_with`
on the `App` struct in `main.rs`) and their bindings are valid for the
lifetime of the buffer. The bind group can therefore be created once.

**Constraint**: The bind group references `rgba_buffer`, which is recreated
whenever `image_w` or `image_h` changes (via `recreate_texture` in
`main.rs`). On resize, the cached bind group MUST be invalidated and
recreated with the new `rgba_buffer`.

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**
- `src/gpu.rs` — add `scene_bind_group: Option<wgpu::BindGroup>` to
  `GpuCompute`, create it on first use (lazy init), expose a method to
  invalidate on resize, consume cached bind group in `compute_galaxy`
- `src/main.rs` — call invalidation method when `rgba_buffer` is recreated

## Steps

### Step 1: Add cached bind group to `GpuCompute`

In `src/gpu.rs`, add a field to `GpuCompute`:

```rust
pub struct GpuCompute {
    pub module: wgpu::ShaderModule,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::ComputePipeline,
    pub scene_bind_group: Option<wgpu::BindGroup>,
}
```

Update the constructor to initialize it as `None`:

```rust
Self {
    module,
    bind_group_layout,
    pipeline,
    scene_bind_group: None,
}
```

### Step 2: Add lazy-init and invalidation methods

Add to `impl GpuCompute`:

```rust
/// Return the scene bind group, creating it lazily if needed.
/// Must be called after uniform_buffer and rgba_buffer are stable.
pub fn ensure_scene_bind_group(
    &mut self,
    device: &wgpu::Device,
    uniform_buffer: &wgpu::Buffer,
    rgba_buffer: &wgpu::Buffer,
) -> &wgpu::BindGroup {
    self.scene_bind_group.get_or_insert_with(|| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: rgba_buffer.as_entire_binding(),
                },
            ],
        })
    })
}

/// Drop the cached bind group (call when rgba_buffer is recreated).
pub fn invalidate_scene_bind_group(&mut self) {
    self.scene_bind_group = None;
}
```

### Step 3: Update `compute_galaxy` to use cached bind group

Change the `compute_galaxy` signature to accept a `&wgpu::BindGroup` instead
of constructing one:

**Before:**
```rust
pub fn compute_galaxy(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    compute: &GpuCompute,
    rgba_buffer: &wgpu::Buffer,
    uniform_buffer: &wgpu::Buffer,
    image_w: u32,
    image_h: u32,
    target_texture: &wgpu::Texture,
) {
```

**After:**
```rust
pub fn compute_galaxy(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    compute: &GpuCompute,
    rgba_buffer: &wgpu::Buffer,
    uniform_buffer: &wgpu::Buffer,
    scene_bind_group: &wgpu::BindGroup,
    image_w: u32,
    image_h: u32,
    target_texture: &wgpu::Texture,
) {
```

Replace the bind group creation block with direct use:

```rust
// ── uniforms (pre-written by caller) ────────────────────
// (remove the create_bind_group call entirely)

// ── dispatch ─────────────────────────────────────────────
let thread_group_x = image_w.div_ceil(8);
let thread_group_y = image_h.div_ceil(8);

let mut encoder =
    device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

{
    let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("scene"),
        timestamp_writes: None,
    });
    cpass.set_pipeline(&compute.pipeline);
    cpass.set_bind_group(0, scene_bind_group, &[]);
    cpass.dispatch_workgroups(thread_group_x, thread_group_y, 1);
}
// ... rest unchanged
```

### Step 4: Wire invalidation in `main.rs`

In `main.rs`, locate the `recreate_*.unwrap()` calls that recreate
`rgba_buffer`. Whenever `rgba_buffer` is reassigned, invalidate:

```rust
self.rgba_buffer = Some(recreate_texture(...));
if let Some(ref mut compute) = self.gpu_compute {
    compute.invalidate_scene_bind_group();
}
```

### Step 5: Update call site in `main.rs`

In `redraw()`, before calling `compute_galaxy`, ensure + retrieve the cached
bind group:

```rust
let scene_bg = self.gpu_compute.as_mut().unwrap()
    .ensure_scene_bind_group(device, self.uniform_buffer.as_ref().unwrap(), self.rgba_buffer.as_ref().unwrap());

gpu::compute_galaxy(
    device, queue,
    self.gpu_compute.as_ref().unwrap(),
    self.rgba_buffer.as_ref().unwrap(),
    self.uniform_buffer.as_ref().unwrap(),
    scene_bg,
    self.render_w, self.render_h,
    display_tex,
);
```

The `gpu_compute` must be `as_mut()` for `ensure_scene_bind_group` (takes
`&mut self`). Adjust the destructure or borrow order accordingly.

### Step 6: Validate

```bash
cargo check
cargo clippy -- -D warnings
cargo test
```

## Test plan

- All 45 existing tests must pass (no new tests — this is a pure refactor
  with no change in visible behavior)
- Manual smoke test: launch app in 2D mode, verify galaxy renders correctly;
  switch to 3D, verify ray-march still works; resize window, verify no
  crash and bind group is correctly invalidated/recreated

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0 (all 45 pass)
- [ ] Bind group is created once, not every frame
- [ ] Resize correctly invalidates and recreates bind group
- [ ] `plans/README.md` updated

## STOP conditions

- `compute_galaxy` signature incompatible with any call site beyond `main.rs`
- Resize causes panic (bind group referencing freed `rgba_buffer`)
- Any existing test fails
