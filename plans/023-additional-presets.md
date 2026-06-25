# Plan 023: Add iconic galaxy presets (M31, M51, M101)

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat HEAD~1..HEAD -- src/galaxy.rs galaxies/`
> If `GalaxyParams` struct or existing presets changed since this plan was
> written, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: M per preset (this plan: 3 presets = 1-2 hours total)
- **Category**: direction (content)
- **Depends on**: none
- **Planned at**: commit `1647567`, 2026-06-25

## Why this matters

The app currently has two presets: Milky Way and NGC 628 (M74). Adding more
iconic targets makes the demo more compelling and exercises the galaxy model
across a wider range of physical parameters.

**M31 (Andromeda Galaxy)** — Our nearest large spiral neighbor. Massive
(M★ ≈ 1.1×10¹¹ M☉), face-on-ish (i ≈ 77°), with a prominent bulge and
weak spiral arms. The dominant bulge makes it a good contrast to the
disk-dominated Milky Way preset.

**M51 (Whirlpool Galaxy)** — The classic grand-design spiral. Face-on,
two prominent arms with strong pitch angle (~20°), interacting with
companion NGC 5195. High arm contrast makes it the best showcase for the
logarithmic spiral formula.

**M101 (Pinwheel Galaxy)** — A large, face-on, late-type spiral with
many loosely-wound arms and low surface brightness. The extreme scale
length (~16 kLY) and low central density stress-test the renderer's
dynamic range.

## Reference data

Data must be sourced and saved to `galaxies/` as per AGENTS.md: "Choose a
real galaxy target and research its physical parameters... Save all
measurements, sources, and derived values to `galaxies/<target>.toml`."

### M31 (Andromeda)

| Parameter | Value | Source |
|-----------|-------|--------|
| Distance | 780 kpc (2.54 Mly) | McConnachie+2005 |
| Stellar mass | 1.1×10¹¹ M☉ | Tamm+2012 |
| Disk scale length | 5.4 kpc (17.6 kLY) | Courteau+2011 |
| Disk scale height | ~0.4 kpc (1.3 kLY) | estimated from hz/hr ≈ 0.075 (lenticular) |
| Bulge effective radius | ~1.0 kpc (3.3 kLY) | Courteau+2011 |
| Bulge-to-total ratio (B/T) | ~0.3 | Courteau+2011 |
| Inclination | 77° (face-on = 90°) | de Vaucouleurs+1991 |
| Arm morphology | Weak, flocculent | — |
| Pitch angle | ~7° | estimated from optical images |

### M51 (Whirlpool)

| Parameter | Value | Source |
|-----------|-------|--------|
| Distance | 7.6 Mpc (24.8 Mly) | Vinkó+2012 |
| Stellar mass | ~5×10¹⁰ M☉ | Leroy+2008 |
| Disk scale length | ~2.5 kpc (8.2 kLY) | Gutiérrez+2011 |
| Disk scale height | ~0.3 kpc (1.0 kLY) | estimated (hz/hr ≈ 0.12) |
| Bulge effective radius | ~0.7 kpc (2.3 kLY) | Gutiérrez+2011 |
| B/T | ~0.15 | Gutiérrez+2011 |
| Inclination | 20° (nearly face-on) | Tully+1988 |
| Arm morphology | Grand-design, two dominant arms | — |
| Pitch angle | ~21° | Kennicutt+1981 |
| Bar | Present (weak) | — |

### M101 (Pinwheel)

| Parameter | Value | Source |
|-----------|-------|--------|
| Distance | 6.4 Mpc (20.9 Mly) | Shappee+Stanford 2018 |
| Stellar mass | ~6×10¹⁰ M☉ | Leroy+2008 |
| Disk scale length | ~4.9 kpc (16.0 kLY) | Muñoz-Mateos+2009 |
| Disk scale height | ~0.5 kpc (1.6 kLY) | estimated (hz/hr ≈ 0.1) |
| Bulge effective radius | ~1.0 kpc (3.3 kLY) | estimated (small bulge) |
| B/T | ~0.08 | estimated from optical images |
| Inclination | 18° (nearly face-on) | de Vaucouleurs+1991 |
| Arm morphology | Many loosely-wound arms (grand-design) | — |
| Pitch angle | ~25° | Kennicutt+1981 |
| H I disk | Extends to ~50 kpc | Walter+2008 |

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**
- `galaxies/m31.toml` — reference data for M31
- `galaxies/m51.toml` — reference data for M51
- `galaxies/m101.toml` — reference data for M101
- `src/galaxy.rs` — add `GalaxyParams::m31()`, `::m51()`, `::m101()` constructors
- `src/gpu.rs` — add column profile tests for each new preset
- `src/main.rs` — add new presets to the egui dropdown

## Steps

### Step 1: Research and document

Create `galaxies/m31.toml`, `galaxies/m51.toml`, `galaxies/m101.toml` with
all physical parameters, sources, derived values, and uncertainty notes.
Follow the format of `galaxies/milky_way.toml` and `galaxies/ngc628.toml`.

Use web search to fill in missing values (especially B/T ratios and
bulge radii) from recent literature (2010+).

### Step 2: Add M31 preset

In `src/galaxy.rs`, add:

```rust
impl GalaxyParams {
    /// M31 (Andromeda Galaxy)
    ///
    /// ... documented comment block with physical parameters ...
    pub fn m31() -> Self { ... }
}
```

Derive `disk_central_density` and `bulge_central_density` from stellar mass
and B/T ratio. Follow the methodology in the existing presets.

### Step 3: Add column profile tests for M31

In `src/gpu.rs` tests:

```rust
#[test]
fn m31_disk_column_exponential() { ... }
#[test]
fn m31_bulge_column_plummer() { ... }
```

Same pattern as milky_way and ngc628 tests.

### Step 4: Add M51 preset + tests

Repeat steps 2-3 for M51. The high pitch angle (~21°) and strong arms
make this an interesting validation case — verify arm modulation produces
visually distinct spiral structure in the column density profile.

### Step 5: Add M101 preset + tests

Repeat steps 2-3 for M101. The large scale length (~16 kLY) pushes the
disc_radius calculation in `generate_star_catalogue` — verify the 8×
scale length doesn't exceed the 80,000 LY cap.

### Step 6: Add to egui dropdown

In `src/main.rs`, add the three new presets to the egui `ComboBox`:

```rust
egui::ComboBox::from_label("Preset")
    .selected_text(...)
    .show_ui(ui, |ui| {
        ui.selectable_value(&mut ..., GalaxyPreset::MilkyWay, "Milky Way");
        ui.selectable_value(&mut ..., GalaxyPreset::Ngc628, "NGC 628 (M74)");
        ui.selectable_value(&mut ..., GalaxyPreset::M31, "M31 (Andromeda)");
        ui.selectable_value(&mut ..., GalaxyPreset::M51, "M51 (Whirlpool)");
        ui.selectable_value(&mut ..., GalaxyPreset::M101, "M101 (Pinwheel)");
    });
