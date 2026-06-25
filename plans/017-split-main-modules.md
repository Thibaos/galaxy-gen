# Plan 017: Split `main.rs` monolith into modules

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat 1647567..HEAD -- src/main.rs`
> If `main.rs` changed substantially (>50 lines) since this plan was written,
> treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: MEDIUM
- **Category**: tech debt
- **Depends on**: 013, 014, 015 recommended (execute them first to avoid
  merge conflicts); this plan CAN be executed standalone if 013/014/015
  are not yet done — just expect slightly different line numbers
- **Planned at**: commit `1647567`, 2026-06-25

## Why this matters

`main.rs` is 1114 lines and does everything: `App` struct (88 fields),
window init, GPU resource init, camera math, input handling (mouse +
keyboard + modifiers), render dispatch, egui panel construction (~350 lines
of UI), star catalogue management, snapshot export, and the winit event
loop.

A monolith of this size makes every change risky: edits to the render loop
can accidentally break snapshot export; adding a UI control can break input
handling. Splitting into focused modules reduces coupling, makes each file
testable in isolation, and makes the codebase approachable to new
contributors.

## Current state

All in `src/main.rs`:

| Concern             | Lines (approx) | Proposed module |
|---------------------|----------------|-----------------|
| `App` struct fields  | 1–88           | `app.rs`        |
| `App::new()` init    | 90–230         | `app.rs`        |
| Camera math          | 192–220        | `camera.rs`     |
| Input handling       | 480–700        | `input.rs`      |
| Render dispatch      | 230–280, 730–810 | `render.rs`   |
| Eq UI construction  | 830–1090       | `ui.rs`         |
| Snapshot export      | 450–480        | `snapshot.rs`   |
| Star catalogue       | 400–445        | `app.rs` (small fn) |
| winit event loop     | 1085–1114      | `main.rs`       |

Key code blocks being moved (current excerpts):

**`camera_position()`** — a free function or method computing
`glam::Mat4::look_at_rh(...)` from azimuth/elevation/distance:
```rust
fn camera_position(azimuth: f32, elevation: f32, dist: f32, target: Vec3) -> (Vec3, Vec3) {
    let pos = target + Vec3::new(
        azimuth.cos() * elevation.cos() * dist,
        elevation.sin() * dist,
        azimuth.sin() * elevation.cos() * dist,
    );
    let forward = (target - pos).normalize();
    (pos, forward)
}
```

**`save_snapshot()`** — current method on `App` (~line 450), maps GPU buffer
to CPU, writes PNG via `image` crate:
```rust
pub fn save_snapshot(&self, path: &std::path::Path) {
    // maps self.rgba_buffer, reads pixel data,
    // constructs image::RgbaImage, calls .save(path)
}
```

**`redraw()`** — currently an `impl App` method (~100 lines). Accesses
`self` fields for device, queue, surface, config, window, display_tex,
gpu_compute, uniform_buffer, rgba_buffer, render_w, render_h, needs_render,
render_mode, show_glow, show_stars, gpu_stars, star_catalogue, egui_state,
egui_renderer, egui_context, frame_count, and snapshot path.
During extraction, convert to a free function taking `app: &mut App` plus
any externally-owned resources (device, queue, surface, config, window).
Within the body, replace `self.` with `app.` and `&self.` with `&app.`.

**App struct camera fields** — the individual fields being collapsed:
```rust
camera_dist: f32,
camera_azimuth: f32,
camera_elevation: f32,
camera_target: glam::Vec3,
camera_fov: f32,
```
These five become a single `camera: Camera` field.

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**
- `src/main.rs` → shrink to ~30 lines (winit loop bootstrapping only)
- `src/app.rs` — new: `App` struct, `App::new()`, `ensure_star_catalogue()`,
  `write_view_proj_matrix()`, `save_snapshot()`
- `src/camera.rs` — new: `Camera` struct, orbit math, `camera_position()`
- `src/render.rs` — new: `redraw()` function
- `src/ui.rs` — new: egui sidebar panel builder
- `src/input.rs` — new: `InputState` struct, event handlers

**Out of scope:**
- `gpu.rs` — already a module, left alone
- `galaxy.rs` — already a module, left alone
- Any behavioral changes, refactoring beyond file splits

## Design

### Module dependency tree

```
main.rs ──► app.rs ──► camera.rs
                │          │
                ├──► gpu.rs
                ├──► galaxy.rs
                ├──► render.rs ──► ui.rs
                └──► snapshot.rs

