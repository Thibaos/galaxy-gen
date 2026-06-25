# Plan 018: Add `generate_star_catalogue` characterization tests

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat 1647567..HEAD -- src/gpu.rs`
> If `generate_star_catalogue`, `star_cell_host`, `hash3_host`,
> `arm_modulation_host`, or `StarInstance` changed since this plan was
> written, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Category**: tests
- **Depends on**: none
- **Planned at**: commit `1647567`, 2026-06-25

## Why this matters

`generate_star_catalogue` is a 100-line critical-path function that
determines the star distribution for the instanced rendering pass. It
involves:

- IMF sampling via `star_cell_host` (mass, temperature, luminosity)
- Column density integration (`disk_column`, `bulge_column`, `halo_column`)
- Spiral arm modulation (`arm_modulation_host`)
- A-Res weighted random sampling (key = -ln(random) / column_density)
- Sort + truncate

This function has ZERO test coverage. A regression here silently corrupts
the star field shape — stars would be uniformly distributed or biased to
wrong regions, which is hard to spot unless you compare screenshots.

The function is deterministic (all randomness comes from PRNG hashes), so
characterization tests are feasible.

## Current state

**Relevant code**: `src/gpu.rs:322-403`:

```rust
pub fn generate_star_catalogue(params: &GalaxyParams, max_stars: u32) -> Vec<StarInstance> {
    let disc_radius = (8.0 * params.disk_scale_length).clamp(50_000.0, 80_000.0);
    let cell_size = 50.0;
    let cells = (disc_radius / cell_size).ceil() as i32;

    struct Candidate { key: f64, star: StarInstance }

    let mut candidates: Vec<Candidate> = Vec::with_capacity((cells as usize * 2).pow(2));
    for ix in -cells..=cells {
        let wx = ix as f64 * cell_size;
        for iz in -cells..=cells {
            let wz = iz as f64 * cell_size;
            let r = (wx * wx + wz * wz).sqrt();
            if r >= disc_radius { continue; }

            // ... density calculation ...
            // ... arm modulation ...
            // ... IMF mass/lum/temp ...
            // ... weighted key ...

            candidates.push(Candidate { key, star });
        }
    }

    candidates.sort_unstable_by(|a, b| a.key.partial_cmp(&b.key).unwrap());
    candidates.truncate(max_stars as usize);
    candidates.into_iter().map(|c| c.star).collect()
}
```

Helper functions to replicate in test harness:

- `hash3_host` (line 168) — deterministic PCG hash
- `star_cell_host` (line 177) — IMF mass from cell coords
- `arm_modulation_host` (line 197) — logarithmic spiral formula
- `disk_column_host`, `bulge_column_host`, `halo_column_host` — column
  density helpers (already exist in `galaxy.rs` tests but with f64 API)

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**
- `src/gpu.rs` — add test module with characterization tests for
  `generate_star_catalogue`
- Test data: use `GalaxyParams::milky_way()` and `GalaxyParams::ngc628()`

**Out of scope:**
- Changing `generate_star_catalogue` implementation
- Integration tests (GPU-requiring)
- Fuzzing

## Test cases

### Test 1: Star catalogue is non-empty and within bounds

```
catalogue = generate_star_catalogue(&milky_way(), 1000)
assert catalogue.len() == 1000
for star in catalogue:
    assert star.pos_x.finite() && star.pos_z.finite()
    assert star.pos_y.abs() < 2000  // within disk scale height neighborhood
    assert star.mass > 0.08 && star.mass < 100
    assert star.temp >= 2300 && star.temp <= 50000
    assert star.lum > 0
```

### Test 2: Star positions span the full disc

```
catalogue = generate_star_catalogue(&milky_way(), 10000)
x_min = min(star.pos_x for star in catalogue)
x_max = max(star.pos_x for star in catalogue)
z_min = min(star.pos_z for star in catalogue)
z_max = max(star.pos_z for star in catalogue)

assert x_max - x_min > 40000  // at least 40k LY span
assert z_max - z_min > 40000
assert x_min < -20000 && x_max > 20000  // crosses origin
assert z_min < -20000 && z_max > 20000
```

### Test 3: Bulge is enriched relative to outer disc

Compare star density in central 5k LY vs outer ring (40k-45k LY):

```
centre_count = count stars with (pos_x² + pos_z²) < 5000²
outer_count  = count stars with 40000 < sqrt(pos_x² + pos_z²) < 45000

centre_density = centre_count / π·5000²
outer_density  = outer_count / (π·45000² − π·40000²)

ratio = centre_density / outer_density
assert ratio > 10  // bulge is at least 10× denser than outer disc
```

### Test 4: Star catalogue is deterministic

```
a = generate_star_catalogue(&milky_way(), 500)
b = generate_star_catalogue(&milky_way(), 500)
assert a == b  // same seed, same params → identical output
```

### Test 5: Different presets produce different catalogues

```
mw   = generate_star_catalogue(&milky_way(), 500)
ngc  = generate_star_catalogue(&ngc628(), 500)
assert mw != ngc  // physically different galaxies
```

### Test 6: NGC 628 is more extended than Milky Way

NGC 628 has larger disk scale length (~6.8 kLY vs ~3.5 kLY), so its star
catalogue should extend further:

```
mw   = generate_star_catalogue(&milky_way(), 5000)
ngc  = generate_star_catalogue(&ngc628(), 5000)

