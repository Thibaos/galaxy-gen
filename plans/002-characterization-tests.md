# Plan 002: Add characterization tests for math-heavy code

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 5a0cdfc..HEAD -- src/galaxy.rs src/gpu.rs src/main.rs Cargo.toml`
> If any of these files changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Category**: tests
- **Depends on**: 001 (tests validate the refactored gpu module)
- **Planned at**: commit `5a0cdfc`, 2026-06-14
- **Issue**: —

## Why this matters

The project's core value is the galaxy image it produces. The column-density integrations (`disk_column`, `bulge_column`, `halo_column`) in the shader are closed-form analytic approximations — one sign error in the power-law exponent or one wrong constant produces a subtly wrong image that a human won't notice but that compounds into incorrect scientific-looking output. The `GalaxyUniform::from_params` conversion (f64→f32 casting, field mapping) has no validation. The tone-mapping math in `render_scene` has no correctness check. Zero tests exist anywhere in the project, so any refactoring (including plan 001) operates blind. These characterization tests create a safety net.

## Current state

The project has no tests at all. The files and functions that need coverage:

- `src/galaxy.rs` — `GalaxyParams` struct and `milky_way()` constructor. Fields are `f64` with no clamping or validation. The GPU shader receives `f32` values; extreme or NaN values could produce undefined shader behavior.
- `src/gpu.rs` — `GalaxyUniform::from_params()` converts f64→f32 and maps 12 fields. A field swap or missing field would compile but produce wrong output.
- `galaxy-shader/src/lib.rs` — The shader functions (`disk_column`, `bulge_column`, `halo_column`, `arm_modulation_2d`, `density`, `column_density`) are pure math but only compilable for `spirv-unknown-vulkan1.2`. They cannot be called directly from host tests. The strategy is to **duplicate the math in a host-side test module** and assert known-good values. This is a characterization test: it pins current behavior so regressions are caught.

The repo has no test conventions to follow; we'll establish them here. The standard Rust pattern is `#[cfg(test)] mod tests { ... }` at the bottom of each source file.

## Commands you will need

| Purpose   | Command               | Expected on success |
|-----------|-----------------------|---------------------|
| Check     | `cargo check`         | exit 0              |
| Clippy    | `cargo clippy`        | exit 0              |
| Test      | `cargo test`          | all tests pass      |

## Scope

**In scope** (files you may modify):
- `src/galaxy.rs` — add `#[cfg(test)] mod tests` with parameter validation tests
- `src/gpu.rs` — add `#[cfg(test)] mod tests` with `GalaxyUniform::from_params` correctness tests
- `Cargo.toml` — may need a `[dev-dependencies]` entry if any test-only crate is needed (unlikely; `f64` math needs no deps)

**Out of scope** (do NOT touch):
- `galaxy-shader/src/lib.rs` — the shader code itself; tests live host-side
- `src/main.rs` — no testable logic beyond what's already covered via galaxy.rs and gpu.rs
- Any actual rendering or GPU code — unit tests only, no GPU integration tests
- The shader build process (build.rs) — unchanged

## Git workflow

- Branch: `advisor/002-characterization-tests`
- Commit per step; message style: imperative, lowercase, e.g. `add GalaxyParams validation tests`
- Do NOT push or open a PR unless instructed

## Conventions

This plan establishes the test conventions for the repo:
- Tests live in `#[cfg(test)] mod tests { use super::*; ... }` at the bottom of each `.rs` file
- Use plain `assert!`, `assert_eq!`, and `assert!((a - b).abs() < epsilon)` for float comparisons
- Test function names describe the scenario: `fn milky_way_params_are_finite()`
- No external test frameworks — plain `cargo test`

## Steps

### Step 1: Add `GalaxyParams` validation tests in `src/galaxy.rs`