input.rs ──► app.rs, render.rs
ui.rs    ──► app.rs, camera.rs
render.rs──► app.rs, camera.rs, gpu.rs
```

### `App` struct stays in `app.rs`

All 88 fields remain on `App`. The struct is the single source of truth for
state. Methods on `App` stay in `app.rs`:

- `App::new()` — window + GPU init
- `ensure_star_catalogue()` — star generation
- `write_view_proj_matrix()` — per-frame uniform upload
- `save_snapshot()` — PNG export

### `Camera` extracted to `camera.rs`

```rust
pub struct Camera {
    pub dist: f32,
    pub azimuth: f32,
    pub elevation: f32,
    pub target: glam::Vec3,
    pub fov_y_deg: f32,
}

impl Camera {
    pub fn position(&self) -> glam::Vec3;
    pub fn orbit(&mut self, d_azimuth: f32, d_elevation: f32);
    pub fn zoom(&mut self, factor: f32);
}
```

### `redraw()` moves to `render.rs`

Function signature becomes:

```rust
pub fn redraw(
    app: &mut App,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    surface: &wgpu::Surface<'_>,
    config: &mut wgpu::SurfaceConfiguration,
    window: &Window,
    display: &DisplayPipeline,
    egui_state: &mut EguiState,
    egui_renderer: &mut egui_wgpu::Renderer,
    egui_context: &egui::Context,
    star_catalogue: &mut Vec<StarInstance>,
) -> Result<(), wgpu::SurfaceError>
```

The function reads state from `App`, dispatches GPU work, and mutates
`App.needs_render` and `App.frame_count`.

### Eq UI moves to `ui.rs`

```rust
pub fn build_sidebar(
    ui: &mut egui::Ui,
    app: &mut App,
    camera: &mut Camera,
) -> bool  // returns true if any control changed (→ needs_render)
```

This function builds the entire egui sidebar panel. It returns a bool
indicating whether any slider/toggle/button was interacted with.

### Input handling moves to `input.rs`

```rust
pub struct InputState {
    pub mouse_x: f64,
    pub mouse_y: f64,
    pub last_mouse_x: f64,
    pub last_mouse_y: f64,
    pub dragging: bool,
    pub orbit_dragging: bool,
}

pub fn handle_window_event(
    event: &WindowEvent,
    app: &mut App,
    input: &mut InputState,
    egui_ctx: &egui::Context,
)
```

## Steps

### Step 0: Verify clean baseline

```bash
cargo check && cargo clippy -- -D warnings && cargo test
```

All must pass before beginning.

### Step 1: Extract `camera.rs`

Create `src/camera.rs` with:

```rust
pub struct Camera {
    pub dist: f32,
    pub azimuth: f32,
    pub elevation: f32,
    pub target: glam::Vec3,
    pub fov_y_deg: f32,
}

