# Galaxy Gen

GPU-accelerated procedural galaxy generator. Renders spiral galaxies with
physically-inspired density models (exponential disk, Plummer bulge, power-law
halo, logarithmic spiral arms) and individual stars sampled from a Kroupa IMF
with blackbody colouring — all in real time on the GPU via a SPIR-V compute
shader.

![Galaxy Gen output](galaxy.png)

## Prerequisites

- [Rust](https://rustup.rs) nightly-2026-04-11 (see `rust-toolchain.toml`)
- The `rust-src`, `rustc-dev`, and `llvm-tools` components (installed automatically by the toolchain file)
- A GPU with Vulkan 1.2 support

## Quick Start

```bash
cargo run
```

## Controls

| Key / Action        | Effect                     |
|---------------------|----------------------------|
| Left / Right arrow  | Decrease / increase exposure |
| Up / Down arrow     | Increase / decrease contrast |
| Mouse drag          | Pan                         |
| Mouse wheel         | Zoom in / out               |

## Galaxy Presets

Galaxy parameters are calibrated against published photometric surveys.
To switch the rendered galaxy, change the constructor call near the top of
`main()` in `src/main.rs`.

| Preset | Galaxy | Type | Key reference |
|--------|--------|------|---------------|
| `GalaxyParams::milky_way()` | Milky Way | SBbc | Binney & Tremaine (2008) |
| `GalaxyParams::ngc628()` | NGC 628 (M74 / Phantom Galaxy) | SA(s)c | S4G (Salo et al. 2015), PHANGS (Leroy et al. 2021) |

Reference data — including original measurements, derived shader parameters,
conversion formulas, and academic sources — lives in `galaxies/*.toml`.
No web searching required when tuning or adding new galaxy targets.

### Calibration confidence

| Parameter | Milky Way | NGC 628 |
|-----------|-----------|---------|
| Disk scale length | Measured (local) | Measured (S4G) |
| Disk scale height | Measured (local) | Estimated (hz/hr ratio) |
| Bulge Re | Measured | Measured (S4G) |
| Arm pitch | Measured (cosmic microwave) | Estimated (images) |
| Stellar mass | Measured (local) | Measured (PHANGS) |
| Halo | Measured (local) | Scaled from MW |

## Architecture

- `src/main.rs` — window, input handling, rendering loop
- `src/gpu.rs` — GPU compute pipeline, uniform buffer, dispatch
- `src/galaxy.rs` — galaxy parameter definitions and presets
- `src/display.wgsl` — fullscreen quad vertex/fragment shader
- `src/lib.rs` — crate root, re-exports
- `galaxy-shader/` — SPIR-V compute shader (density model, stars, tone mapping)
- `galaxies/` — reference data for real-galaxy targets (TOML, sources included)
- `build.rs` — compiles galaxy-shader to SPIR-V via spirv-builder
