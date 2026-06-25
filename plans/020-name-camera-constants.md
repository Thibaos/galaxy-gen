# Plan 020: Name camera and orbit magic numbers

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat HEAD~1..HEAD -- src/main.rs`
> If camera constants or orbit logic changed since this plan was written,
> treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
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

Search for each numeric literal in the camera/orbit/zoom code and replace
with the named constant. Key locations:

- `self.camera_azimuth -= dx as f32 * ORBIT_SENSITIVITY;`
- `self.camera_elevation = (...).clamp(ELEVATION_MIN, ELEVATION_MAX);`
- `if dir.dot(up).abs() > UP_PARALLEL_THRESHOLD { ... }`
- `let factor = if scroll > 0.0 { 1.0 / ZOOM_SPEED } else { ZOOM_SPEED };`
- `.clamp(CAMERA_DIST_MIN, CAMERA_DIST_MAX)`
- Default initialization values

Also update the `write_view_proj_matrix` near/far planes.

### Step 3: Validate

```bash
cargo check
cargo clippy -- -D warnings
cargo test
```

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

- Any behavioral change in camera/orbit/zoom (different feel after rename)
- Any existing test fails