impl Camera {
    pub fn position(&self) -> glam::Vec3 { ... }
    pub fn forward(&self) -> glam::Vec3 { ... }
    pub fn orbit(&mut self, d_azimuth: f32, d_elevation: f32) { ... }
    pub fn zoom(&mut self, factor: f32) { ... }
}
```

Move the existing `camera_position()` free function body into these methods.
Replace individual `camera_dist`, `camera_azimuth`, `camera_elevation`,
`camera_target`, `camera_fov` fields on `App` with a single `camera: Camera`
field. Update every reference to `self.camera_*` to `self.camera.*`.
Start with `App::new()` and `redraw()`; compiler errors will guide you to
remaining call sites.

**Verify**: `cargo check` → exit 0

### Step 2: Extract `snapshot.rs`

Create `src/snapshot.rs`. Move the `save_snapshot()` method body to a
free function:

```rust
pub fn save_snapshot(rgba_buffer: &wgpu::Buffer, device: &wgpu::Device,
    queue: &wgpu::Queue, width: u32, height: u32, path: &std::path::Path)
```

Remove `save_snapshot` from `impl App`. Update the caller in the egui
sidebar button handler to pass `self.rgba_buffer`, `self.device`, etc.

**Verify**: `cargo check` → exit 0

### Step 3: Extract `ui.rs`

Create `src/ui.rs`. Move the entire egui sidebar construction block
(~lines 830–1090) into:

```rust
pub fn build_sidebar(ui: &mut egui::Ui, app: &mut App) -> bool
```

Returns `true` if any control changed (→ set `needs_render`). The function
references app fields like `params`, `render_mode`, `show_stars`, all
sliders/toggles/buttons. Cut the block whole and replace with a call.

After this step, `main.rs` should be ~600 lines.

**Verify**: `cargo check` → exit 0

### Step 4: Extract `input.rs`

Create `src/input.rs`. Move mouse/keyboard/scroll/cursor event handling
from the `WindowEvent` match arms into:

```rust
pub struct InputState { pub mouse_x: f64, pub mouse_y: f64, ... }
pub fn handle_window_event(event: &WindowEvent, app: &mut App,
    input: &mut InputState, egui_ctx: &egui::Context)
```

The app event loop calls this function, which mutates `app` state
(camera angles, needs_render, etc.).

**Verify**: `cargo check` → exit 0

### Step 5: Extract `render.rs`

Create `src/render.rs`. Move the `redraw()` function into it. This is the
final extraction. After this, `main.rs` is ~30–50 lines: `fn main()`,
winit event loop bootstrap, and delegation to modules.

**Verify**: `cargo check` → exit 0

### Step 6: Full validation

```bash
cargo clippy -- -D warnings
cargo test
```

Fix any warnings. All 45 tests must pass. `wc -l src/main.rs` < 100.

## Git workflow

- Branch: `advisor/017-split-main-modules`
- Commit message: `refactor: split main.rs into modules`
- Do NOT push or open a PR unless instructed.

## Test plan

- All 45 existing tests pass (no behavioral change)
- `cargo clippy` produces zero warnings
- Manual smoke: launch app, verify 2D + 3D rendering works, orbit camera,
  toggle stars, switch presets, take screenshot

## Done criteria

- [ ] `src/main.rs` < 100 lines
- [ ] New modules: `camera.rs`, `snapshot.rs`, `ui.rs`, `input.rs`, `render.rs`
- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0 (all 45 pass)
- [ ] Manual smoke test passes
- [ ] `plans/README.md` updated

## STOP conditions

- The code at the locations in "Current state" doesn't match the excerpts
  (the codebase has drifted since this plan was written).
- Any compilation error that can't be resolved by adding `pub` visibility
- Cyclic module dependency (e.g. `render.rs` importing `ui.rs` and vice versa)
- Any existing test fails
- Manual test reveals rendering regression

## Maintenance notes

- After the split, `src/main.rs` is the bootstrapper only. Any new feature
  should be scoped to the appropriate module: camera math → `camera.rs`,
  UI controls → `ui.rs`, input → `input.rs`, rendering → `render.rs`.
- The `App` struct remains in `app.rs` as the single source of truth.
  Resist the temptation to split it further unless a clear bounded
  sub-struct emerges (e.g. `RenderState`).
- If plans 013/014/015 were executed before this one, line numbers in
  `main.rs` may differ from the excerpts. The function signatures and
  names are the stable references.
