# Plan 018: Add `generate_star_catalogue` characterization tests

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat HEAD~1..HEAD -- src/gpu.rs`
> If `generate_star_catalogue`, `star_cell_host`, `hash3_host`,
> `arm_modulation_host`, or `StarInstance` changed since this plan was
> written, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
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

### Step 1: Add test module

In `src/gpu.rs`, at the bottom of the existing `mod tests` block (after all
existing tests), add a new module or extend with the 7 tests above.

Helper functions already exist in the test module (`host_mass_to_temp`,
`host_temperature_to_rgb`, host-side column density functions from
`galaxy.rs` tests). Reuse them or add thin wrappers.

### Step 2: Implement all 7 tests

Write each test as described above. Start with test 4 (determinism) and
test 7 (no NaN) as they're the simplest and catch the most common bugs.

### Step 3: Validate

```bash
cargo test generate_star_catalogue
cargo test  # full suite
cargo clippy -- -D warnings
```

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

- `generate_star_catalogue` signature changed since plan was written
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
