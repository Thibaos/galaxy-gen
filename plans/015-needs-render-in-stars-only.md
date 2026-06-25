# Plan 015: Clear `needs_render` in stars-only 3D mode

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat HEAD~1..HEAD -- src/main.rs`
> If the `needs_render` logic or the `do_compute` guard changed since this
> plan was written, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Category**: correctness
- **Depends on**: none
- **Planned at**: commit `1647567`, 2026-06-25

## Why this matters

When `render_mode == 1` (3D) and `show_glow == false` (stars-only view),
the `do_compute` boolean evaluates to `false`, causing the compute dispatch
block to be skipped entirely. Inside that block is the ONLY place where
`self.needs_render` is set to `false`.

Result: once `needs_render` is set to `true` (by any slider, preset switch,
or input event), it STAYS `true` forever in stars-only mode. The
`about_to_wait` handler sees `needs_render == true` and requests another
redraw — creating an infinite redraw loop at the maximum frame rate the GPU
can sustain, even when nothing is changing.

This wastes GPU power, CPU time, and generates unnecessary heat/noise on
laptops.

## Current state

**Relevant code**: `src/main.rs:745-768`:

```rust
let do_compute = render_mode != 1 || show_glow;

// ... buffer writes and bind group check ...

if self.needs_render && do_compute {
    // ... compute dispatch ...
    self.needs_render = false;  // <-- ONLY clearance point
}
```

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**
- `src/main.rs` — restructure the `needs_render` clearance to happen
  unconditionally (or at least in both compute + stars-only paths)

## Steps

### Step 1: Extract `needs_render` clearance

The simplest correct fix: add `self.needs_render = false;` at the END of the
redraw path, after ALL rendering work is complete. The compute block's
internal `needs_render = false` becomes redundant and can be removed, or we
keep both and clear unconditionally after.

**Option A (recommended — simplest, least risk):**

At the very end of the render logic in `redraw()`, after the encoder is
submitted, add:

```rust
self.needs_render = false;
```

And remove the `self.needs_render = false;` from inside the
`if self.needs_render && do_compute` block (line 767).

**Option B (keep guard on compute block)**

If a render is needed but in stars-only mode, still set `needs_render =
false` after the star render pass completes because the frame WAS rendered.

Either way, after the `queue.submit(Some(encoder.finish()));` call, add:

```rust
self.needs_render = false;
```

This ensures that after ANY frame is rendered (compute + stars, or
stars-only), the flag is cleared until the next input event sets it again.

### Step 2: Remove redundant clearance inside compute block

If using Option A, delete line 767:
```rust
self.needs_render = false;
```

This is now redundant since the unconditional clearance at the end handles it.

### Step 3: Validate

```bash
cargo check
cargo clippy -- -D warnings
cargo test
```

## Test plan

- All 45 existing tests pass
- Manual test: launch in 3D stars-only mode (Show Glow off). Verify GPU
  usage drops to idle (no continuous redraws). Move the mouse to orbit —
  redraws resume while dragging, then stop when idle.
- Verify 2D mode still updates correctly on pan/zoom.

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0 (all 45 pass)
- [ ] Stars-only 3D mode idles when no input is being processed
- [ ] 2D mode still redraws on slider changes, pan, and zoom
- [ ] `plans/README.md` updated

## STOP conditions

- 2D rendering stops updating (needs_render somehow not set before compute
  dispatch)
- Screen goes black after first frame in any mode
- Any existing test fails