mw_extent  = max(sqrt(pos_x² + pos_z²) for star in mw)
ngc_extent = max(sqrt(pos_x² + pos_z²) for star in ngc)
assert ngc_extent > mw_extent
```

### Test 7: No NaN or infinite values

```
catalogue = generate_star_catalogue(&milky_way(), 1000)
for star in catalogue:
    assert star.pos_x.is_finite()
    assert star.pos_y.is_finite()
    assert star.pos_z.is_finite()
    assert star.mass.is_finite()
    assert star.temp.is_finite()
    assert star.lum.is_finite()
```

## Steps

### Step 1: Locate the test module

In `src/gpu.rs`, find the existing `#[cfg(test)] mod tests { ... }` block
at the bottom of the file. This is where all new tests go. The module
currently contains tests for `host_mass_to_temp`, `host_temperature_to_rgb`,
column density host helpers (imported from `galaxy.rs` tests), and uniform
struct layout/size checks.

Available helpers you can reuse:
- `host_mass_to_temp(mass)` — maps IMF mass to temperature (K)
- `host_temperature_to_rgb(temp)` — maps temperature to RGB via the LUT
- `super::hash3_host(x, y, seed)` — deterministic PCG hash
- `super::star_cell_host(cx, cy, cz, seed)` — IMF star from cell coords
- `super::arm_modulation_host(x, z, params)` — spiral arm amplitude
- `super::generate_star_catalogue(params, max_stars)` — the function under test

**Verify**: `cargo check` → exit 0 (confirm test module is found)

### Step 2: Implement tests 4 and 7 first (safety + determinism)

Test 4 (determinism) and test 7 (no NaN) are the simplest and catch the
most common regressions. Write them first:

```rust
#[test]
fn star_catalogue_deterministic() {
    let params = GalaxyParams::milky_way();
    let a = generate_star_catalogue(&params, 500);
    let b = generate_star_catalogue(&params, 500);
    assert_eq!(a, b);
}

#[test]
fn star_catalogue_no_nan_or_infinite() {
    let catalogue = generate_star_catalogue(&GalaxyParams::milky_way(), 1000);
    for star in &catalogue {
        assert!(star.pos_x.is_finite());
        assert!(star.pos_y.is_finite());
        assert!(star.pos_z.is_finite());
        assert!(star.mass.is_finite());
        assert!(star.temp.is_finite());
        assert!(star.lum.is_finite());
    }
}
```

**Verify**: `cargo test star_catalogue_deterministic star_catalogue_no_nan` → both pass

### Step 3: Implement remaining 5 tests

Write tests 1, 2, 3, 5, 6. Test 3 (bulge enrichment) is the most complex —
here is the Rust implementation:

```rust
#[test]
fn star_catalogue_bulge_enriched_over_outer_disc() {
    let params = GalaxyParams::milky_way();
    let catalogue = generate_star_catalogue(&params, 10_000);
    use std::f64::consts::PI;
    let centre_count = catalogue.iter()
        .filter(|s| s.pos_x.powi(2) + s.pos_z.powi(2) < 5000.0_f64.powi(2))
        .count() as f64;
    let outer_count = catalogue.iter()
        .filter(|s| {
            let r = (s.pos_x.powi(2) + s.pos_z.powi(2)).sqrt();
            r > 40_000.0 && r < 45_000.0
        })
        .count() as f64;
    let centre_density = centre_count / (PI * 5000.0_f64.powi(2));
    let outer_area = PI * (45_000.0_f64.powi(2) - 40_000.0_f64.powi(2));
    let outer_density = outer_count / outer_area;
    let ratio = centre_density / outer_density;
    assert!(ratio > 10.0, "bulge enrichment ratio {} should exceed 10", ratio);
}
```

Tests 1, 2, 5, 6 follow simpler patterns (filter, count, assert). Use
`max_stars=5000` for spatial tests; `max_stars=1000` for faster ones.

**Verify**: `cargo test` → all new tests pass (≥52 total)

### Step 4: Full validation

```bash
cargo clippy -- -D warnings
cargo test  # must complete in < 10 seconds (debug mode)
```

## Git workflow

- Branch: `advisor/018-star-catalogue-tests`
- Commit message: `test: add generate_star_catalogue characterization tests`
- Do NOT push or open a PR unless instructed.

## Test plan

- All 45 existing tests + 7 new tests pass
- New tests cover: bounds, spatial extent, density gradient, determinism,
  preset differentiation, NGC 628 extent, and NaN safety

## Done criteria

- [ ] 7 new `generate_star_catalogue` tests pass
- [ ] `cargo test` exits 0 (≥52 tests total)
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] Each test has a doc comment explaining what it validates
- [ ] `plans/README.md` updated

## STOP conditions

- The code at the locations in "Current state" doesn't match the excerpts
  (the codebase has drifted since this plan was written).
- Determinism test fails (PRNG logic changed)
- Any existing test fails
- Test run takes > 10 seconds (weighted sort of large Vec may be slow in
  debug mode — consider using smaller max_stars values in tests)

## Maintenance notes

- The determinism test (test 4) will break if the hash seed or IMF
  constants change. This is intentional — it catches silent regressions.
- Test 3 (bulge enrichment ratio > 10) uses a conservative threshold.
  The actual ratio for Milky Way params is ~50-100. If the weighted
  sampling algorithm changes, relax this threshold rather than removing it.
