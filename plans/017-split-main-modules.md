# Plan 017: Split `main.rs` monolith into modules

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat HEAD~1..HEAD -- src/main.rs`
> If `main.rs` changed substantially (>50 lines) since this plan was written,
> treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: L
- **Category**: tech debt
- **Depends on**: 013, 014, 015 (all small modifications to main.rs — execute
  them first to avoid conflict)
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

Create `src/camera.rs` with the `Camera` struct and methods. Move
`camera_position()` logic from `App` impl into `Camera::position()`.

Update `App` to hold a `camera: Camera` field instead of individual
`camera_*` fields. Update all call sites.

**Verify**: `cargo check`

### Step 2: Extract `snapshot.rs`

Move `save_snapshot()` from `App` impl to a free function in `snapshot.rs`.
Takes `&App` (or the needed fields) + output path.

**Verify**: `cargo check`

### Step 3: Extract `ui.rs`

Move the entire egui sidebar construction block (~lines 830-1090) into
`ui::build_sidebar(&mut egui::Ui, ...)`. This is the largest single block.

At this point `main.rs` should be ~600 lines with only the render loop
and input handling left.

**Verify**: `cargo check`

### Step 4: Extract `input.rs`

Move input event handling (`WindowEvent` match arms for mouse, keyboard,
modifiers, scroll, cursor moved) into `input::handle_window_event()`.

**Verify**: `cargo check`

### Step 5: Extract `render.rs`

Move the `redraw()` function into `render.rs`. This is the final large
extraction.

After this step, `main.rs` should be ~30-50 lines: `fn main()`, winit
event loop setup, and delegation to `app`, `input`, `render`, `ui` modules.

**Verify**: `cargo check`

### Step 6: Full validation

```bash
cargo clippy -- -D warnings
cargo test
```

Fix any warnings. All 45 tests must pass.

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

- Any compilation error that can't be resolved by adding `pub` visibility
- Cyclic module dependency (e.g. `render.rs` importing `ui.rs` and vice versa)
- Any existing test fails
- Manual test reveals rendering regression
