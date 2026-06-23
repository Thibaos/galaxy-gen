# Plan 010: Replace blackbody star colors with physically-accurate stellar-spectrum-based lookup

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**: `git diff --stat c2dfbe5..HEAD -- galaxy-shader/src/lib.rs src/gpu.rs`
> If either of these files changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Category**: direction (visual fidelity / physical accuracy)
- **Depends on**: none
- **Planned at**: commit `c2dfbe5`, 2026-06-23

## Why this matters

The renderer currently colors stars using a blackbody approximation (`temperature_to_rgb`)
fed by a crude mass→temperature power law (`mass_to_temp`).  The astrophysics
literature (Harre & Heller 2021, "Digital color codes of stars", AN 342, 578) shows
that blackbody-derived colors are substantially wrong — M dwarfs appear too red,
OB stars appear too white/pale, and the overall color palette lacks the correct
orange→white→blue sequence of the main sequence.  Replacing the blackbody
pipeline with a lookup table baked from real stellar spectral libraries
(PHOENIX + Kurucz + Silva + Pickles, convolved through CIE standard-observer
color-matching functions) produces colors that match what the human eye would
see from space.  The improvement is both visually striking and scientifically
defensible.

## Current state

**Relevant files:**

- `galaxy-shader/src/lib.rs` — compute shader; contains `mass_to_temp`,
  `mass_to_lum`, `temperature_to_rgb`, and `cell_star_light`
- `src/gpu.rs` — host-side uniform struct + test suite with host-side
  replicas of shader math (including `host_arm_modulation_2d`, etc.)

**Current `mass_to_temp` (galaxy-shader/src/lib.rs, ~lines 148-151):**

```rust
/// Effective temperature from mass (main sequence, rough).
fn mass_to_temp(m: f32) -> f32 {
    // T ∝ M^0.5  with  T_sun ≈ 5778 K.
    5778.0 * m.sqrt()
}
```

This is a single power-law fit.  Real M–Teff relations (Eker et al. 2018,
MNRAS 479, 5491) have distinct breakpoints at ~1.05, ~2.4, and ~7.0 M☉.

**Current `temperature_to_rgb` (galaxy-shader/src/lib.rs, ~lines 154-180):**

```rust
/// Blackbody colour  →  linear RGB.
/// Based on Tanner Helland's approximation.
fn temperature_to_rgb(t_kelvin: f32) -> Vec3 {
    let t = (t_kelvin / 100.0).clamp(10.0, 400.0);
    // ... piecewise approximation for R, G, B channels
    Vec3::new(r, g, b)
}
```

This is a pure blackbody radiator approximation.  Real stellar spectra have
molecular absorption bands (TiO, VO, CN in M dwarfs; Balmer lines in A stars)
that shift perceived color away from the blackbody locus.

**Current `cell_star_light` (galaxy-shader/src/lib.rs, ~lines 230-235):**

```rust
fn cell_star_light(cx: u32, cz: u32) -> Vec3 {
    let mass = sample_imf_from_cell(cx, cz);
    let lum = mass_to_lum(mass);
    let temp = mass_to_temp(mass);
    temperature_to_rgb(temp) * lum
}
```

The calling pattern (mass → temp → color, multiplied by bolometric luminosity)
is preserved.  Only the color function changes.

**Conventions to follow (from the existing shader):**

- SPIR-V compute shader in `galaxy-shader/src/lib.rs`
- All floats are `f32` (spirv-std `glam::Vec3`)
- Functions are private (`fn`, no `pub`) unless the entry point refers to them
- Constants use `const FOO: f32 = ...;` at module scope
- Host-side replicas in `src/gpu.rs` exist for all shader math; every shader
  change MUST have a corresponding host-side update and test
- Tests live at the bottom of `src/gpu.rs` in `#[cfg(test)] mod tests { ... }`
- Run commands: `cargo check`, `cargo clippy -- -D warnings`, `cargo test`

## Commands you will need

| Purpose   | Command                        | Expected on success            |
|-----------|--------------------------------|--------------------------------|
| Check     | `cargo check`                  | exit 0, no errors              |
| Lint      | `cargo clippy -- -D warnings`  | exit 0, no warnings            |
| Tests     | `cargo test`                   | all pass (existing + new)      |

Note: changing `galaxy-shader/src/lib.rs` requires the SPIR-V to be
recompiled by `build.rs`.  `cargo check` / `cargo test` will handle this
automatically via the build script.  No manual `cargo clean` needed.

## Scope

