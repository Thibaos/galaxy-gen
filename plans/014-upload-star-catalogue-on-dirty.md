# Plan 014: Upload star catalogue only when dirty

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat HEAD~1..HEAD -- src/main.rs src/gpu.rs`
> If the star catalogue upload logic or `GpuStars` struct changed since this
> plan was written, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Category**: performance
- **Depends on**: none
- **Planned at**: commit `1647567`, 2026-06-25

## Why this matters

Every frame, the full star instance buffer (header + up to 524,288 stars ×
28 bytes = ~14.7 MB) is uploaded to the GPU via `queue.write_buffer` even
when the star catalogue hasn't changed. The catalogue is only regenerated
when the galaxy preset changes or `ensure_star_catalogue` detects a dirty
flag.

Uploading 14+ MB per frame at 60 FPS = ~880 MB/s of wasted PCIe bandwidth.
On integrated GPUs (UMA), this is pure CPU-side memcpy waste. On discrete
GPUs it saturates the bus with redundant data.

The fix: add a `star_catalogue_uploaded: bool` flag. Set it to `false` when
the catalogue is regenerated; set it to `true` after the upload. Skip the
`write_buffer` calls when the flag is already `true`.

## Current state

**Relevant code**: `src/main.rs:779-786` (inside `redraw()`, after the
compute dispatch):

```rust
if self.render_mode == 1 && self.show_stars && !self.star_catalogue.is_empty() {
    let gpu_stars = self.gpu_stars.as_ref().unwrap();
    self.write_view_proj_matrix(queue, gpu_stars);
    queue.write_buffer(&gpu_stars.instance_buffer, 0, bytemuck::cast_slice(&[count, max]));
    queue.write_buffer(&gpu_stars.instance_buffer, 8, bytemuck::cast_slice(&self.star_catalogue));
    let mut rpass = encoder.begin_render_pass(...);
    // ... instanced draw ...
}
```

The view-proj matrix and brightness must update every frame (camera moves,
brightness slider changes). But the instance buffer (two `write_buffer`
calls) only needs to update when the catalogue changes.

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**
- `src/main.rs` — add `star_catalogue_uploaded: bool` to `App` struct,
  set it `false` in `ensure_star_catalogue()` when catalogue is generated,
  guard the two instance `write_buffer` calls with it, set it `true` after
  upload; add to egui debug overlay

**Out of scope:**
- View-proj matrix upload (must remain per-frame)
- Brightness buffer upload (must remain per-frame)
- Changing `GpuStars` or `generate_star_catalogue`

## Steps

### Step 1: Add flag to `App` struct

In `src/main.rs`, add to the `App` struct (near the other star-related
fields):

```rust
star_catalogue_uploaded: bool,
```

Initialize in `App::new()`:

```rust
star_catalogue_uploaded: false,
```

### Step 2: Set flag in `ensure_star_catalogue`

In `ensure_star_catalogue()`, after the line that assigns
`self.star_catalogue = gpu::generate_star_catalogue(...)`, add:

```rust
self.star_catalogue_uploaded = false;
```

This ensures any catalogue regeneration triggers a re-upload.

### Step 3: Guard instance buffer uploads

In `redraw()`, wrap the two instance `write_buffer` calls:

**Before:**
```rust
queue.write_buffer(&gpu_stars.instance_buffer, 0, bytemuck::cast_slice(&[count, max]));
queue.write_buffer(&gpu_stars.instance_buffer, 8, bytemuck::cast_slice(&self.star_catalogue));
```

**After:**
```rust
if !self.star_catalogue_uploaded {
    queue.write_buffer(&gpu_stars.instance_buffer, 0, bytemuck::cast_slice(&[count, max]));
    queue.write_buffer(&gpu_stars.instance_buffer, 8, bytemuck::cast_slice(&self.star_catalogue));
    self.star_catalogue_uploaded = true;
}
```

Keep `write_view_proj_matrix` BEFORE this block (it always runs) and the
draw call after.

### Step 4: Show flag in egui debug

Add a line in the egui sidebar (e.g. near the star controls):

```rust
ui.label(format!("Catalogue: {} stars {}", self.star_catalogue.len(),
    if self.star_catalogue_uploaded { "(synced)" } else { "(dirty)" }));
```

This is useful for verifying the optimization works during development.

### Step 5: Validate

```bash
cargo check
cargo clippy -- -D warnings
cargo test
```

## Test plan

- All 45 existing tests pass (no behavioral change)
- Manual test: launch app, switch to 3D mode, toggle Show Stars on — stars
  appear. Toggle Show Stars off/on — stars reappear without visible delay.
  Switch preset — star field updates with new galaxy shape.
- Manual verification: the debug label should show "(synced)" after the
  first frame and only flip to "(dirty)" on preset change.

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0 (all 45 pass)
- [ ] Instance buffer only uploaded when catalogue is dirty
- [ ] View-proj and brightness still updated every frame
- [ ] Preset switch triggers re-upload
- [ ] `plans/README.md` updated

## STOP conditions

- Star rendering breaks (stars disappear or jitter) after guard addition
- Preset switch doesn't update star field
- Any existing test fails
