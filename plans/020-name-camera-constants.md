# Plan 020: Name camera and orbit magic numbers

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat 1647567..HEAD -- src/main.rs`
> If camera constants or orbit logic changed since this plan was written,
> treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Category**: tech debt
- **Depends on**: 017 (if executed after the module split, constants should
  go in `camera.rs` instead of `main.rs`)
- **Planned at**: commit `1647567`, 2026-06-25

## Why this matters

Several numeric constants in the camera and orbit code have no names or
comments explaining their origin:

| Value | Location | Purpose |
|-------|----------|---------|
| `0.005` | `src/main.rs` (orbit sensitivity) | Radians per pixel of mouse drag — tuned empirically |
| `100.0` / `1_000_000.0` | `src/main.rs` | Near/far planes for perspective projection |
| `0.9999` | `src/main.rs` (look-at guard) | Degeneracy threshold when camera forward ≈ world up |
| `0.01` / `0.05` | `src/main.rs` (elevation clamp) | Avoid gimbal-lock near ±90° elevation |
| `1.0 / 1.15` | `src/main.rs` (ZOOM_SPEED) | Scroll-wheel zoom factor |
| `5_000.0` / `500_000.0` | `src/main.rs` (dist clamp) | Camera distance limits in light-years |

Without named constants, adjusting these values requires hunting through
the codebase, and their interrelationships (e.g. orbit sensitivity ×
camera distance = angular velocity) are invisible.

## Current state

Representative code where magic numbers appear — `src/main.rs`:

**Orbit sensitivity** in the `CursorMoved` handler:
```rust
self.camera_azimuth -= dx as f32 * 0.005;
self.camera_elevation = (self.camera_elevation + dy as f32 * 0.005)
    .clamp(0.01, std::f32::consts::FRAC_PI_2 - 0.01);
```

**Near/far planes and up-parallel guard** in `write_view_proj_matrix`:
```rust
let proj = glam::Mat4::perspective_rh(
    self.camera_fov.to_radians(), aspect, 100.0, 1_000_000.0
);
// ...
if dir.dot(up).abs() > 0.9999 {
    up = glam::Vec3::Z;
}
```

**Zoom and distance clamp** in the scroll handler:
```rust
let factor = if scroll > 0.0 { 1.0 / 1.15 } else { 1.15 };
self.camera_dist = (self.camera_dist * factor).clamp(5_000.0, 500_000.0);
```

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**
- `src/main.rs` (or `src/camera.rs` if plan 017 is done first) — extract
  magic numbers to named constants at module level
- Document each constant with a comment explaining its purpose and units

## Steps

### Step 1: Define constants

At the top of the module (or inside the `Camera` impl if in `camera.rs`):

```rust
/// Radians of orbit rotation per pixel of mouse drag.
/// Tuned for ~30° rotation over a ~100px drag at default window size.
const ORBIT_SENSITIVITY: f32 = 0.005;

/// Near clipping plane for perspective projection (light-years).
/// Must be > 0; 100 LY is effectively at the camera's position for galaxy scales.
const CAMERA_NEAR: f32 = 100.0;

/// Far clipping plane for perspective projection (light-years).
/// 1 million LY is well beyond any galaxy structure.
const CAMERA_FAR: f32 = 1_000_000.0;

/// Threshold for detecting when the camera forward vector is near-parallel
/// to the world up vector (0, 1, 0).  When |dot(forward, up)| > this value,
/// the look-at matrix is degenerate; we fall back to an alternative up vector.
const UP_PARALLEL_THRESHOLD: f32 = 0.9999;

/// Minimum elevation angle (radians from the XZ plane upward).
/// 0.01 rad ≈ 0.57° — prevents gimbal lock at exactly ±π/2.
const ELEVATION_MIN: f32 = 0.01;

/// Maximum elevation angle (radians from the XZ plane).
/// FRAC_PI_2 - 0.01 ≈ 1.56 rad ≈ 89.4°.
const ELEVATION_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

/// Scroll-wheel zoom in factor (1 / ZOOM_SPEED) and zoom out factor.
/// 1.15 gives ~15% distance change per scroll notch.
const ZOOM_SPEED: f64 = 1.15;

/// Minimum camera distance from target (light-years).
const CAMERA_DIST_MIN: f64 = 5_000.0;

/// Maximum camera distance from target (light-years).
const CAMERA_DIST_MAX: f64 = 500_000.0;

/// Default camera distance from target (light-years).
/// 100,000 LY gives a full-disk view for typical spiral galaxies.
const CAMERA_DIST_DEFAULT: f32 = 100_000.0;

/// Default vertical field of view (degrees).
const FOV_DEFAULT: f32 = 45.0;
```

### Step 2: Replace magic numbers

In `src/main.rs` (or `src/camera.rs` if plan 017 was already executed),
replace each literal with its named constant. The locations to search:

| Literal | Replace with | Lines to check |
|---------|-------------|----------------|
| `0.005` (orbit sensitivity) | `ORBIT_SENSITIVITY` | CursorMoved handler, `camera_azimuth` update |
| `100.0` (near plane) | `CAMERA_NEAR` | `write_view_proj_matrix` |
| `1_000_000.0` (far plane) | `CAMERA_FAR` | `write_view_proj_matrix` |
| `0.9999` (parallel threshold) | `UP_PARALLEL_THRESHOLD` | `write_view_proj_matrix`, `dir.dot(up).abs()` guard |
| `0.01` (clamp offset) | `ELEVATION_MIN`, `ELEVATION_MAX` | elevation `.clamp()` calls |
| `FRAC_PI_2 - 0.01` | `ELEVATION_MAX` | elevation `.clamp()` calls |
| `1.15` (zoom per notch) | `ZOOM_SPEED` | scroll handler |
| `5_000.0` (min dist) / `500_000.0` (max dist) | `CAMERA_DIST_MIN`, `CAMERA_DIST_MAX` | dist `.clamp()` calls |
| `100_000.0` (default dist) | `CAMERA_DIST_DEFAULT` | `App::new()` camera init |
| `45.0` (default FOV) | `FOV_DEFAULT` | `App::new()` camera init |

**Verify**: `cargo check` → exit 0

### Step 3: Full validation

```bash
cargo clippy -- -D warnings
cargo test
```

All 45 tests pass. No warnings.

## Git workflow

- Branch: `advisor/020-name-camera-constants`
- Commit message: `refactor: name camera and orbit magic numbers`
- Do NOT push or open a PR unless instructed.

## Test plan

- All 45 existing tests pass (constants are value-preserving renames)
- Manual test: orbit sensitivity feels the same, zoom speed unchanged,
  camera distance limits unchanged, no gimbal lock at extreme elevations

## Done criteria

- [ ] All camera/orbit/zoom magic numbers replaced with named constants
- [ ] Each constant has a doc comment explaining purpose and units
- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0 (all 45 pass)
- [ ] `plans/README.md` updated

## STOP conditions

- The code at the locations in "Current state" doesn't match the excerpts
  (the codebase has drifted since this plan was written).
- Any behavioral change in camera/orbit/zoom (different feel after rename)
- Any existing test fails

## Maintenance notes

- These constants are pure value-preserving renames. If the precision of
  any constant changes (e.g. `ORBIT_SENSITIVITY` from 0.005 to 0.004),
  that's a behavioral change — treat it as a separate plan.
- If plan 017 (module split) was executed first, these constants live in
  `src/camera.rs`. If not, they live in `src/main.rs`. Both paths produce
  identical behavior.