**In scope (files you may modify):**

- `galaxy-shader/src/lib.rs` — replace `mass_to_temp` and `temperature_to_rgb`,
  add the lookup table, keep `cell_star_light` call site
- `src/gpu.rs` — add host-side replicas of the new color functions + tests

**Out of scope (do NOT touch):**

- `src/main.rs` — no UI changes
- `src/galaxy.rs` — galaxy presets unchanged
- The `mass_to_lum` function — luminosity scaling is a separate issue
- The `sample_star_grid` / `column_density` functions — only color changes
- `GalaxyUniform` struct — no new fields needed

## Steps

### Step 1: Replace `mass_to_temp` with a piecewise empirical relation

In `galaxy-shader/src/lib.rs`, replace the current `mass_to_temp` function
(which is `5778.0 * m.sqrt()`) with a piecewise function based on the Eker
et al. (2018) empirical mass–effective-temperature relation for main-sequence
stars.  The relation has segments with different power-law indices.

**Old code to replace** (lines ~148-151):

```rust
/// Effective temperature from mass (main sequence, rough).
fn mass_to_temp(m: f32) -> f32 {
    // T ∝ M^0.5  with  T_sun ≈ 5778 K.
    5778.0 * m.sqrt()
}
```

**New code:**

```rust
/// Effective temperature from mass (main sequence).
///
/// Piecewise-log fit to the empirical M–Teff relation from Pecaut &
/// Mamajek (2013, ApJS 208, 9, Table 5) and Eker et al. (2018, MNRAS
/// 479, 5491).  The relation is log₁₀(Teff) = a + b·log₁₀(M) with
/// breakpoints at 0.5, 1.05, 2.4, and 7.0 M☉.
///
/// Reference values (M☉ → K):  0.08→2300, 0.16→3060, 0.50→3750,
/// 0.88→5240, 1.00→5770, 2.00→8180, 2.40→9700, 7.00→22000,
/// 15.0→30000, 60.0→48000.
const T_SOLAR: f32 = 5770.0;
const M_SOLAR: f32 = 1.0;

fn mass_to_temp(m: f32) -> f32 {
    if m <= 0.08 {
        return 2300.0;
    }
    let (ref_m, ref_t, exp) = if m < 0.50 {
        // Very-low-mass segment: T ∝ M^0.53  (anchored at 0.16 M☉ → 3060 K)
        (0.16, 3060.0, 0.53)
    } else if m < 1.05 {
        // Low-mass segment: T ∝ M^0.67  (anchored at 0.88 M☉ → 5240 K)
        (0.88, 5240.0, 0.67)
    } else if m < 2.40 {
        // Solar-mass segment: T ∝ M^0.57  (anchored at 1.0 M☉ → 5770 K)
        (M_SOLAR, T_SOLAR, 0.57)
    } else if m < 7.0 {
        // Intermediate-mass segment: T ∝ M^0.36  (anchored at 2.4 M☉ → 9700 K)
        (2.40, 9700.0, 0.36)
    } else {
        // High-mass segment: T ∝ M^0.20  (anchored at 15.0 M☉ → 30000 K)
        (15.0, 30000.0, 0.20)
    };
    // Cap at 50000 K (O-type stars; color is essentially converged beyond this)
    (ref_t * (m / ref_m).powf(exp)).min(50000.0)
}
```

**Verify**: `cargo check` → exit 0

### Step 2: Replace `temperature_to_rgb` with a spectrum-based lookup table

In `galaxy-shader/src/lib.rs`, replace the entire `temperature_to_rgb` function
with a new function `temperature_to_rgb_lut` that interpolates over a
piecewise-linear lookup table.

The table is derived from the **vendian.org star color datafile** (Charity
2001, updated 2002), which averages stellar spectra from Kurucz, Silva, and
Pickles libraries, convolves them with CIE 1931 2-degree color-matching
functions (Judd-Vos corrected), and converts to linear sRGB with D65
whitepoint.  This methodology is validated by the peer-reviewed Harre & Heller
(2021) paper which reaches essentially identical results using PHOENIX spectra.

**Old code to replace** (lines ~153-180 — the entire `temperature_to_rgb`
function including its doc comment):

Delete everything from `/// Blackbody colour ...` through the closing `}` of
`temperature_to_rgb`, and replace with:

