# Plan 008: Fix spiral arm formula to use logarithmic spiral

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6978e9e..HEAD -- galaxy-shader/src/lib.rs src/galaxy.rs src/gpu.rs galaxies/ngc628.toml`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts below against the live code before proceeding;
> on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M (shader formula change, preset retuning, test additions)
- **Risk**: MED (changes visual output of both presets; pitch values rebase)
- **Depends on**: none (plan 009 removes dead functions but is independent)
- **Category**: bug
- **Planned at**: commit `6978e9e`, 2026-06-22
- **Issue**: —

## Why this matters

Spiral galaxies follow logarithmic spirals (θ = cot(φ) × ln(r/r₀)) — the pitch
angle φ being the angle between the spiral tangent and the circle of constant
radius.  Arm spacing grows proportionally with radius.  The current shader
formula `theta - (r / hr) * arm_pitch` produces an Archimedean spiral with
*constant* inter-arm separation, not logarithmic.  The README and code comments
document "logarithmic spiral arms" but the implementation doesn't match.
Correcting the formula to a true logarithmic spiral makes the rendering
physically accurate, aligns the parameter convention with published pitch-angle
measurements (the NGC 628 TOML already records the correct 15° value), and
produces much more realistic spiral morphology — arms that curve continuously
across the disk instead of staying nearly radial out to the edge.

## Current state

The live arm formula lives in one function in the shader — `arm_modulation_2d`
at `galaxy-shader/src/lib.rs:78-97`.  (A 3D variant `arm_modulation` on the
same file line 131 is dead code — it will be removed by plan 009; this plan
ignores it.)

**Shaders — `galaxy-shader/src/lib.rs:78-97`** (the formula to fix):

```rust
/// Same spiral modulation but takes flat (x,z) instead of Vec3.
fn arm_modulation_2d(x: f32, z: f32, r: f32, p: &GalaxyUniform) -> f32 {
    if p.arm_count == 0 || p.arm_strength <= 0.0 {
        return 1.0;
    }
    let theta = x.atan2(z);
    let log_spiral = theta - (r / p.disk_scale_length) * p.arm_pitch;
                                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
                                 Linear in r → Archimedean spiral,
                                 not a logarithmic spiral.

    let arm_width = 1.0 / p.arm_concentration;
    let mut min_dtheta = PI;

    for k in 0..p.arm_count {
        let phase = log_spiral + TAU * (k as f32) / (p.arm_count as f32);
        let dtheta = rem_euclid(phase, TAU);
        let dtheta = if dtheta > PI { dtheta - TAU } else { dtheta };
        min_dtheta = min_dtheta.min(dtheta.abs());
    }

    let arg = min_dtheta / arm_width;
    1.0 + p.arm_strength * (-0.5 * arg * arg).exp()
}
```

The name `log_spiral` is misleading — the right-hand side is linear in `r`.

**Parameters — `src/galaxy.rs:17`** (doc comment to update):

```rust
    /// Pitch angle in radians (tightness of the winding).
    pub arm_pitch: f64,
```

**Milky Way preset — `src/galaxy.rs:55`**:

```rust
            arm_pitch: 0.22,
```

This value was tuned as a linear winding factor for the Archimedean formula,
not as a physical pitch angle.  For the MW, a realistic pitch angle is ~12°
(0.21 rad).  We'll change the value to `0.21` and add a literature citation.

**NGC 628 preset — `src/galaxy.rs:106`**:

```rust
            arm_pitch: 0.262, // 15°
```

This value already IS a true pitch angle (0.262 rad = 15.0°) as recorded in
`galaxies/ngc628.toml`.  The parameter does not change — only the formula
changes.  The comment `// 15°` already describes what the number means;
the fix makes the formula honor that meaning.

**GalaxyUniform — `src/gpu.rs:88`**: `pub arm_pitch: f32` — cast from `f64` in
`from_params()`.  No struct layout change needed — field offset stays at byte
16, size unchanged.

**Host-side tests — `src/gpu.rs:487`**: The test `zero_params_produce_zero_density`
sets `arm_pitch: 0.0`.  No assertion currently validates arm modulation output.
New tests will be added.

**No existing arm-modulation tests** anywhere.  The `#[cfg(test)]` blocks in
`src/gpu.rs` test disk/bulge/halo column profiles, but nothing validates the
arm formula.

