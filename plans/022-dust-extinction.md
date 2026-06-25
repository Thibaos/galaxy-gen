# Plan 022: Dust extinction in the ray-march path

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat 1647567..HEAD -- galaxy-shader/src/lib.rs src/gpu.rs src/main.rs`
> If `ray_march_galaxy`, the density functions, `GalaxyUniform`, or its
> `from_params` method changed since this plan was written, treat it as a
> STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
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

## Current state

**`ray_march_galaxy`** — `galaxy-shader/src/lib.rs` (~line 440):

```rust
fn ray_march_galaxy(pos: Vec3, dir: Vec3, params: &GalaxyUniform) -> Vec3 {
    let dt = (MAX_DIST - MIN_DIST) / (STEPS as f32);
    let mut t = MIN_DIST;
    let mut acc = Vec3::new(0.0, 0.0, 0.0);

    for _ in 0..STEPS {
        let p = pos + dir * t;
        let dens = density_3d(p, params);
        // Current: constant warm-white emissivity at each step
        acc.x += dens * dt * EMISSIVITY;
        acc.y += dens * dt * EMISSIVITY * 0.9;
        acc.z += dens * dt * EMISSIVITY * 0.7;
        t += dt;
    }

    acc
}
```

**`GalaxyUniform` struct** (`galaxy-shader/src/lib.rs` and `src/gpu.rs`) —
currently 28 `f32` fields ending with `fov_y_deg`. New field will follow.

**`from_params`** — `src/gpu.rs` (~line 310). Constructs struct literal
from `GalaxyParams`. Add `dust_tau` to the literal.

**Egui 3D panel** — `src/main.rs` (~line 940). Section with Show Glow,
Show Stars, Star brightness, Star size. Add Dust τ slider here.

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

### Step 1: Add dust constants + uniform field to shader

In `galaxy-shader/src/lib.rs`:

**A)** Add near other constants (after `EMISSIVITY`):
```rust
const DUST_EXTINCTION_R: f32 = 0.75;
const DUST_EXTINCTION_G: f32 = 1.00;
const DUST_EXTINCTION_B: f32 = 1.32;
```

**B)** Add to `GalaxyUniform` struct, after `fov_y_deg`:
```rust
pub dust_tau: f32,
```

**Verify**: `cargo check` → exit 0 for shader crate

### Step 2: Update host GalaxyUniform + from_params

In `src/gpu.rs`:

**A)** Add `pub dust_tau: f32,` to host struct (same position).

**B)** Add `dust_tau: f32` parameter to `from_params` and use it in
   the struct literal (instead of hardcoding 0.2):
```rust
pub fn from_params(params: &GalaxyParams, dust_tau: f32) -> Self {
    Self {
        // ... existing fields ...
        dust_tau,
    }
}
```

**C)** Update `galaxy_uniform_size` test (28 → 29 f32 fields, 112 → 116 bytes).
Update any offset test referencing `fov_y_deg`.

**Verify**: `cargo check` → exit 0; `cargo test galaxy_uniform` → pass

### Step 3: Apply extinction in ray_march_galaxy

In `galaxy-shader/src/lib.rs`, modify the loop body in `ray_march_galaxy`.
Add three accumulators before the loop, replace emissivity accumulation:

```rust
let mut dust_r = 0.0_f32;
let mut dust_g = 0.0_f32;
let mut dust_b = 0.0_f32;

for _ in 0..STEPS {
    let p = pos + dir * t;
    let dens = density_3d(p, params);

    let dust_dtau = dens * dt * params.dust_tau;
    dust_r += dust_dtau * DUST_EXTINCTION_R;
    dust_g += dust_dtau * DUST_EXTINCTION_G;
    dust_b += dust_dtau * DUST_EXTINCTION_B;

    acc.x += dens * dt * EMISSIVITY * (-dust_r).exp();
    acc.y += dens * dt * EMISSIVITY * 0.9 * (-dust_g).exp();
    acc.z += dens * dt * EMISSIVITY * 0.7 * (-dust_b).exp();

    t += dt;
}
```

**Verify**: `cargo check` → exit 0; requires shader rebuild (touch `build.rs`)

### Step 4: Add slider + init in main.rs

**A)** Add `dust_tau: f32,` to `App` struct, init to `0.2` in `App::new()`.

**B)** In egui 3D panel (~line 940), after Star size slider:
```rust
ui.add(egui::Slider::new(&mut self.dust_tau, 0.0..=1.0).text("Dust τ"));
```

**C)** At the `from_params` call site in `redraw()`, pass `self.dust_tau`:
```rust
let uniform_data = GalaxyUniform::from_params(&self.params, self.dust_tau);
```

Also update any test code that calls `from_params` to pass `0.2`.

**Verify**: `cargo check` → exit 0

### Step 5: Full validation

```bash
cargo clippy -- -D warnings
cargo test  # fix any test failures from uniform layout changes
```

## Git workflow

- Branch: `advisor/022-dust-extinction`
- Commit message: `feat: add dust extinction to ray-march path`
- Do NOT push or open a PR unless instructed.

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

- The code at the locations in "Current state" doesn't match the excerpts
  (the codebase has drifted since this plan was written).
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