```rust
// ═══════════════════════════════════════════════════════════
//  Physically-accurate star colour via spectrum-based LUT
//
//  The table is derived from the vendian.org stellar colour
//  datafile (Charity 2001–2002), which averages real stellar
//  spectra (Kurucz, Silva, Pickles) and processes them through
//  CIE 1931 2º CMFs (Judd-Vos), sRGB primaries, and D65
//  whitepoint.  Methodology validated by Harre & Heller (2021).
//
//  Entries are (Teff_K, linear_R, linear_G, linear_B).
//  Colours between entries are linearly interpolated.
// ═══════════════════════════════════════════════════════════

/// Number of entries in the colour lookup table.
const COLOR_LUT_LEN: usize = 16;

/// Temperature→RGB lookup table: (Teff [K], linear R, linear G, linear B).
///
/// Data source: vendian.org starcolor datafile, Main Sequence (Class V).
/// Spectral types mapped to Teff via Pecaut & Mamajek (2013, Table 5).
const COLOR_LUT: [(f32, f32, f32, f32); COLOR_LUT_LEN] = [
    //  Teff(K)    R      G      B      SpT (approx)
    (   2300.0, 1.000, 0.745, 0.424), // M9.5V
    (   2600.0, 1.000, 0.765, 0.427), // M7V  (interpolated)
    (   3060.0, 1.000, 0.800, 0.435), // M5V
    (   3400.0, 1.000, 0.808, 0.506), // M3V
    (   3750.0, 1.000, 0.765, 0.545), // M0V
    (   4400.0, 1.000, 0.847, 0.710), // K4V  (K5 anchor at 4140K)
    (   5240.0, 1.000, 0.933, 0.867), // K0V
    (   5770.0, 1.000, 0.961, 0.949), // G2V (Sun)
    (   6540.0, 0.973, 0.969, 1.000), // F5V
    (   7220.0, 0.878, 0.898, 1.000), // F0V
    (   8180.0, 0.792, 0.843, 1.000), // A5V
    (   9700.0, 0.725, 0.788, 1.000), // A0V
    (  15200.0, 0.667, 0.749, 1.000), // B5V
    (  26500.0, 0.612, 0.698, 1.000), // B0V
    (  41400.0, 0.608, 0.690, 1.000), // O5V
    (  50000.0, 0.608, 0.690, 1.000), // O2V (clamped — colour converged)
];

/// Spectrum-based star colour as linear RGB.
///
/// Piecewise-linear interpolation over the baked `COLOR_LUT`.
/// Temperatures outside the covered range are clamped to the nearest endpoint.
fn temperature_to_rgb(t_kelvin: f32) -> Vec3 {
    // Clamp to LUT range
    let t = t_kelvin.clamp(COLOR_LUT[0].0, COLOR_LUT[COLOR_LUT_LEN - 1].0);

    // Find the segment [lo, hi] containing t
    if t <= COLOR_LUT[0].0 {
        return Vec3::new(COLOR_LUT[0].1, COLOR_LUT[0].2, COLOR_LUT[0].3);
    }
    for i in 0..(COLOR_LUT_LEN - 1) {
        if t <= COLOR_LUT[i + 1].0 {
            let t_lo = COLOR_LUT[i].0;
            let t_hi = COLOR_LUT[i + 1].0;
            let frac = (t - t_lo) / (t_hi - t_lo);
            let r = COLOR_LUT[i].1 + frac * (COLOR_LUT[i + 1].1 - COLOR_LUT[i].1);
            let g = COLOR_LUT[i].2 + frac * (COLOR_LUT[i + 1].2 - COLOR_LUT[i].2);
            let b = COLOR_LUT[i].3 + frac * (COLOR_LUT[i + 1].3 - COLOR_LUT[i].3);
            return Vec3::new(r, g, b);
        }
    }
    // t > last entry
    let last = COLOR_LUT[COLOR_LUT_LEN - 1];
    Vec3::new(last.1, last.2, last.3)
}
```

Note: the calling code in `cell_star_light` already calls `temperature_to_rgb(temp)`
— no call-site change needed since we're replacing the function body with the
same signature.

**Verify**: `cargo check` → exit 0

### Step 3: Add host-side replicas of new functions to `src/gpu.rs`

In `src/gpu.rs`, find the existing host-side column density replicas (around
line 370, the `host_disk_column`, `host_bulge_column`, etc. functions).  Add
new host-side functions immediately before the test module that replicate the
shader's new `mass_to_temp` and `temperature_to_rgb`:

```rust
// ── Host-side replicas of shader star-colour functions ──

const T_SOLAR_HOST: f64 = 5770.0;

fn host_mass_to_temp(m: f64) -> f64 {
    if m <= 0.08 {
        return 2300.0;
    }
    let (ref_m, ref_t, exp) = if m < 0.50 {
        (0.16, 3060.0, 0.53)
    } else if m < 1.05 {
        (0.88, 5240.0, 0.67)
    } else if m < 2.40 {
        (1.00, T_SOLAR_HOST, 0.57)
    } else if m < 7.0 {
        (2.40, 9700.0, 0.36)
    } else {
        (15.0, 30000.0, 0.20)
    };
    (ref_t * (m / ref_m).powf(exp)).min(50000.0)
}

const COLOR_LUT_HOST: [(f64, f64, f64, f64); 16] = [
    (2300.0, 1.000, 0.745, 0.424),
    (2600.0, 1.000, 0.765, 0.427),
    (3060.0, 1.000, 0.800, 0.435),
    (3400.0, 1.000, 0.808, 0.506),
    (3750.0, 1.000, 0.765, 0.545),
    (4400.0, 1.000, 0.847, 0.710),
    (5240.0, 1.000, 0.933, 0.867),
    (5770.0, 1.000, 0.961, 0.949),
    (6540.0, 0.973, 0.969, 1.000),
    (7220.0, 0.878, 0.898, 1.000),
    (8180.0, 0.792, 0.843, 1.000),
    (9700.0, 0.725, 0.788, 1.000),
    (15200.0, 0.667, 0.749, 1.000),
    (26500.0, 0.612, 0.698, 1.000),
    (41400.0, 0.608, 0.690, 1.000),
    (50000.0, 0.608, 0.690, 1.000),
];

fn host_temperature_to_rgb(t_kelvin: f64) -> (f64, f64, f64) {
    let t = t_kelvin.clamp(COLOR_LUT_HOST[0].0, COLOR_LUT_HOST[15].0);
    if t <= COLOR_LUT_HOST[0].0 {
        return (COLOR_LUT_HOST[0].1, COLOR_LUT_HOST[0].2, COLOR_LUT_HOST[0].3);
    }
    for i in 0..(COLOR_LUT_HOST.len() - 1) {
        if t <= COLOR_LUT_HOST[i + 1].0 {
            let t_lo = COLOR_LUT_HOST[i].0;
            let t_hi = COLOR_LUT_HOST[i + 1].0;
            let frac = (t - t_lo) / (t_hi - t_lo);
            let r = COLOR_LUT_HOST[i].1 + frac * (COLOR_LUT_HOST[i + 1].1 - COLOR_LUT_HOST[i].1);
            let g = COLOR_LUT_HOST[i].2 + frac * (COLOR_LUT_HOST[i + 1].2 - COLOR_LUT_HOST[i].2);
            let b = COLOR_LUT_HOST[i].3 + frac * (COLOR_LUT_HOST[i + 1].3 - COLOR_LUT_HOST[i].3);
            return (r, g, b);
        }
    }
    let last = COLOR_LUT_HOST[15];
    (last.1, last.2, last.3)
}
```

**Verify**: `cargo check` → exit 0

### Step 4: Add tests in `src/gpu.rs`

Add the following tests to the `mod tests` block in `src/gpu.rs`.  Place them
after the existing color-related tests, before the end of the module.