## Commands you will need

| Purpose   | Command                    | Expected on success |
|-----------|----------------------------|---------------------|
| Check     | `cargo check`              | exit 0              |
| Clippy    | `cargo clippy -- -D warnings` | exit 0           |
| Test      | `cargo test`               | all pass (21 existing + N new) |
| Build     | `cargo build`              | exit 0              |
| Shader rebuild | `touch build.rs && cargo check` | exit 0 |

(Shader changes require a clean rebuild because `build.rs` caches SPIR-V
output.  `touch build.rs` forces re-invocation.  If that doesn't work, use
`cargo clean -p galaxy-shader && cargo check`.)

## Scope

**In scope** (files you may modify):

- `galaxy-shader/src/lib.rs` — change the `arm_modulation_2d` formula
- `src/galaxy.rs` — update MW pitch value (0.22 → 0.21), add citation comment
- `src/gpu.rs` — add host-side arm modulation tests, comment on `arm_pitch` field
- `galaxies/ngc628.toml` — verify / update the pitch angle field (probably unchanged)

**Out of scope** (do NOT touch):

- `src/main.rs` — no GUI changes
- `src/display.wgsl` — unchanged
- `build.rs` — unchanged
- `plans/` — only this plan and README (executor maintains status row)
- Dead shader code (`density`, `disk_density`, empty `arm_modulation`, `bulge_density`, `halo_density`, `smoothstep`) — plan 009 removes these; the live `arm_modulation_2d` is the only target here

## Conventions

The repo uses:

- `f64` on host, `f32` on GPU (cast `as f32` in `from_params`)
- Field comments in `GalaxyParams` include physical derivations and literature
- Tests compare floating-point values with `assert!((x - y).abs() < 0.01)`
- Shader functions use `PI` / `TAU` constants already defined at `line 26-27`
- Parameter safety: functions guard against `<= 0.0` denominators

## Steps

### Step 1: Add host-side arm modulation tests (before changing formula)

These tests validate the *current* incorrect formula against known inputs,
then will be updated in Step 3 to validate the new formula.  This follows the
characterization-test pattern already used for disk/bulge/halo columns.

Add to the test module at the bottom of `src/gpu.rs`, after the existing
`zero_params_produce_zero_density` test (approximately line 494):

