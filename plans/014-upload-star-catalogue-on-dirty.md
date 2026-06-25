# Plan 014: Upload star catalogue only when dirty

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat 1647567..HEAD -- src/main.rs`
> If the star catalogue upload logic or `ensure_star_catalogue` changed
> since this plan was written, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
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

**Relevant code** — `src/main.rs`:

1. **Catalogue generation** (~line 415 in `ensure_star_catalogue()`):
```rust
fn ensure_star_catalogue(&mut self) {
    if self.star_catalogue_dirty || self.star_catalogue.is_empty() {
        self.star_catalogue = gpu::generate_star_catalogue(&self.params, gpu::MAX_STARS);
        self.star_catalogue_dirty = false;
    }
}
```

2. **Upload block** (~line 779 in `redraw()`, after the compute dispatch):
```rust
if self.render_mode == 1 && self.show_stars && !self.star_catalogue.is_empty() {
    let gpu_stars = self.gpu_stars.as_ref().unwrap();
    self.write_view_proj_matrix(queue, gpu_stars);
    let count = self.star_catalogue.len() as u32;
    let max = gpu::MAX_STARS as u32;
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

In `src/main.rs`, add to the `App` struct (near `star_catalogue_dirty` and
other star-related fields):

```rust
star_catalogue_uploaded: bool,
```

Initialize in `App::new()`:

```rust
star_catalogue_uploaded: false,
```

**Verify**: `cargo check` → exit 0 (field compiles, defaults correctly)

### Step 2: Set flag in `ensure_star_catalogue`

In `ensure_star_catalogue()` (~line 415), locate the block where the
catalogue is generated. After the `self.star_catalogue = ...` assignment
(*inside* the `if` block, right after it), add:

```rust
self.star_catalogue_uploaded = false;
```

The resulting block looks like:

```rust
if self.star_catalogue_dirty || self.star_catalogue.is_empty() {
    self.star_catalogue = gpu::generate_star_catalogue(&self.params, gpu::MAX_STARS);
    self.star_catalogue_dirty = false;
    self.star_catalogue_uploaded = false;
}
```

This ensures any catalogue regeneration triggers a re-upload.

**Verify**: `cargo check` → exit 0 (method compiles, flag used)

### Step 3: Guard instance buffer uploads

In `redraw()`, locate the star upload block (~line 779). Move the `count`
and `max` definitions inside the new guard along with the two
`write_buffer` calls:

**Before:**
```rust
let count = self.star_catalogue.len() as u32;
let max = gpu::MAX_STARS as u32;
queue.write_buffer(&gpu_stars.instance_buffer, 0, bytemuck::cast_slice(&[count, max]));
queue.write_buffer(&gpu_stars.instance_buffer, 8, bytemuck::cast_slice(&self.star_catalogue));
```

**After:**
```rust
if !self.star_catalogue_uploaded {
    let count = self.star_catalogue.len() as u32;
    let max = gpu::MAX_STARS as u32;
    queue.write_buffer(&gpu_stars.instance_buffer, 0, bytemuck::cast_slice(&[count, max]));
    queue.write_buffer(&gpu_stars.instance_buffer, 8, bytemuck::cast_slice(&self.star_catalogue));
    self.star_catalogue_uploaded = true;
}
```

Keep `write_view_proj_matrix` BEFORE this block (it always runs — camera
moves every frame) and the draw call after.

**Verify**: `cargo check` → exit 0 (guard compiles, `count`/`max` correctly scoped)

### Step 4: Show flag in egui debug

In the egui sidebar, near the star controls (Show Stars toggle, Star
brightness, Star size), add a debug label:

```rust
ui.label(format!("Catalogue: {} stars {}", self.star_catalogue.len(),
    if self.star_catalogue_uploaded { "(synced)" } else { "(dirty)" }));
```

This label can be kept permanently (single line, useful for debugging) or
removed after validation — your choice.

**Verify**: `cargo check` → exit 0 (egui label compiles)

### Step 5: Full validation

```bash
cargo check
cargo clippy -- -D warnings
cargo test
```

All 45 tests must pass. Fix any warnings before declaring done.

## Git workflow

- Branch: `advisor/014-star-catalogue-on-dirty`
- Commit message: `perf: upload star catalogue only when dirty`
  (repo uses conventional-ish prefixes: `perf:`, `fix:`, `feat:`, `docs:`)
- Do NOT push or open a PR unless instructed.

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

- The code at the locations in "Current state" doesn't match the excerpts
  (the codebase has drifted since this plan was written).
- Star rendering breaks (stars disappear or jitter) after guard addition
- Preset switch doesn't update star field
- Any existing test fails

## Maintenance notes

- Toggling `show_stars` off then on does NOT trigger a re-upload — the
  catalogue is unchanged. This is correct behavior; the stars are still
  in GPU memory.
- The debug label in the egui sidebar can be removed later if you decide
  the optimization is proven stable. Until then, it serves as a live
  invariant check.
- If a future feature allows per-star property changes (e.g. star color
  variation driven by a slider), the uploaded flag must be set to `false`
  alongside `star_catalogue_dirty` to trigger re-upload.