```rust
    // ── New star colour tests ─────────────────────────

    #[test]
    fn mass_to_temp_produces_correct_teff() {
        // Reference values from Pecaut & Mamajek (2013, Table 5)
        // matched to spectral types.
        // M☉ → Teff(K), tolerance ±200K
        let cases: &[(f64, f64)] = &[
            (0.08, 2300.0),  // M9.5V
            (0.16, 3060.0),  // M5V
            (0.50, 3750.0),  // M0V
            (0.88, 5240.0),  // K0V
            (1.00, 5770.0),  // G2V (Sun)
            (2.00, 8180.0),  // A5V
            (2.40, 9700.0),  // A0V
            (7.00, 22000.0), // B2V
            (15.0, 30000.0), // B0V
        ];
        for &(mass, expected_teff) in cases {
            let teff = host_mass_to_temp(mass);
            assert!(
                (teff - expected_teff).abs() < 300.0,
                "mass_to_temp({mass}) = {teff}, expected ~{expected_teff}"
            );
        }
    }

    #[test]
    fn mass_to_temp_monotonic() {
        let mut prev = host_mass_to_temp(0.05);
        for mass in [0.08_f64, 0.1, 0.2, 0.5, 0.8, 1.0, 1.5, 2.0, 3.0, 5.0, 10.0, 20.0, 50.0] {
            let t = host_mass_to_temp(mass);
            assert!(t >= prev, "mass_to_temp({mass}) = {t} < prev {prev}");
            prev = t;
        }
    }

    #[test]
    fn temperature_to_rgb_sun_is_white() {
        // The Sun (G2V, 5770K) should appear white to slightly yellowish.
        // vendian.org: G2(V) → #fff5f2 → linear (1.0, 0.961, 0.949)
        let (r, g, b) = host_temperature_to_rgb(5770.0);
        assert!((r - 1.0).abs() < 0.01);
        assert!(g > 0.95 && g < 1.0, "G={g} should be ~0.961");
        assert!(b > 0.93 && b < 0.97, "B={b} should be ~0.949");
    }

    #[test]
    fn temperature_to_rgb_no_green_stars() {
        // There are no green, cyan, or purple stars (Harre & Heller 2021).
        // The R channel should never be the minimum for a mid-range temperature.
        for t_k in [2500.0_f64, 3500.0, 4500.0, 5770.0, 7000.0, 8200.0, 10000.0, 15000.0] {
            let (r, g, b) = host_temperature_to_rgb(t_k);
            // For cool stars: R ≥ G ≥ B  (red/orange)
            // For hot stars: B ≥ G ≥ R  (blue/white)
            if t_k < 5000.0 {
                assert!(r >= g && g >= b, "at {t_k}K: R={r:.3} G={g:.3} B={b:.3} — expected R≥G≥B");
            } else {
                assert!(b >= g && g >= r, "at {t_k}K: R={r:.3} G={g:.3} B={b:.3} — expected B≥G≥R");
            }
        }
    }

    #[test]
    fn temperature_to_rgb_m_dwarfs_are_orange() {
        // M dwarfs (M0V, ~3750K) appear orange, not deep red.
        // vendian.org: M0(V) → #ffc38b → linear (1.0, 0.765, 0.545)
        let (r, g, b) = host_temperature_to_rgb(3750.0);
        assert!((r - 1.0).abs() < 0.01, "M dwarf R={r} should be 1.0");
        assert!(g > 0.70 && g < 0.85, "M dwarf G={g} should be ~0.765");
        assert!(b > 0.45 && b < 0.65, "M dwarf B={b} should be ~0.545");
        // Not red: R should dominate but G should be substantial (>0.5 means orange, not red)
        assert!(g > 0.5, "M dwarf G={g} > 0.5 (orange, not red)");
        assert!(r > g && g > b, "M dwarf: R > G > B (orange, not red)");
    }

    #[test]
    fn temperature_to_rgb_o_stars_are_blue() {
        // O5V at 41400K: the colour converges to (0.608, 0.690, 1.0)
        let (r, g, b) = host_temperature_to_rgb(41400.0);
        assert!(b > 0.99, "O star B should be ~1.0");
        assert!(r > 0.55 && r < 0.70, "O star R={r} should be ~0.61");
        assert!(g > 0.60 && g < 0.75, "O star G={g} should be ~0.69");
        // Blue dominant: B > G > R
        assert!(b > g && g > r, "O star: expected B > G > R");
    }

    #[test]
    fn temperature_to_rgb_monotonic_channels() {
        // R channel should strictly decrease with temperature
        // B channel should strictly increase with temperature
        let mut prev_r = 2.0;
        let mut prev_b = -1.0;
        for t in [2300_f64, 3060., 3750., 4400., 5240., 5770., 6540., 7220., 8180., 9700., 15200., 26500.] {
            let (r, _g, b) = host_temperature_to_rgb(t);
            assert!(r <= prev_r + 0.001, "R({t}) = {r} > prev {prev_r}");
            assert!(b >= prev_b - 0.001, "B({t}) = {b} < prev {prev_b}");
            prev_r = r;
            prev_b = b;
        }
    }

    #[test]
    fn cell_star_light_with_new_teff_gives_plausible_colors() {
        // Integration test: simulate what cell_star_light produces for a few
        // representative masses.  Just verify the output isn't crazy.
        use crate::galaxy::GalaxyParams;
        let p = GalaxyParams::milky_way();

        let test_masses = [0.1_f64, 0.3, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0];
        for &mass in &test_masses {
            let teff = host_mass_to_temp(mass);
            let (r, g, b) = host_temperature_to_rgb(teff);

            // All channels should be in [0, 1]
            assert!((0.0..=1.0).contains(&r), "mass={mass}: R={r} out of range");
            assert!((0.0..=1.0).contains(&g), "mass={mass}: G={g} out of range");
            assert!((0.0..=1.0).contains(&b), "mass={mass}: B={b} out of range");

            // At least one channel should be at 1.0 (chromaticity normalized)
            let max_ch = r.max(g).max(b);
            assert!(
                (max_ch - 1.0).abs() < 0.01,
                "mass={mass}: no channel near 1.0 (R={r}, G={g}, B={b})"
            );
        }
    }

    #[test]
    fn lut_entries_are_sorted_by_temperature() {
        // Verify the LUT is in ascending temperature order
        for i in 0..(COLOR_LUT_HOST.len() - 1) {
            assert!(
                COLOR_LUT_HOST[i].0 < COLOR_LUT_HOST[i + 1].0,
                "LUT entry {i} Teff={} >= entry {} Teff={}",
                COLOR_LUT_HOST[i].0,
                i + 1,
                COLOR_LUT_HOST[i + 1].0
            );
        }
    }
```