```

Add the enum variants to `GalaxyPreset`.

### Step 7: Full validation

```bash
cargo clippy -- -D warnings
cargo test
```

## Test plan

- All existing tests + ~9 new column profile tests (3 per preset × ~3 tests)
  pass
- Each new preset constructor passes the existing finiteness/clone/f32-cast
  tests (any test that iterates over all presets)
- Manual test: switch to each new preset, verify 2D and 3D rendering produce
  visually distinct galaxies with correct relative sizes

## Done criteria

- [ ] `galaxies/m31.toml`, `m51.toml`, `m101.toml` exist with complete data
- [ ] Three new `GalaxyParams::*()` constructors in `src/galaxy.rs`
- [ ] Column profile tests for all three presets in `src/gpu.rs`
- [ ] All three presets appear in egui dropdown
- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0 (all pass)
- [ ] `plans/README.md` updated

## STOP conditions

- Unable to find reliable B/T ratio or bulge radius for any target
- Derived `disk_central_density` produces NaN or negative values
- Column density tests fail to converge (exponential/Plummer profiles don't
  match derived parameters)
- Disk scale length > 20 kLY (disc_radius clamp may need adjustment)
- Any existing test breaks

## Maintenance notes

- The hz/hr estimates for all three galaxies use literature correlations
  (late-type: 0.1–0.15; lenticular: ~0.075). These have ~30% uncertainty.
  Note this in each .toml file.
- M101's extreme scale length pushes the 8× scale_length disc_radius to
  128 kLY, which hits the 80k LY clamp. This is acceptable — the density
  at 80k LY is exp(−80/16) ≈ 0.7% of central, effectively invisible.
  Note this in `galaxies/m101.toml`.