At the bottom of `src/galaxy.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milky_way_params_are_finite() {
        let p = GalaxyParams::milky_way();
        assert!(p.disk_scale_length.is_finite() && p.disk_scale_length > 0.0);
        assert!(p.disk_scale_height.is_finite() && p.disk_scale_height > 0.0);
        assert!(p.disk_central_density.is_finite() && p.disk_central_density > 0.0);
        assert!(p.arm_count > 0);
        assert!(p.arm_pitch.is_finite());
        assert!(p.arm_concentration.is_finite() && p.arm_concentration > 0.0);
        assert!(p.arm_strength.is_finite());
        assert!(p.bulge_radius.is_finite() && p.bulge_radius > 0.0);
        assert!(p.bulge_central_density.is_finite() && p.bulge_central_density > 0.0);
        assert!(p.halo_radius.is_finite() && p.halo_radius > 0.0);
        assert!(p.halo_central_density.is_finite());
        assert!(p.halo_slope.is_finite() && p.halo_slope < 0.0);
    }

    #[test]
    fn milky_way_params_clone_is_equal() {
        let p1 = GalaxyParams::milky_way();
        let p2 = p1.clone();
        // Compare fields individually since we didn't derive PartialEq
        assert_eq!(p1.disk_scale_length.to_bits(), p2.disk_scale_length.to_bits());
        assert_eq!(p1.disk_central_density.to_bits(), p2.disk_central_density.to_bits());
        assert_eq!(p1.arm_count, p2.arm_count);
        assert_eq!(p1.arm_pitch.to_bits(), p2.arm_pitch.to_bits());
        assert_eq!(p1.bulge_radius.to_bits(), p2.bulge_radius.to_bits());
        assert_eq!(p1.halo_slope.to_bits(), p2.halo_slope.to_bits());
    }

    #[test]
    fn milky_way_params_cast_to_f32_is_finite() {
        let p = GalaxyParams::milky_way();
        // Verify all fields can be cast to f32 without overflow/inf
        assert!((p.disk_scale_length as f32).is_finite());
        assert!((p.disk_scale_height as f32).is_finite());
        assert!((p.disk_central_density as f32).is_finite());
        assert!((p.arm_pitch as f32).is_finite());
        assert!((p.arm_concentration as f32).is_finite());
        assert!((p.arm_strength as f32).is_finite());
        assert!((p.bulge_radius as f32).is_finite());
        assert!((p.bulge_central_density as f32).is_finite());
        assert!((p.halo_radius as f32).is_finite());
        assert!((p.halo_central_density as f32).is_finite());
        // halo_slope is negative; casting to f32 is fine
        assert!((p.halo_slope as f32).is_finite());
    }
}
```

**Verify**: `cargo test` → exit 0, 3 tests pass

### Step 2: Add `GalaxyUniform::from_params` correctness tests in `src/gpu.rs`

At the bottom of `src/gpu.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::galaxy::GalaxyParams;

    #[test]
    fn from_params_preserves_values() {
        let params = GalaxyParams::milky_way();
        let uniform = GalaxyUniform::from_params(
            &params, 1920, 1080, 512_000.0, 0.0, 0.0, 0.60, 0.04,
        );
        // Field-by-field assertions to catch field-order bugs
        assert_eq!(uniform.disk_scale_length, params.disk_scale_length as f32);
        assert_eq!(uniform.disk_scale_height, params.disk_scale_height as f32);
        assert_eq!(uniform.disk_central_density, params.disk_central_density as f32);
        assert_eq!(uniform.arm_count, params.arm_count);
        assert_eq!(uniform.arm_pitch, params.arm_pitch as f32);
        assert_eq!(uniform.arm_concentration, params.arm_concentration as f32);
        assert_eq!(uniform.arm_strength, params.arm_strength as f32);
        assert_eq!(uniform.bulge_radius, params.bulge_radius as f32);
        assert_eq!(uniform.bulge_central_density, params.bulge_central_density as f32);
        assert_eq!(uniform.halo_radius, params.halo_radius as f32);
        assert_eq!(uniform.halo_central_density, params.halo_central_density as f32);
        assert_eq!(uniform.halo_slope, params.halo_slope as f32);
        assert_eq!(uniform.image_width, 1920);
        assert_eq!(uniform.image_height, 1080);
        assert_eq!(uniform.extent, 512_000.0_f32);
        assert_eq!(uniform.center_x, 0.0_f32);
        assert_eq!(uniform.center_y, 0.0_f32);
        assert_eq!(uniform.exposure, 0.60);
        assert_eq!(uniform.log_contrast, 0.04);
    }

    #[test]
    fn from_params_with_nonzero_center() {
        let params = GalaxyParams::milky_way();
        let uniform = GalaxyUniform::from_params(
            &params, 800, 600, 100_000.0, 5000.0, -2000.0, 0.50, 0.02,
        );
        assert_eq!(uniform.center_x, 5000.0_f32);
        assert_eq!(uniform.center_y, -2000.0_f32);
        assert_eq!(uniform.extent, 100_000.0_f32);
        assert_eq!(uniform.image_width, 800);
        assert_eq!(uniform.image_height, 600);
        assert_eq!(uniform.exposure, 0.50);
        assert_eq!(uniform.log_contrast, 0.02);
    }

    #[test]
    fn uniform_is_pod() {
        // Verify bytemuck traits work
        let params = GalaxyParams::milky_way();
        let uniform = GalaxyUniform::from_params(
            &params, 100, 100, 1000.0, 0.0, 0.0, 0.5, 0.05,
        );
        let _bytes: &[u8] = bytemuck::bytes_of(&uniform);
        // If this compiles and doesn't panic, Pod+Zeroable is satisfied
    }

    #[test]
    fn uniform_struct_size_matches_fields() {
        // Runtime size check as a cross-check against shader-side layout mismatch
        let expected = std::mem::size_of::<f32>() * 16 + std::mem::size_of::<u32>() * 3;
        assert_eq!(std::mem::size_of::<GalaxyUniform>(), expected);
    }
}
```

