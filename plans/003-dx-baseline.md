# Plan 003: Establish README, AGENTS.md, CI, and lint config

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 5a0cdfc..HEAD -- Cargo.toml rust-toolchain.toml`
> If either file changed, compare "Current state" excerpts against live code.

## Status

- **Priority**: P1
- **Effort**: S
- **Category**: dx
- **Depends on**: none
- **Planned at**: commit `5a0cdfc`, 2026-06-14
- **Issue**: —

## Why this matters

The project has no README, no AGENTS.md, no CI, no lint enforcement, no license, and no `.env.example`. A contributor (human or agent) arriving fresh cannot know what the project does, how to build it, what prerequisites are needed, or what conventions to follow. Adding these files requires zero code changes but makes the project maintainable and enables automated quality gates.

## Current state

- No `README.md`, `AGENTS.md`, `LICENSE`, `.github/`, `.env.example`, `clippy.toml`
- `Cargo.toml` has correct metadata (name/version/edition)
- `rust-toolchain.toml` pins `nightly-2026-04-11` with `rust-src`, `rustc-dev`, `llvm-tools`
- `Cargo.lock` is committed (correct for a binary)
- Prerequisites: nightly Rust + spirv-builder (depends on `rustc-dev` + `llvm-tools` components)
- Build command: `cargo build`
- Check: `cargo check`, `cargo clippy`
- Test: `cargo test` (currently no tests)
- The `galaxy.png` in the repo root (919 KB) is a reference render

## Commands you will need

| Purpose   | Command               | Expected on success |
|-----------|-----------------------|---------------------|
| Check     | `cargo check`         | exit 0              |
| Clippy    | `cargo clippy -- -D warnings` | exit 0, no warnings |
| Test      | `cargo test`          | all tests pass      |

## Scope

**In scope** (files to create):
- `README.md` — project description, screenshot, prerequisites, build/run commands
- `AGENTS.md` — agent guidance: conventions, commands, gotchas
- `LICENSE` — MIT or Apache-2.0 (ask user if not already decided)
- `.github/workflows/ci.yml` — CI: check, clippy (deny warnings), test
- `clippy.toml` — enforce `too_many_arguments` threshold = 12 (current code has 11-arg functions; this silences the pre-existing warning while catching worse cases)
- `.env.example` — placeholder noting no env vars needed currently

**Out of scope**:
- Any Rust source file changes
- `Cargo.toml` changes
- `.gitignore` changes

## Git workflow

- Branch: `advisor/003-dx-baseline`
- Commit per logical file group; message style: imperative, lowercase, e.g. `add README with build instructions`
- Do NOT push or open a PR unless instructed

## Steps

### Step 1: Create `README.md`

Create at repo root:

```markdown
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
# Build and run
cargo run --release
```

## Controls

| Key / Action        | Effect                     |
|---------------------|----------------------------|
| Left / Right arrow  | Decrease / increase exposure |
| Up / Down arrow     | Increase / decrease contrast |
| Mouse drag          | Pan                         |
| Mouse wheel         | Zoom in / out               |

## Development

```bash
cargo check        # Fast compile check
cargo clippy       # Lint
cargo test         # Run tests
cargo build        # Build
cargo run --release # Run optimized
```

## Architecture

- `src/main.rs` — window, input handling, rendering loop
- `src/gpu.rs` — GPU compute pipeline, uniform buffer, dispatch
- `src/galaxy.rs` — galaxy parameter definitions
- `src/display.wgsl` — fullscreen quad vertex/fragment shader
- `src/lib.rs` — crate root, re-exports
- `galaxy-shader/` — SPIR-V compute shader (density model, stars, tone mapping)
- `build.rs` — compiles galaxy-shader to SPIR-V via spirv-builder
```

**Verify**: `cat README.md | head -5` → shows "# Galaxy Gen"

### Step 2: Create `AGENTS.md`

Create at repo root:

```markdown
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

## Gotchas

- The `build.rs` compiles `galaxy-shader` to SPIR-V and embeds it via `env!("galaxy_shader.spv")`
- Changing `galaxy-shader/src/lib.rs` requires a clean rebuild (`cargo clean` or touch `build.rs`)
- The SPIR-V compute shader uses 8×8 thread groups; image dimensions must be divisible by 8 or the shader handles out-of-bounds via early-return
- The `image` crate is listed in Cargo.toml but unused — plans 006 removes it
- Pan: mouse drag. Zoom: mouse wheel. Exposure: ← →. Contrast: ↑ ↓.
```

**Verify**: `cat AGENTS.md | head -3` → shows "# AGENTS.md"

### Step 3: Create `LICENSE`

Create `LICENSE` at repo root. Use the MIT license (short, permissive, standard for Rust projects). Full text:

```
MIT License

Copyright (c) 2026

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

**STOP**: If the project maintainer prefers a different license, replace the content. Otherwise proceed with MIT.

**Verify**: `grep -c "MIT License" LICENSE` → `1`

### Step 4: Create `clippy.toml`

Create at repo root. The current code has functions with 11 arguments (clippy's default threshold is 7). Rather than suppressing the lint entirely, raise the threshold to 12 to allow the current code while catching anything worse:

```toml
too-many-arguments-threshold = 12
```

**Verify**: `cargo clippy -- -D warnings` → exit 0

### Step 5: Create `.github/workflows/ci.yml`

Create the directory structure:

```yaml
name: CI

on:
  push:
    branches: [main, master]
  pull_request:
    branches: [main, master]

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    name: Check + Clippy + Test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust nightly
        run: |
          rustup toolchain install nightly-2026-04-11 --profile minimal --component rust-src rustc-dev llvm-tools
          rustup override set nightly-2026-04-11

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: ${{ runner.os }}-cargo-

      - name: Check
        run: cargo check

      - name: Clippy (deny warnings)
        run: cargo clippy -- -D warnings

      - name: Test
        run: cargo test
```

**Verify**: `ls .github/workflows/ci.yml` → file exists

### Step 6: Create `.env.example`

Create at repo root:

```
# galaxy-gen does not currently use environment variables.
# This file exists as a placeholder for future configuration needs.
```

**Verify**: `cat .env.example` → shows content

### Step 7: Final verification

Run all verification commands:

**Verify**:
- `cargo check` → exit 0
- `cargo clippy -- -D warnings` → exit 0
- `cargo test` → exit 0 (tests from plan 002 if already applied; passes trivially if not)
- `git status` → only new files added

## Test plan

No code changes — the CI workflow file is the test. It will run on the next push/PR and validate check + clippy + test.

## Done criteria

- [ ] `README.md` exists and contains build/run instructions
- [ ] `AGENTS.md` exists and contains conventions + commands + gotchas
- [ ] `LICENSE` exists (MIT)
- [ ] `.github/workflows/ci.yml` exists with check + clippy + test jobs
- [ ] `clippy.toml` exists with `too-many-arguments-threshold = 12`
- [ ] `.env.example` exists
- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] No source files modified
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back if:

- `cargo clippy -- -D warnings` fails. Do NOT lower the threshold below 11; fix any actual warnings instead.
- The project uses a VCS other than git (confirmed git at time of plan).
- A file you're asked to create already exists with conflicting content — report the conflict.

## Maintenance notes

- The CI workflow uses `ubuntu-latest` which has GPU drivers but no physical GPU — `cargo check`/`clippy`/`test` work without a GPU because `spirv-builder` compiles the shader at build time and tests are CPU-only.
- If the project later adds integration tests that require a GPU, those must be gated behind a separate CI job or skipped in headless CI.
- When the nightly toolchain is bumped in `rust-toolchain.toml`, update the CI workflow's install step to match.
