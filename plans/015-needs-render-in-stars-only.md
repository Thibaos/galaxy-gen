# Plan 015: Clear `needs_render` in stars-only 3D mode

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat 1647567..HEAD -- src/main.rs`
> If the `needs_render` logic or the `do_compute` guard changed since this
> plan was written, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
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

**Relevant code** — `src/main.rs`:

1. **The `do_compute` guard and clearance** (~line 745):
```rust
let do_compute = self.render_mode != 1 || self.show_glow;

// ... uniform buffer write, bind group check ...

if self.needs_render && do_compute {
    // ... compute dispatch ...
    self.needs_render = false;  // <-- ONLY clearance point
}
```

2. **The star upload and encoder submission** (~line 779, after compute block):
```rust
// ... star catalogue upload, view-proj write, instanced draw ...

queue.submit(Some(encoder.finish()));
presentation
```
There is exactly ONE `queue.submit` call in `redraw()`. The unconditional
clearance will go right before or after it.

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

### Step 1: Move `needs_render` clearance to after all rendering

In `src/main.rs`, make two changes:

**A) Remove clearance from inside the compute guard** (~line 767). Delete:
```rust
self.needs_render = false;
```
from inside the `if self.needs_render && do_compute { ... }` block.

**B) Add unconditional clearance after all rendering.** Find the
existing `queue.submit(Some(encoder.finish()));` line (~line 810). Immediately
after it, add:

```rust
self.needs_render = false;
```

The resulting tail of `redraw()` looks like:

```rust
queue.submit(Some(encoder.finish()));
self.needs_render = false;
}
```

This single clearance covers all modes: 2D compute, 3D compute+stars, and
3D stars-only. No mode leaves the flag stuck.

**Verify**: `cargo check` → exit 0 (no errors)

### Step 2: Full validation

```bash
cargo check
cargo clippy -- -D warnings
cargo test
```

All 45 tests must pass. Fix any warnings before declaring done.

## Git workflow

- Branch: `advisor/015-needs-render-stars-only`
- Commit message: `fix: clear needs_render after every frame in all modes`
  (repo uses conventional-ish prefixes: `perf:`, `fix:`, `feat:`, `docs:`)
- Do NOT push or open a PR unless instructed.

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

- The code at the locations in "Current state" doesn't match the excerpts
  (the codebase has drifted since this plan was written).
- Screen goes black after first frame in any mode
- Any existing test fails

## Maintenance notes

- The invariant is now: `needs_render` is `false` at function return after
  every single call to `redraw()`. It is `true` only when a new render has
  been requested and has not yet been serviced. Any future code added to
  `redraw()` that could skip the `queue.submit` call (e.g. an early return
  for window minimization) must also clear `needs_render` or risk the same
  infinite-redraw bug.
- If a double-buffered or multi-pass rendering pipeline is added later,
  there should still be exactly one `needs_render = false` — at the point
  where the frame is confirmed submitted, not per-pass.