**Verify**: `cargo test` → exit 0, all 7 tests pass (3 from step 1 + 4 new)

### Step 3: Add column-density math characterization tests

Since the shader functions (`disk_column`, `bulge_column`, `halo_column`) compile only for SPIR-V, we write equivalent host-side Rust functions in `src/gpu.rs`'s test module and verify they produce reasonable values. These are **characterization tests** — they pin the current math behavior.

Add to the test module in `src/gpu.rs`:

```rust
    // ── Host-side replicas of shader math for characterization ──

    fn host_disk_column(r: f64, p: &GalaxyParams) -> f64 {
        if p.disk_scale_length <= 0.0 || p.disk_scale_height <= 0.0 {
            return 0.0;
        }
        let radial = (-r / p.disk_scale_length).exp();
        2.0 * p.disk_scale_height * p.disk_central_density * radial
    }

    fn host_bulge_column(r: f64, p: &GalaxyParams) -> f64 {
        if p.bulge_radius <= 0.0 {
            return 0.0;
        }
        let x = r / p.bulge_radius;
        (4.0 / 3.0) * p.bulge_radius * p.bulge_central_density * (1.0 + x * x).powf(-2.0)
    }

    fn host_halo_column(r: f64, p: &GalaxyParams) -> f64 {
        if p.halo_radius <= 0.0 || r < 1e-6 {
            return p.halo_radius * p.halo_central_density * std::f64::consts::PI;
        }
        let x = r / p.halo_radius;
        std::f64::consts::PI * p.halo_radius * p.halo_central_density * (1.0 + x).powf(p.halo_slope + 1.0)
    }

    #[test]
    fn disk_column_decreases_with_radius() {
        let p = GalaxyParams::milky_way();
        let d0 = host_disk_column(0.0, &p);
        let d1 = host_disk_column(p.disk_scale_length, &p);
        let d2 = host_disk_column(3.0 * p.disk_scale_length, &p);
        assert!(d0 > 0.0, "disk column at r=0 should be positive");
        assert!(d1 < d0, "disk should decrease with radius");
        assert!(d2 < d1, "disk should continue decreasing");
        // Exponential drop: d1/d0 ≈ 1/e
        let ratio = d1 / d0;
        assert!((ratio - 1.0 / std::f64::consts::E).abs() < 0.01,
            "d(scale_length)/d(0) should be ~1/e, got {ratio}");
    }

    #[test]
    fn bulge_column_has_plummer_profile() {
        let p = GalaxyParams::milky_way();
        let d0 = host_bulge_column(0.0, &p);
        let d_r = host_bulge_column(p.bulge_radius, &p);
        assert!(d0 > 0.0, "bulge column at center should be positive");
        // Plummer: Σ(R) ∝ (1 + R²/a²)^(-2). At R=a, factor is (1+1)^(-2) = 1/4
        let expected_ratio = 0.25;
        let actual_ratio = d_r / d0;
        assert!((actual_ratio - expected_ratio).abs() < 0.01,
            "bulge at R=a should be ~1/4 of central, got {actual_ratio}");
    }

    #[test]
    fn halo_column_is_positive_and_finite() {
        let p = GalaxyParams::milky_way();
        let values: Vec<f64> = [0.0, 1000.0, 10000.0, 50000.0, 100000.0]
            .iter()
            .map(|&r| host_halo_column(r, &p))
            .collect();
        for (r, v) in [0.0_f64, 1000.0, 10000.0, 50000.0, 100000.0].iter().zip(&values) {
            assert!(v.is_finite(), "halo_column({r}) = {v} is not finite");
            assert!(*v >= 0.0, "halo_column({r}) = {v} is negative");
        }
        // Halo should decrease with radius
        assert!(values[0] >= values[1], "halo should decrease with radius");
        assert!(values[1] > values[3], "halo should continue decreasing");
    }

    #[test]
    fn zero_params_produce_zero_density() {
        let zero_params = GalaxyParams {
            disk_scale_length: 0.0,
            disk_scale_height: 0.0,
            disk_central_density: 0.0,
            arm_count: 0,
            arm_pitch: 0.0,
            arm_concentration: 0.0,
            arm_strength: 0.0,
            bulge_radius: 0.0,
            bulge_central_density: 0.0,
            halo_radius: 0.0,
            halo_central_density: 0.0,
            halo_slope: -3.0,
        };
        assert_eq!(host_disk_column(1000.0, &zero_params), 0.0);
        assert_eq!(host_bulge_column(1000.0, &zero_params), 0.0);
        // halo_radius=0 returns the center value regardless of r
    }
```

