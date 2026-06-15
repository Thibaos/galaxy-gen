# AGENTS.md — Agent guidance for galaxy-gen

## Project

A Rust desktop app that renders procedural galaxy images on the GPU.
Single binary, no server, no network, no database.

## Commands

| Purpose   | Command                    |
|-----------|----------------------------|
| Check     | `cargo check`              |
| Lint      | `cargo clippy -- -D warnings` |
| Test      | `cargo test`               |
| Build     | `cargo build`              |
| Run       | `cargo run --release`      |

## Conventions

- Rust edition 2024, nightly toolchain (see `rust-toolchain.toml`)
- GPU resources stored as `Option<T>` on `App` struct, created in `App::init()`
- Per-frame resources recreated only when dimensions change (see `recreate_texture` pattern)
- `unwrap()` is acceptable for GPU resource access after the init guard clause
- Shader source lives in `galaxy-shader/src/lib.rs` (spirv-std, `#[spirv(compute(threads(8,8,1)))]`)
- Host–shader struct correspondence: `src/gpu.rs` and `galaxy-shader/src/lib.rs` must keep `GalaxyUniform` in sync
- Tests live in `#[cfg(test)] mod tests` at the bottom of each source file
- Avoid using the `cargo clean` command, unless absolutely necessary

## Gotchas

- The `build.rs` compiles `galaxy-shader` to SPIR-V and embeds it via `env!("galaxy_shader.spv")`
- Changing `galaxy-shader/src/lib.rs` requires a clean rebuild (`cargo clean` or touch `build.rs`)
- The SPIR-V compute shader uses 8×8 thread groups; image dimensions must be divisible by 8 or the shader handles out-of-bounds via early-return
- The `image` crate is listed in Cargo.toml but unused — plans 006 removes it
- Pan: mouse drag. Zoom: mouse wheel. Exposure: ← →. Contrast: ↑ ↓.