```rust
/// Host-side replica of arm_modulation_2d using the CURRENT formula.
/// This will be updated in a later step when the formula changes.
fn host_arm_modulation_2d(x: f32, z: f32, p: &GalaxyParams) -> f32 {
    let hr = p.disk_scale_length as f32;
    let r = (x * x + z * z).sqrt();
    if p.arm_count == 0 || p.arm_strength <= 0.0 || hr <= 0.0 {
        return 1.0;
    }
    let theta = x.atan2(z);
    // CURRENT formula (Archimedean) — will become logarithmic in step 3
    let log_spiral = theta - (r / hr) * p.arm_pitch as f32;
    let arm_width = 1.0 / p.arm_concentration as f32;
    let mut min_dtheta = std::f32::consts::PI;
    for k in 0..p.arm_count {
        let phase = log_spiral + std::f32::consts::TAU * (k as f32) / (p.arm_count as f32);
        let dtheta = phase % std::f32::consts::TAU;
        let dtheta = if dtheta > std::f32::consts::PI { dtheta - std::f32::consts::TAU } else { dtheta };
        min_dtheta = min_dtheta.min(dtheta.abs());
    }
    let arg = min_dtheta / arm_width;
    1.0 + p.arm_strength as f32 * (-0.5 * arg * arg).exp()
}

#[test]
fn arm_modulation_no_arms_returns_one() {
    let mut p = GalaxyParams::milky_way();
    p.arm_count = 0;
    // any position
    assert!((host_arm_modulation_2d(1000.0, 500.0, &p) - 1.0).abs() < 1e-6);
}

#[test]
fn arm_modulation_zero_strength_returns_one() {
    let mut p = GalaxyParams::milky_way();
    p.arm_strength = 0.0;
    assert!((host_arm_modulation_2d(5000.0, 3000.0, &p) - 1.0).abs() < 1e-6);
}

#[test]
fn arm_modulation_is_positive() {
    // Test both presets at various radii and azimuths.
    for (label, p) in [
        ("MW", GalaxyParams::milky_way()),
        ("NGC628", GalaxyParams::ngc628()),
    ] {
        for (r, theta) in &[
            (1000.0_f32, 0.0_f32),
            (5000.0, 0.5),
            (15000.0, 1.2),
            (25000.0, 2.0),
        ] {
            let x = r * theta.sin();
            let z = r * theta.cos();
            let m = host_arm_modulation_2d(x, z, &p);
            assert!(m.is_finite() && m > 0.0,
                "{label} modulation at r={r}, θ={theta} = {m} not positive+finite");
        }
    }
}

#[test]
fn arm_modulation_enhances_along_arms() {
    // At theta = (r/hr)*pitch (zero phase), we should be at an arm peak:
    // min_dtheta ~ 0, arg ~ 0, exp(-0.5*0) = 1, modulation ≈ 1 + strength * 1
    let mut p = GalaxyParams::milky_way();
    p.arm_strength = 1.0;
    let r = p.disk_scale_length as f32; // r = hr
    let theta_on_arm = (r / p.disk_scale_length as f32) * p.arm_pitch as f32;
    let x = r * theta_on_arm.sin();
    let z = r * theta_on_arm.cos();
    let m = host_arm_modulation_2d(x, z, &p);
    // Near an arm, modulation should be close to 1 + strength = 2
    assert!((m - 2.0).abs() < 0.01, "on-arm modulation = {m}, expected ~2.0");
}

#[test]
fn arm_modulation_periodic_across_arms() {
    // Two positions with same r but theta differing by exactly one inter-arm
    // spacing (TAU / arm_count) should have identical modulation.
    let p = GalaxyParams::milky_way();
    let r = 8000.0f32;
    let theta0 = 1.0f32;
    let spacing = std::f32::consts::TAU / p.arm_count as f32;
    let x0 = r * theta0.sin();
    let z0 = r * theta0.cos();
    let x1 = r * (theta0 + spacing).sin();
    let z1 = r * (theta0 + spacing).cos();
    let m0 = host_arm_modulation_2d(x0, z0, &p);
    let m1 = host_arm_modulation_2d(x1, z1, &p);
    assert!((m0 - m1).abs() < 1e-5,
        "modulation at θ={theta0} = {m0}, at θ={} = {m1}; should match",
        theta0 + spacing);
}
```

**Verify**: `cargo test` → all tests pass (21 existing + 5 new = 26 total).

The `arm_modulation_is_positive` test exercises both Milky Way and NGC 628 presets
at on-axis and off-axis positions.  The guard-rail tests (`no_arms`, `zero_strength`)
only need one preset since they force parameters that override the physics; MW is
sufficient.

### Step 2: Change the shader formula to logarithmic spiral

Edit `galaxy-shader/src/lib.rs`, function `arm_modulation_2d` (lines 78-97).
Replace the single line that computes `log_spiral`:

**Old** (line 83):

```rust
    let log_spiral = theta - (r / p.disk_scale_length) * p.arm_pitch;
```

**New**:

```rust
    // Logarithmic spiral: θ = cot(φ) × ln(r/r₀)
    // Guard r > 0 to avoid ln(0); use ln(1 + r/hr) for smooth centre
    let cot_phi = 1.0 / p.arm_pitch.tan();
    let log_spiral = theta - cot_phi * (1.0 + r / p.disk_scale_length).ln();
```

Update the doc comment on `arm_modulation_2d` (line 77).  The current comment:

```rust
/// Same spiral modulation but takes flat (x,z) instead of Vec3.
```

Change to:

```rust
/// Logarithmic spiral arm modulation.
///
/// Each arm follows θ = cot(φ)·ln(1 + r/h_r)  where φ = `arm_pitch`
/// is the true pitch angle (angle between spiral tangent and the
/// tangent circle).  The `1 +` avoids a singularity at the origin
/// where the bulge dominates anyway.
```

**Verify**: `touch build.rs && cargo check` → exit 0.  If `cargo check` alone
doesn't recompile the shader, run `cargo clean -p galaxy-shader && cargo check`.

### Step 3: Update the host-side replica in tests

In `src/gpu.rs`, update the `host_arm_modulation_2d` helper to match the new
formula.  Replace the `log_spiral` computation inside the helper:

**Old** (the line added in Step 1):

```rust
    let log_spiral = theta - (r / hr) * p.arm_pitch as f32;
```

**New**:

```rust
    let cot_phi = 1.0 / (p.arm_pitch as f32).tan();
    let log_spiral = theta - cot_phi * (1.0 + r / hr).ln();
```

Update the `arm_modulation_enhances_along_arms` test.  The on-arm condition
changes — with the log formula, phase = 0 occurs at:

```
theta - cot(φ) × ln(1 + r/hr) = 0  →  theta = cot(φ) × ln(1 + r/hr)
```

Replace the test body:

```rust
#[test]
fn arm_modulation_enhances_along_arms() {
    // At theta = cot(φ)·ln(1+r/hr) the phase is zero → on-arm peak.
    // Use r = 0.5·hr to keep theta safely within [-π, π], avoiding
    // atan2 wrap-around for pitch values up to ~0.35 rad (20°).
    let mut p = GalaxyParams::milky_way();
    p.arm_strength = 1.0;
    let r = 0.5 * p.disk_scale_length as f32;
    let cot_phi = 1.0 / (p.arm_pitch as f32).tan();
    let theta_on_arm = cot_phi * (1.0 + r / p.disk_scale_length as f32).ln();
    let x = r * theta_on_arm.sin();
    let z = r * theta_on_arm.cos();
    let m = host_arm_modulation_2d(x, z, &p);
    assert!((m - 2.0).abs() < 0.01, "on-arm modulation = {m}, expected ~2.0");
}
```

**Verify**: `cargo test` → all tests pass (26 total: 21 existing + 5 arm modulation).

Verify that the on-arm test still passes after the MW pitch change in step 4 as
well — the test uses `p.arm_pitch` dynamically so it should adapt.  If it fails
at step 4, the `atan2` wrap-around guard in the test body (the `0.5·hr` radius)
was likely insufficient for the new pitch value; report back.

### Step 4: Update the Milky Way pitch angle to a physical value

In `src/galaxy.rs`, Milky Way preset (line ~55), change the pitch angle:

**Old**:

```rust
            arm_pitch: 0.22,
```

**New**:

```rust
            // Pitch angle: ~12° = 0.21 rad (consistent with MW
            // arm measurements — Lin & Shu 1964, Vallee 2005).
            arm_pitch: 0.21,
```

Also update the field-level doc comment in the `GalaxyParams` struct at line ~17:

**Old**:

```rust
    /// Pitch angle in radians (tightness of the winding).
    pub arm_pitch: f64,
```

**New**:

```rust
    /// Pitch angle φ in radians — the angle between the spiral tangent
    /// and the circle of constant radius.  Typical values: 0.10 (6°, tight)
    /// to 0.35 (20°, open).  Used in the logarithmic spiral formula.
    pub arm_pitch: f64,
```

**Verify**: `cargo check` → exit 0.  `cargo test` → all 26 tests pass (the
on-arm test should still pass — it reads `arm_pitch` dynamically from the
preset and the `0.5·hr` radius keeps θ in [-π, π] for pitch ≤ 0.35).

### Step 5: Verify NGC 628 TOML and preset are consistent

**Check**: `src/galaxy.rs:106` already uses `arm_pitch: 0.262, // 15°` and
`galaxies/ngc628.toml` already records `pitch_angle_rad = 0.262`.  These are
already true pitch angles.  The parameter value does not change — only the
formula in the shader now correctly interprets it.

No code changes needed in this step.  Just verify:

```bash
grep 'arm_pitch.*0.262' src/galaxy.rs && echo "OK" || echo "MISMATCH"
grep 'pitch_angle_rad.*0.262' galaxies/ngc628.toml && echo "OK" || echo "MISMATCH"
```

If both print "OK", move on.

### Step 6: Run full verification suite

```bash
cargo clean -p galaxy-shader && cargo check
cargo clippy -- -D warnings
cargo test
cargo build
```

All four must exit 0.  `cargo test` must show 26 tests passing (21 original +
5 new arm modulation tests).

### Step 7: Visual smoke test (manual)

Launch the app (`cargo run`).  Confirm:

- The spiral arms now show continuous curvature (arcs, not straight-ish lines).
- The arms wind more tightly with increasing radius (logarithmic twist).
- Both presets (Milky Way + NGC 628) render without artifacts.
- The arm sliders in the egui sidebar still work: changing pitch,
  concentration, strength, and arm count produces visible changes.