**Note**: `GalaxyParams` doesn't derive `PartialEq`. The `zero_params` construction above uses struct literal syntax — this is fine since all fields are `pub`. If the struct changes to have private fields or a constructor requirement, you'll need to add a test-only constructor or derive `Default`.

**Verify**: `cargo test` → exit 0, all 11 tests pass (3 + 4 + 4 new)

### Step 4: Run full test suite

**Verify**: `cargo test` → exit 0, 11 tests pass, no warnings

## Test plan

All tests described above are the plan's tests. They cover:
- GalaxyParams validity (finiteness, positive-critical values, f32-castability)
- GalaxyUniform field mapping correctness (field-by-field comparison, size check)
- Column density math behavior (monotonicity, known analytical ratios, zero-params edge case)

## Done criteria

- [ ] `cargo test` exits 0 with 11 passing tests
- [ ] `cargo clippy` exits 0 (pre-existing warnings acceptable; no new warnings)
- [ ] `src/galaxy.rs` has a `#[cfg(test)] mod tests` with at least 3 tests
- [ ] `src/gpu.rs` has a `#[cfg(test)] mod tests` with at least 8 tests
- [ ] No files outside the in-scope list are modified
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back if:

- The code at the locations in "Current state" doesn't match the excerpts.
- A test assertion fails and the math in the shader appears to be the bug — do NOT "fix" the shader to make the test pass; report the discrepancy.
- `cargo test` fails for any reason that isn't a simple typo.
- `GalaxyParams` gains a non-`pub` field or a constructor that prevents struct-literal construction (the `zero_params` test will break).

## Maintenance notes

- The host-side replicas of shader math (`host_disk_column` etc.) must be kept in sync with `galaxy-shader/src/lib.rs`. If the shader's column density formulas change, these tests must be updated identically. A future improvement (outside scope) would be to extract these functions into a shared no_std crate both sides can use.
- If `GalaxyParams` fields become non-`pub`, the `zero_params` test needs a test-only constructor.
- These are characterization tests, not correctness proofs — they validate the code matches itself and doesn't regress, but they don't prove the physics is right.
