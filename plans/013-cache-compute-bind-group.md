# Plan 013: Cache compute bind group across frames

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat 1647567..HEAD -- src/gpu.rs src/main.rs`
> If `compute_galaxy`, `GpuCompute`, or the `rgba_buffer` recreation logic in
> `redraw()` changed since this plan was written, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
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

**Relevant `main.rs` context** — the `redraw()` function destructures `self`
into local bindings. You'll interact with two regions:

1. **Resize block** (~line 735): where `self.rgba_buffer` is reassigned
   when dimensions change:
```rust
if self.render_w != new_w || self.render_h != new_h {
    self.render_w = new_w;
    self.render_h = new_h;
    self.surface.configure(device, &config);
    // rgab_buffer is recreated here alongside the display texture
    self.rgba_buffer = Some(recreate_texture(device, new_w, new_h, ...));
    ...
}
```

2. **Compute dispatch** (~line 760): where `compute_galaxy` is called:
```rust
let uniform_data = ...;
queue.write_buffer(self.uniform_buffer.as_ref().unwrap(), 0, bytemuck::bytes_of(&uniform_data));

gpu::compute_galaxy(
    device, queue,
    self.gpu_compute.as_ref().unwrap(),
    self.rgba_buffer.as_ref().unwrap(),
    self.uniform_buffer.as_ref().unwrap(),
    self.render_w, self.render_h,
    display_tex,
);
```

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

**Verify**: `cargo check` → exit 0 (struct compiles with new field)

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

**Verify**: `cargo check` → exit 0 (methods compile, no callers yet)

### Step 3: Update `compute_galaxy` to use cached bind group

In `src/gpu.rs`, change the `compute_galaxy` function in two places:

**A) Signature** — add `scene_bind_group` parameter after `uniform_buffer`:

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

**B) Body** — delete the bind group creation block (currently lines 573–583:
the entire `let scene_bg = device.create_bind_group(...);` statement including
the trailing `);`). Replace the `cpass.set_bind_group` call inside the
compute pass to use the parameter:

```rust
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
// ... copy_buffer_to_texture unchanged ...
```

Note: `uniform_buffer` is no longer used inside `compute_galaxy` — it was
only consumed by the now-deleted `create_bind_group` call. Leave it in the
signature for now (removing it would cascade to the `main.rs` call site and
is a separate cleanup). The compiler may warn about an unused argument;
accept it.

**Verify**: `cargo check` → exit 0 *for gpu.rs*. Expect errors from
`main.rs` (signature mismatch at the call site — fixed in step 5).

### Step 4: Wire invalidation in `main.rs`

In `main.rs`, find every line where `self.rgba_buffer` is reassigned
(search: `self.rgba_buffer = Some(`). There are two locations:

1. **In `redraw()`** — inside the
   `if self.render_w != new_w || self.render_h != new_h` resize block.
   After the `self.rgba_buffer = Some(recreate_texture(...))` line, add:
```rust
if let Some(ref mut compute) = self.gpu_compute {
    compute.invalidate_scene_bind_group();
}
```

2. **In `App::init()`** — after the initial `rgba_buffer` creation. Same
   block as above (harmless since no cached bind group exists yet, but
   defensive).

**Verify**: `cargo check` → exit 0 for main.rs (invalidation code compiles;
still has signature mismatch from step 3)

### Step 5: Update call site in `main.rs`

In `redraw()`, locate the `compute_galaxy` call (~line 760). Replace it
to pass the cached bind group.

**Borrow checker constraint**: `ensure_scene_bind_group` takes `&mut self`
on `GpuCompute`. Call it BEFORE the existing field destructure that borrows
`self.gpu_compute` immutably. Insert this block right before the
`compute_galaxy` call (after the uniform buffer write):

```rust
// Ensure cached scene bind group (created lazily, reused across frames)
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

If the compiler rejects this due to overlapping mutable + immutable borrows
on `self.gpu_compute`, extract the `ensure_scene_bind_group` call to a
separate `let scene_bg = ...;` statement BEFORE any other field borrows.
The mutable borrow ends at the semicolon of that `let` statement, freeing
`self.gpu_compute` for the subsequent immutable borrow.

**Verify**: `cargo check` → exit 0 (no errors anywhere)

### Step 6: Full validation

```bash
cargo check
cargo clippy -- -D warnings
cargo test
```

All 45 tests must pass. Fix any warnings before declaring done.

## Git workflow

- Branch: `advisor/013-cache-bind-group`
- Commit message: `perf: cache compute bind group across frames`
  (repo uses conventional-ish prefixes: `perf:`, `fix:`, `feat:`, `docs:`)
- Do NOT push or open a PR unless instructed.

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

- The code at the locations in "Current state" doesn't match the excerpts
  (the codebase has drifted since this plan was written).
- `compute_galaxy` signature change breaks any call site beyond `main.rs`
- Resize causes panic (bind group referencing freed `rgba_buffer`)
- You discover that `uniform_buffer` or `rgba_buffer` ARE recreated every
  frame (the core assumption of this plan is false — report this).
- Any existing test fails

## Maintenance notes

- If `uniform_buffer` or `rgba_buffer` are ever made ephemeral (recreated
  every frame in a future refactor), this cache introduces a stale-buffer
  UAF and must be removed or guarded with a frame counter.
- `invalidate_scene_bind_group()` is the single point of invalidation — any
  future code that recreates `rgba_buffer` (e.g. for dynamic resolution
  scaling) MUST call it.
- The `uniform_buffer` parameter on `compute_galaxy` is now vestigial (only
  used by the removed `create_bind_group` call). If a future plan removes
  it, update the `main.rs` call site to drop the argument.