- The preset switching still works.
- No shader compilation panics at startup.

## Test plan

- 5 new tests in `src/gpu.rs` test module (added in Step 1, updated in Step 3):
  - `arm_modulation_no_arms_returns_one` — arm_count=0 → modulation=1
  - `arm_modulation_zero_strength_returns_one` — strength=0 → modulation=1
  - `arm_modulation_is_positive` — modulation is finite + positive at various radii
    and azimuths for both Milky Way and NGC 628 presets
  - `arm_modulation_enhances_along_arms` — on exact arm path, modulation ≈ 2 (strength=1)
  - `arm_modulation_periodic_across_arms` — identical modulation one inter-arm spacing apart
- All 21 existing tests continue to pass
- Verification: `cargo test` → 26 passed

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0; 26 tests pass (21 existing + 5 new arm modulation)
- [ ] `cargo build` exits 0
- [ ] `arm_modulation_2d` in `galaxy-shader/src/lib.rs` uses `cot(φ) × ln(1 + r/hr)`, not `(r/hr) × pitch`
- [ ] Milky Way preset uses `arm_pitch: 0.21` (12° physical pitch, with literature comment)
- [ ] NGC 628 preset arm_pitch unchanged at `0.262` (15° — already a physical value)
- [ ] `GalaxyParams.arm_pitch` doc comment describes it as a true pitch angle in logarithmic spiral context
- [ ] No files outside the in-scope list are modified (`git diff --stat`)
- [ ] A new `galaxy.png` renders with visibly curved, logarithmic spiral arms

## STOP conditions

Stop and report back if:

- The code at the locations in "Current state" doesn't match the excerpts
  (the codebase has drifted since this plan was written at commit `6978e9e`).
- `touch build.rs && cargo check` fails to rebuild the shader after the
  formula change — the shader uses `tan()`, `ln()`, and f32 division which
  are available via spirv-std's `Real` trait, but report any compilation
  error from the SPIR-V build step.
- A step's verification fails twice after a reasonable fix attempt.
- The fix appears to require touching an out-of-scope file.
- The host-side `rem_euclid` in Step 1 tests doesn't match the shader's
  `rem_euclid` function — the shader uses a custom Euclidean remainder
  for f32, while `%` on f32 in std Rust does truncating remainder.
  Specifically, if `arm_modulation_periodic_across_arms` fails with a
  modulation difference around ~1.0 (the arm_width is 0.2, so a TAU/2
  phase error puts you in the inter-arm trough), the remainder mismatch
  is the cause.  If test results are off near phase boundaries, replace
  every `phase % TAU` in the helper with a `rem_euclid_f32` helper that
  matches the shader:

  ```rust
  fn rem_euclid_f32(x: f32, y: f32) -> f32 {
      let r = x % y;
      if r < 0.0 { r + y } else { r }
  }
  ```

## Maintenance notes

- If `arm_pitch` is ever changed in a future preset, it must be a true pitch
  angle in radians, not a winding-rate parameter.  Typical spiral galaxies have
  pitch angles in the range 5–25° (0.09–0.44 rad).
- The `ln(1 + r/hr)` form transitions from linear (Archimedean-like) near
  the centre (r ≪ hr, where ln(1 + r/hr) ≈ r/hr) to fully logarithmic in
  the outer disk (r ≫ hr, where ln(1 + r/hr) ≈ ln(r/hr)).  This is by
  design — the bulge dominates the inner region, and a pure log spiral
  has infinite winding at the origin (ln(0) = −∞).  If future work requires
  the pure log form, use `ln(max(r, ε)/r₀)` with a small ε guard, but the
  current smoothing is preferable for rendering.
- If the shader is later adapted for 3D rendering (not column-density
  projection), the 3D arm modulation (newly deleted by plan 009, or a new
  function) must use the same formula.
- The arm width (controlled by `arm_concentration`) is measured in radians
  in the phase-θ coordinate.  With the log-spiral formula, the physical
  arm width at radius r is approximately `r × arm_width × cot(φ)`.  If arms
  appear too narrow/fat, adjust `arm_concentration` in the presets.
- The egui sidebar slider for arm_pitch ranges 0.05..=0.8.  0.05 rad ≈ 2.9°
  (very tight, approaches a ring), 0.8 rad ≈ 46° (extremely open, near-flat).
  This range is still appropriate after the formula change.
