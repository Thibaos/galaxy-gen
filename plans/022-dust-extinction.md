# Plan 022: Dust extinction in the ray-march path

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat HEAD~1..HEAD -- galaxy-shader/src/lib.rs`
> If `ray_march_galaxy` or the density functions changed since this plan was
> written, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
- **Category**: direction (realism)
- **Depends on**: none
- **Planned at**: commit `1647567`, 2026-06-25

## Why this matters

The current ray-march path emits identical warm-white light at every
position regardless of depth into the galaxy. Real galaxies show significant
dust extinction near the midplane — the inner disk appears reddened because
blue light is preferentially scattered and absorbed by interstellar dust
grains.

Adding a simple extinction term makes the galaxy feel deeper: near-edge-on
views show a dark dust lane bisecting the disk; face-on views show the
inner regions tinted slightly warmer/orange, with the outer disk remaining
bluer.

This is a high-impact visual improvement for minimal code change (~10 lines
in the shader).

## Design

Model extinction as: **T(λ) = exp(−τ · A_λ / A_V · column_density / column_0)**

Where:
- `τ` is a tunable optical depth at visual wavelengths (~0.1–0.5)
- `A_λ / A_V` is the wavelength-dependent extinction (Cardelli+1989 law)
- `column_density / column_0` is the normalized dust column along the
  line of sight

For simplicity, pre-compute three extinction coefficients (one per RGB
channel) based on the standard RV=3.1 extinction curve:

```
A_R/A_V ≈ 0.75   (red least affected)
A_G/A_V ≈ 1.0    (green reference)
A_B/A_V ≈ 1.32   (blue most affected)
```

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**
- `galaxy-shader/src/lib.rs` — add extinction constants and apply them in
  `ray_march_galaxy`; add `dust_tau` field to `GalaxyUniform`
- `src/gpu.rs` — add `dust_tau: f32` to host `GalaxyUniform`, update
  `from_params`, update offset/size tests
- `src/main.rs` — add `dust_tau` slider to egui 3D panel (0.0–1.0, default 0.2);
  pass it through `from_params`

## Steps

### Step 1: Add dust extinction constants to shader

In `galaxy-shader/src/lib.rs`, add near the other constants:

```rust
/// Extinction per RGB channel relative to visual (A_λ / A_V).
/// Based on Cardelli, Clayton & Mathis (1989) for R_V = 3.1.
/// Blue is most extinguished (A_B ≈ 1.32 A_V), red least (A_R ≈ 0.75 A_V).
const DUST_EXTINCTION_R: f32 = 0.75;
const DUST_EXTINCTION_G: f32 = 1.00;
const DUST_EXTINCTION_B: f32 = 1.32;
```

### Step 2: Add `dust_tau` to GalaxyUniform

In both `galaxy-shader/src/lib.rs` and `src/gpu.rs`, add to the struct:

```rust
pub dust_tau: f32,  // optical depth multiplier for dust extinction
```

Place it after `fov_y_deg` in both halves. Update the offset and size tests.

### Step 3: Apply extinction in ray_march_galaxy

In `ray_march_galaxy`, after accumulating emissivity for each step, apply
extinction based on the integrated dust column along the ray so far.

Add a running extinction accumulator (three channels):

```rust
let mut dust_r = 0.0_f32;
let mut dust_g = 0.0_f32;
let mut dust_b = 0.0_f32;
```

After each density sample, accumulate extinction:

```rust
// Dust extinction accumulates with density × path length
let dust_dtau = dens * dt * params.dust_tau;
dust_r += dust_dtau * DUST_EXTINCTION_R;
dust_g += dust_dtau * DUST_EXTINCTION_G;
dust_b += dust_dtau * DUST_EXTINCTION_B;

// Emitted light is extinguished by the dust between the emission
// point and the camera.  Apply extinction BEFORE adding to accumulator.
let extinction_r = (-dust_r).exp();
let extinction_g = (-dust_g).exp();
let extinction_b = (-dust_b).exp();
```

The accumulated emissivity at each step becomes:

```rust
acc.x += dens * dt * EMISSIVITY * extinction_r;
acc.y += dens * dt * EMISSIVITY * 0.9 * extinction_g;
acc.z += dens * dt * EMISSIVITY * 0.7 * extinction_b;
```

This "extinguish before add" model means light emitted at the far side
of the galaxy passes through all the intervening dust and emerges redder.

### Step 4: Add slider in egui

In the 3D View section of the egui sidebar:

```rust
ui.add(egui::Slider::new(&mut self.dust_tau, 0.0..=1.0).text("Dust τ"));
```

Initialize `dust_tau = 0.2` in `App::new()`.

### Step 5: Update from_params

Add `dust_tau` parameter to `GalaxyUniform::from_params()` and update all
call sites (main.rs redraw + test functions). Use `0.2` as default in
tests.

### Step 6: Validate

```bash
cargo check
cargo clippy -- -D warnings
cargo test
```

Fix test failures from the new uniform field.

## Test plan

- All existing tests pass (updated for new `dust_tau` field)
- Add test: `dust_tau_default_reasonable` — verifies default 0.2 is finite
  and positive
- Manual test: orbit to edge-on view, toggle dust tau from 0.0 to 1.0 —
  verify the midplane darkens and the disk rim becomes warm-colored.
  Face-on view should show subtle reddening toward center at higher tau.

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0 (all pass)
- [ ] Dust extinction visibly reddens edge-on views at τ > 0
- [ ] Dust tau slider works in egui
- [ ] `plans/README.md` updated

## STOP conditions

- Shader compilation fails (SPIR-V error)
- Extinction makes galaxy invisible at default settings
- Offset/size tests fail and can't be trivially updated
- Any existing test fails for non-trivial reasons

## Maintenance notes

- The extinction model is deliberately simple (no scattering, no grain size
  distribution, no PAH emission). It captures the first-order reddening
  effect with negligible GPU cost.
- `dust_tau = 0.2` is a reasonable default for face-on spirals. For edge-on
  views, tau through the midplane can reach 1-2 — the slider range 0-1
  lets users explore this.
- Future improvements: make dust_tau scale with metallicity (older/redder
  galaxies have more dust), or add a separate dust column profile that
  follows the molecular gas rather than the stellar density.
