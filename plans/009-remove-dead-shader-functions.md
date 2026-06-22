# Plan 009: Remove dead shader functions

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6978e9e..HEAD -- galaxy-shader/src/lib.rs src/gpu.rs`
> If either file changed since this plan was written, compare the
> "Current state" excerpts below against the live code before proceeding;
> on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S (delete-only edits to one file)
- **Risk**: LOW (functions are already gated behind `#[allow(dead_code)]` — removing them can only reveal the `Real` import necessity)
- **Depends on**: none
- **Category**: tech-debt
- **Planned at**: commit `6978e9e`, 2026-06-22
- **Issue**: —

## Why this matters

The galaxy compute shader (`galaxy-shader/src/lib.rs`) defines six functions
that are never called: five are 3D density helpers (`density`, `disk_density`,
`arm_modulation`, `bulge_density`, `halo_density`) and one is a utility
(`smoothstep`).  These are vestiges from an earlier code path that computed 3D
density fields per star, before the column-density refactor.  Every function
is marked `#[allow(dead_code)]` so the compiler stays quiet, but they clutter
the file (~70 lines), mislead future readers looking for the active code path,
and accumulate drift — e.g. the dead `arm_modulation` still uses the old
Archimedean formula, which is inconsistent after plan 008 fixes the live
version.

Removing them makes the shader easier to read and ensures no one accidentally
calls a stale implementation.

## Current state

**File**: `galaxy-shader/src/lib.rs`

**Import (line 5)** — kept alive by the remaining code via method calls:

```rust
#[allow(unused)]
use spirv_std::num_traits::real::Real;
```

The `Real` trait provides `.powf()`, `.exp()`, `.ln()`, `.cosh()`, `.tan()`,
`.atan2()`, etc. on `f32` in a `no_std` SPIR-V context.  The compiler flags it
as unused because usage is implicit (method resolution), not explicit `Real::`
calls.  Even after dead-function removal, the remaining active code (`disk_column`,
`arm_modulation_2d`, `bulge_column`, `halo_column`, `hash3`, `sample_imf_from_cell`,
`temperature_to_rgb`, `mass_to_lum`, `mass_to_temp`, `sample_star_grid`,
`render_scene`, `cell_star_light`, `cell_has_star`, `smoothstep`) still uses
these methods.  We'll try to remove the `#[allow(unused)]` and the import;
if compilation fails, restore the import with a clarifying comment.

**Dead function 1 — `density` (lines 44–52)**:

```rust
#[allow(dead_code)]
fn density(pos: Vec3, p: &GalaxyUniform) -> f32 {
    let r = (pos.x * pos.x + pos.z * pos.z).sqrt();
    let z = pos.y.abs();

    disk_density(r, z, p) * arm_modulation(pos, r, p)
        + bulge_density(pos.length(), p)
        + halo_density(pos.length(), p)
}
```

**Dead function 2 — `disk_density` (lines 119–126)**:

```rust
#[allow(dead_code)]
fn disk_density(r: f32, z: f32, p: &GalaxyUniform) -> f32 {
    if p.disk_scale_length <= 0.0 || p.disk_scale_height <= 0.0 {
        return 0.0;
    }
    let radial = (-r / p.disk_scale_length).exp();
    let zeta = z / p.disk_scale_height;
    let sech = 1.0 / zeta.cosh();
    p.disk_central_density * radial * sech * sech
}
```

**Dead function 3 — `arm_modulation` 3D variant (lines 130–147)**:

```rust
#[allow(dead_code)]
fn arm_modulation(pos: Vec3, r: f32, p: &GalaxyUniform) -> f32 {
    if p.arm_count == 0 || p.arm_strength <= 0.0 {
        return 1.0;
    }
    let theta = pos.x.atan2(pos.z);
    let log_spiral = theta - (r / p.disk_scale_length) * p.arm_pitch;

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

**Dead function 4 — `bulge_density` (lines 152–158)**:

```rust
#[allow(dead_code)]
fn bulge_density(dist: f32, p: &GalaxyUniform) -> f32 {
    if p.bulge_radius <= 0.0 {
        return 0.0;
    }
    let x = dist / p.bulge_radius;
    p.bulge_central_density * (1.0 + x * x).powf(-2.5)
}
```

**Dead function 5 — `halo_density` (lines 161–167)**:

```rust
#[allow(dead_code)]
fn halo_density(dist: f32, p: &GalaxyUniform) -> f32 {
    if p.halo_radius <= 0.0 || dist < 1e-6 {
        return p.halo_central_density;
    }
    let x = dist / p.halo_radius;
    p.halo_central_density * (1.0 + x).powf(p.halo_slope)
}
```

**Dead function 6 — `smoothstep` (lines 279–285)**:

```rust
/// Smooth Hermite interpolation (same as GLSL smoothstep).
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
```

This function has no `#[allow(dead_code)]` — the compiler doesn't warn
because `main.rs` procedures (via `#[spirv(…)])` get special treatment.
Grepping the entire shader confirms `smoothstep` is never called:
`grep -n "smoothstep(" galaxy-shader/src/lib.rs` → only the definition
(and its doc comment).

## Commands you will need

| Purpose   | Command                    | Expected on success |
|-----------|----------------------------|---------------------|
| Check     | `cargo check`              | exit 0              |
| Clippy    | `cargo clippy -- -D warnings` | exit 0           |
| Test      | `cargo test`               | all 21 tests pass   |
| Build     | `cargo build`              | exit 0              |
| Force shader recompile | `cargo clean -p galaxy-shader && cargo check` | exit 0 |

## Scope

**In scope** (only file you may modify):

- `galaxy-shader/src/lib.rs` — delete the six dead functions

**Out of scope** (do NOT touch):

- `src/main.rs`, `src/gpu.rs`, `src/galaxy.rs`, `src/lib.rs`, `src/display.wgsl`
- `build.rs`, `Cargo.toml`
- `galaxies/ngc628.toml`
- The `GalaxyUniform` struct in the shader — it's used by `render_scene`
- The `#[allow(unused)]` on `use spirv_std::num_traits::real::Real` is
  **tentatively in scope** (Step 4) — try to clean it up, but keep it if the
  shader won't compile without it

## Conventions

- Shader functions are ordered by category: column density → stars → render
- Section comments like `// ═══════ Stars: hashing …` are kept
- `#[allow(clippy::manual_saturating_arithmetic)]` on `sample_star_grid` (line 340) is kept — it's live code
- Shader uses `spirv-std` intrinsic methods via the `Real` trait; don't remove the import unless the build proves it's genuinely unused

## Steps

### Step 1: Delete the six dead functions

Edit `galaxy-shader/src/lib.rs`:

#### Deletion 1: `density` function (lines 44–52)

Remove these lines entirely:

```
#[allow(dead_code)]
fn density(pos: Vec3, p: &GalaxyUniform) -> f32 {
    let r = (pos.x * pos.x + pos.z * pos.z).sqrt();
    let z = pos.y.abs();

    disk_density(r, z, p) * arm_modulation(pos, r, p)
        + bulge_density(pos.length(), p)
        + halo_density(pos.length(), p)
}

```

Keep the blank line and the section comment `// ── Column density …` that follows it — the section comment marks the start of the live `column_density` function (the active code path).

#### Deletion 2: `disk_density` (lines ~119–126)

```
#[allow(dead_code)]
fn disk_density(r: f32, z: f32, p: &GalaxyUniform) -> f32 {
    if p.disk_scale_length <= 0.0 || p.disk_scale_height <= 0.0 {
        return 0.0;
    }
    let radial = (-r / p.disk_scale_length).exp();
    let zeta = z / p.disk_scale_height;
    let sech = 1.0 / zeta.cosh();
    p.disk_central_density * radial * sech * sech
}

```

#### Deletion 3: `arm_modulation` 3D variant (lines ~130–147)

```
#[allow(dead_code)]
fn arm_modulation(pos: Vec3, r: f32, p: &GalaxyUniform) -> f32 {
    if p.arm_count == 0 || p.arm_strength <= 0.0 {
        return 1.0;
    }
    let theta = pos.x.atan2(pos.z);
    let log_spiral = theta - (r / p.disk_scale_length) * p.arm_pitch;

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

#### Deletion 4: `bulge_density` (lines ~152–158)

```
#[allow(dead_code)]
fn bulge_density(dist: f32, p: &GalaxyUniform) -> f32 {
    if p.bulge_radius <= 0.0 {
        return 0.0;
    }
    let x = dist / p.bulge_radius;
    p.bulge_central_density * (1.0 + x * x).powf(-2.5)
}

```

#### Deletion 5: `halo_density` (lines ~161–167)

```
#[allow(dead_code)]
fn halo_density(dist: f32, p: &GalaxyUniform) -> f32 {
    if p.halo_radius <= 0.0 || dist < 1e-6 {
        return p.halo_central_density;
    }
    let x = dist / p.halo_radius;
    p.halo_central_density * (1.0 + x).powf(p.halo_slope)
}

```

#### Deletion 6: `smoothstep` (lines ~279–285)

```
/// Smooth Hermite interpolation (same as GLSL smoothstep).
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

```

**Verify**: `cargo clean -p galaxy-shader && cargo check` → exit 0.

The shader must recompile — the dead functions aren't referenced by the entry
point, so compiling to SPIR-V will still succeed.  If the build fails with
errors about missing imports, that means a dead function was transitively
referenced — report it as a STOP condition.

### Step 2: Run the full verification suite

```bash
cargo clippy -- -D warnings
cargo test
cargo build
```

All three must exit 0.  `cargo test` must show exactly 21 tests passing
(no new tests added or removed by this plan).

### Step 3: Confirm no dead code remains in the shader

```bash
grep -n "#\[allow(dead_code)\]" galaxy-shader/src/lib.rs
```

Must return **no matches**.  If any `#[allow(dead_code)]` remains, it means
the corresponding function was not deleted — go back to Step 1.

Also confirm `smoothstep` is gone:

```bash
grep -n smoothstep galaxy-shader/src/lib.rs
```

Must return no matches.

### Step 4: Attempt to clean up the `Real` import

Try removing the `#[allow(unused)]` gate from the `Real` import:

```rust
// Old:
#[allow(unused)]
use spirv_std::num_traits::real::Real;

// Attempt 1: remove the attribute
use spirv_std::num_traits::real::Real;
```

Run `cargo clean -p galaxy-shader && cargo check`.

- **If it compiles**: keep the clean import.  Done.
- **If it produces an "unused import" warning/error**: try removing the
  import entirely: delete the `use spirv_std::num_traits::real::Real;` line.
  Run `cargo clean -p galaxy-shader && cargo check`.
  - **If it compiles**: the trait isn't needed.  Done.
  - **If it fails** (e.g. `no method named 'exp' found for type 'f32'`):
    restore the import with a clarifying comment:

```rust
// Real trait provides f32 methods (exp, ln, powf, …) in no_std SPIR-V context.
// The compiler sees no explicit `Real::` calls but the methods are used
// implicitly via UFCS resolution.
#[allow(unused)]
use spirv_std::num_traits::real::Real;
```

Do NOT spend more than 2 iterations on this step.  Keeping the import with
`#[allow(unused)]` is harmless and a known pattern in spirv-std shaders.

**Verify**: `cargo clean -p galaxy-shader && cargo check && cargo clippy -- -D warnings` → all exit 0.

## Test plan

No new tests — this is purely a deletion.  All 21 existing tests must
continue to pass.  The tests in `src/gpu.rs` test host-side replicas
of column density functions (disk, bulge, halo), none of which are
being deleted.

Verification: `cargo test` → 21 passed.

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0; 21 tests pass
- [ ] `cargo build` exits 0
- [ ] `grep -n "#\[allow(dead_code)\]" galaxy-shader/src/lib.rs` returns no matches
- [ ] `grep -n smoothstep galaxy-shader/src/lib.rs` returns no matches
- [ ] `grep -n "fn density\|fn disk_density\|fn arm_modulation\|fn bulge_density\|fn halo_density" galaxy-shader/src/lib.rs` returns no dead-function matches (safe to run — the active `fn column_density` does NOT match the grep patterns)
- [ ] No files outside the in-scope list are modified (`git diff --stat`)
- [ ] The shader still compiles to SPIR-V and the app launches (`cargo run` for a visual smoke)

## STOP conditions

Stop and report back if:

- The code at the locations in "Current state" doesn't match the excerpts
  (codebase drifted since `6978e9e`).
- Deleting any dead function causes a compilation error in remaining live code
  (e.g., a function was transitively called by a live path — re-check with
  `cargo clean -p galaxy-shader && cargo check --target spirv-unknown-vulkan1.2`).
- `cargo clippy -- -D warnings` produces a NEW warning that wasn't present
  before the deletions (the import cleanup in Step 4 may surface one — resolve
  or revert the import stanza).
- A step's verification fails twice after a reasonable fix attempt.
- Running the app after deletions produces different galaxy output from before
  (this would indicate a live function was incorrectly identified as dead).

## Maintenance notes

- If 3D rendering is ever added back (e.g., for edge-on views with proper
  vertical integration), the 3D density functions would need to be rewritten
  anyway — the deleted functions used the old Archimedean arm formula.
- The deleted `smoothstep` was a standard GLSL utility.  If future shader
  code needs smoothstep, it's a 3-line function — copy it from the WebGPU
  reference or re-add the same lines.
- The `#[allow(unused)]` on `Real` is fragile across spirv-std updates.  If
  the trait is renamed or re-exported in a future spirv-std version, the
  import will need a matching update.