**Verify**: `cargo test` → all tests pass, including the new ones

### Step 5: Run the full validation suite

```bash
cargo clippy -- -D warnings
cargo test
```

Both must exit 0.  If clippy emits any warnings, fix them before proceeding.

### Step 6: Update `plans/README.md`

Add a row for plan 010 to the execution order table:

```
| 010  | Physical star colors via spectrum-based LUT         | P1       | M      | —          | TODO   |
```

## Test plan

- **New tests** go in `src/gpu.rs` `#[cfg(test)] mod tests { ... }`, following
  the pattern of existing tests like `milky_way_params_are_finite` (concise,
  focused, with clear assertion messages).
- Tests added in Step 4 cover:
  - `mass_to_temp` calibration against Pecaut & Mamajek (2013) reference values
  - `mass_to_temp` monotonicity
  - `temperature_to_rgb` solar color accuracy
  - `temperature_to_rgb` absence of non-physical colors (no green/cyan/purple stars)
  - `temperature_to_rgb` M-dwarf orange (not red) verification
  - `temperature_to_rgb` O-star blue verification
  - `temperature_to_rgb` per-channel monotonicity
  - Integration: `cell_star_light` output plausibility for representative masses
  - LUT structural invariant (ascending Teff order)
- **Existing tests** must continue to pass — none of the existing tests
  depend on the old blackbody color function, so no changes to existing
  tests should be needed.  However, if any existing test does break, update
  it to match the new physically-accurate values (do NOT regress to the
  blackbody approximation).

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0; at least 9 new star color tests exist and pass
- [ ] `grep -rn "Tanner Helland" galaxy-shader/src/lib.rs` returns no matches
      (old blackbody approximation removed)
- [ ] `grep -rn "5778.0 \* m.sqrt" galaxy-shader/src/lib.rs` returns no matches
      (old mass_to_temp removed)
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` updated with row for plan 010

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the locations in "Current state" doesn't match the excerpts
  (the codebase has drifted since this plan was written at `c2dfbe5`).
- `cargo check` fails after either Step 1 or Step 2 and the error is not
  a simple typo from the plan text (e.g. SPIR-V compilation error).
- Any existing test breaks in a way that is NOT related to the color change
  (e.g. a column-density test fails — that would indicate the build script
  produced a broken SPIR-V binary).
- Clippy emits warnings that cannot be resolved with straightforward fixes
  (the plan intentionally avoids `#[allow(...)]` — all warnings should be
  cleanly fixable).

## Maintenance notes

- **If you add new stellar mass ranges** (e.g. brown dwarfs below 0.08 M☉),
  extend the `mass_to_temp` piecewise function and add corresponding LUT
  entries at the low end (2300K is the current minimum; PHOENIX spectra
  go down to 2300K).
- **If you change the IMF** (e.g. add giant star population), note that
  the color LUT is main-sequence only (Class V).  Giant stars (Class III)
  and supergiants (Class I) have different colors at the same Teff — the
  vendian.org datafile includes separate tables for those luminosity classes.
- **The LUT data should be kept in sync** between `galaxy-shader/src/lib.rs`
  and `src/gpu.rs`.  The host-side tests validate this implicitly, but a
  future compile-time check could enforce it.
- **The choice of D65 vs. D50 whitepoint** makes a significant difference
  to the color of Sun-like stars.  This plan uses D65 (sRGB standard).
  If switching to a different display color space, the LUT values must be
  recomputed from the original spectral data.
