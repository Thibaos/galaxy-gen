# Plan 007: Add interactive GUI with egui

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat dffa539..HEAD -- Cargo.toml src/main.rs`
> If either file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L (~300–400 lines of changes across 2 files)
- **Risk**: MED (version compatibility, rendering architecture change)
- **Category**: direction
- **Depends on**: none
- **Planned at**: commit `dffa539`, 2026-06-15
- **Issue**: —

## Why this matters

The app currently has no GUI — all interaction is keyboard shortcuts (exposure ←→, contrast ↑↓) and mouse (pan, zoom). Changing galaxy parameters requires editing `src/main.rs` and recompiling. Adding an immediate-mode GUI with `egui` transforms the app from a fixed-config demo into an interactive exploration tool: users can switch presets, tweak every parameter with sliders in real time, take screenshots, and see frame timing — all without recompiling. The UI multiplies the value of the existing preset system (now two presets: Milky Way + NGC 628) and makes the app usable by non-developers.

## Current state

The app is a single-window wgpu + winit application using `ApplicationHandler`:

- **`Cargo.toml`** — deps: wgpu 24, bytemuck 1, pollster 0.4, winit 0.30 (resolved to 0.30.13), image 0.25.10. No UI framework.
- **`src/main.rs`** — Struct `App` holds all state as `Option<T>` for GPU resources (convention: guard clause checks all are `Some`, then `unwrap()`). `App::init()` creates one-time resources. `App::redraw()` handles compute dispatch + display render pass. `App::window_event()` handles input. `about_to_wait()` requests redraw when `needs_render` is true.
- **`src/gpu.rs`** — `GpuCompute` (cached compute pipeline), `GalaxyUniform`, `compute_galaxy()`. Not modified by this plan.
- **`src/galaxy.rs`** — `GalaxyParams` with 12 public fields, `milky_way()` and `ngc628()` constructors. 9 tests. Not modified by this plan.
- **Toolchain**: nightly-2026-04-11. This satisfies the MSRV of egui 0.31+ (which requires 1.77+).

Key rendering flow in `redraw()` (lines 305–400):

```rust
// ── re-render galaxy (all on GPU) ────────────
if self.needs_render {
    // ... write uniforms, call compute_galaxy (creates own encoder, submits)
    self.needs_render = false;
}

// ── display ──────────────────────────────────
let frame = match surface.get_current_texture() { ... };
let view = frame.texture.create_view(...);
let mut encoder = device.create_command_encoder(...);
{
    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
            ..
        })],
        ..
    });
    rpass.set_pipeline(pipeline);
    rpass.set_bind_group(0, bind_group, &[]);
    rpass.draw(0..3, 0..1);
} // rpass dropped
queue.submit(Some(encoder.finish()));
frame.present();
```

The egui render pass must go between the display pass and `frame.present()`, using the same encoder and frame view with `LoadOp::Load` to preserve galaxy content. Only one `submit` + `present` per frame.

The `window_event` handler handles keyboard (arrow keys for exposure/contrast), mouse drag (pan), and mouse wheel (zoom). These must be gated behind `!self.egui_ctx.wants_pointer_input()` / `!self.egui_ctx.wants_keyboard_input()` to avoid conflicts.

## Commands you will need

| Purpose   | Command                    | Expected on success |
|-----------|----------------------------|---------------------|
| Check     | `cargo check`              | exit 0              |
| Clippy    | `cargo clippy -- -D warnings` | exit 0           |
| Test      | `cargo test`               | 21 tests pass       |
| Build     | `cargo build`              | exit 0              |

## Scope

**In scope** (files you may modify):

- `Cargo.toml` — add `egui`, `egui-wgpu`, `egui-winit` dependencies
- `src/main.rs` — add egui state fields, init, event forwarding, UI panel, render pass

**Out of scope** (do NOT touch):

- `src/gpu.rs` — no changes to the compute pipeline
- `src/galaxy.rs` — no changes to params/presets
- `galaxy-shader/src/lib.rs` — shader unchanged
- `src/display.wgsl` — unchanged
- `build.rs` — unchanged
- `src/lib.rs` — no new re-exports needed (egui is main.rs only)

## Git workflow

- Branch: `advisor/007-egui-gui`
- Commit per step; message style: imperative, lowercase (matches repo), e.g. `add egui dependencies`, `integrate egui render pass`
- Do NOT push or open a PR unless instructed

## Conventions

The repo stores GPU resources as `Option<T>` on `App`, initialized in `App::init()`, and unwrapped in methods after a guard clause. The egui renderer follows this same pattern — stored as `Option<egui_wgpu::Renderer>`.

Existing exemplar of the pattern (`src/main.rs:127-205` — `init` method, surface/device/queue/config/render_pipeline fields):

```rust
self.surface = Some(surface);
self.device = Some(device);
self.queue = Some(queue);
self.config = Some(config);
self.render_pipeline = Some(render_pipeline);
```

Uses of `unwrap()` after guard clauses (lines 305–325 in `redraw`):

```rust
if self.surface.is_none() || self.device.is_none() || ... || self.gpu_compute.is_none() {
    return;
}

let (surface, device, queue, ...) = (
    self.surface.as_ref().unwrap(),
    self.device.as_ref().unwrap(),
    ...
);
```

Match these patterns exactly for the new egui fields.

## Steps

### Step 1: Add egui dependencies

Add to `Cargo.toml` under `[dependencies]`:

```toml
egui = "0.31"
egui-wgpu = "0.31"
egui-winit = "0.31"
```

The app targets wgpu 24 and winit 0.30.13. egui 0.31 was the first major version line to support wgpu 24. The exact patch version will be resolved by Cargo.

**If `cargo check` fails with version-resolution errors**, try incrementing to `"0.32"` then `"0.33"`. Do NOT move past `"0.33"` without reporting — later versions may have API changes beyond what this plan covers.

**Verify**: `cargo check` → exit 0 (this downloads and resolves deps; may take a few minutes on first run)

### Step 2: Add egui state fields to the `App` struct

In `src/main.rs`, add three new fields to the `App` struct inside the existing struct body. Place them after the existing egui-irrelevant fields, before the `// ── mouse` section:

```rust
    // ── egui ──────────────────────────────
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,
```

In `App::new()`, initialize the non-Option field:

```rust
egui_ctx: egui::Context::default(),
egui_state: None,
egui_renderer: None,
```

**Verify**: `cargo check` → exit 0 (struct compiles; fields unused for now — expect dead-code warnings, ignore them)

### Step 3: Initialize egui in `App::init()`

Inside `App::init()`, after the line `self.gpu_compute = Some(gpu::GpuCompute::new(...));` (currently around line 230), add egui initialization.

The code needs the `window` (already an `Arc<Window>`), `device`, `queue`, and `surface_format`. These are all available by this point in `init()`.

The exact constructor signatures depend on the resolved egui version. Here are the APIs for 0.31 (try these first; adjust for 0.32/0.33 as needed):

```rust
// Create egui winit state
let egui_state = egui_winit::State::new(
    self.egui_ctx.clone(),
    egui::viewport::ViewportId::default(),
    &window,
    None,   // scale_factor override
    None,   // requested_theme
    None,   // follow_system_theme
);

// Create egui wgpu renderer
let egui_renderer = egui_wgpu::Renderer::new(
    device,
    config.format,            // surface_format
    None,                     // depth_format (no depth buffer)
    1,                        // msaa samples
    false,                    // shader validation
);

self.egui_state = Some(egui_state);
self.egui_renderer = Some(egui_renderer);
```

**If the API differs** (e.g. `State::new` takes different args in the resolved version):

- Run `cargo doc --open` and look up `egui_winit::State::new` to find the correct signature.
- `egui_wgpu::Renderer::new` typically accepts `(&device, texture_format, depth_format, msaa_samples)`.

**Verify**: `cargo check` → exit 0

### Step 4: Forward window events to egui

In `App::window_event()`, at the very top of the match block (before any event handling), add event forwarding. The `egui_state.on_window_event()` method takes `&Window` and `&WindowEvent` and returns an `EventResponse` with `consumed` and `repaint` fields (or equivalent struct, depending on version):

```rust
fn window_event(
    &mut self,
    event_loop: &ActiveEventLoop,
    _window_id: winit::window::WindowId,
    event: WindowEvent,
) {
    // Forward to egui — always
    if let (Some(egui_state), Some(window)) = (&mut self.egui_state, &self.window) {
        let response = egui_state.on_window_event(&window, &event);
        if response.repaint {
            window.request_redraw();
        }
    }

    match event {
        // ... existing handlers (modified below)
    }
}
```

Then modify each existing input handler to check egui consumption:

**Keyboard handler** (lines ~470-490): Wrap the arrow-key logic in `if !self.egui_ctx.wants_keyboard_input()` — this prevents exposure/contrast changes when the user is typing in an egui text field or focused on a slider.

```rust
WindowEvent::KeyboardInput { event, .. } => {
    if event.state.is_pressed()
        && let PhysicalKey::Code(key) = event.physical_key
        && !self.egui_ctx.wants_keyboard_input()
    {
        match key {
            KeyCode::ArrowLeft => self.exposure -= EXPOSURE_STEP,
            KeyCode::ArrowRight => self.exposure += EXPOSURE_STEP,
            KeyCode::ArrowUp => self.contrast += CONTRAST_STEP,
            KeyCode::ArrowDown => self.contrast -= CONTRAST_STEP,
            _ => return,
        }
        self.update_title();
        self.needs_render = true;
    }
}
```

**Mouse drag handler** (lines ~495-510): Wrap the pan logic in `if !self.egui_ctx.wants_pointer_input()`:

```rust
WindowEvent::CursorMoved { position, .. } => {
    let (cx, cy) = (position.x, position.y);
    let (lx, ly) = self.last_mouse;
    self.last_mouse = (cx, cy);

    if self.dragging && !self.egui_ctx.wants_pointer_input() {
        // ... existing pan logic (unchanged)
    }
}
```

**Mouse wheel / zoom handler** (lines ~515-545): Same — gate with `!self.egui_ctx.wants_pointer_input()`:

```rust
WindowEvent::MouseWheel { delta, .. } => {
    if self.egui_ctx.wants_pointer_input() {
        return;
    }
    // ... existing zoom logic (unchanged)
}
```

**Verify**: `cargo check` → exit 0. At this point the app won't render egui yet, but events are forwarded and input is correctly gated.

### Step 5: Build the egui UI in `redraw()`

In `redraw()`, after the galaxy display render pass but BEFORE `queue.submit()` and `frame.present()`, build the egui UI. The UI lives in a `egui::SidePanel::right` panel. This goes between the display render pass's closing brace and the `queue.submit()` call.

First, prepare the egui input for the frame:

```rust
// Build egui UI
let raw_input = self.egui_state.as_mut().unwrap().take_egui_input(&window);
let full_output = self.egui_ctx.run(raw_input, |ctx| {
    // ── Sidebar UI ──────────────────────
    egui::SidePanel::right("galaxy_controls")
        .default_width(280.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("Galaxy Controls");

            // ── Preset ──
            ui.separator();
            ui.label("Preset");
            let mut preset_idx: usize = if self.params.disk_scale_length < 9_000.0 { 0 } else { 1 };
            // HACK: differentiate Milky Way (disk_scale_length=8500) vs NGC 628 (10630)
            // A proper enum would be better, but we match existing convention.
            let preset_changed = egui::ComboBox::from_id_salt("preset")
                .selected_text(match preset_idx {
                    0 => "Milky Way",
                    _ => "NGC 628 (M74)",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut preset_idx, 0, "Milky Way");
                    ui.selectable_value(&mut preset_idx, 1, "NGC 628 (M74)");
                });
            if preset_changed.response.changed() {
                self.params = match preset_idx {
                    0 => GalaxyParams::milky_way(),
                    _ => GalaxyParams::ngc628(),
                };
                self.needs_render = true;
            }

            // ── Disk ──
            ui.separator();
            ui.label("Disk");
            let mut changed = false;
            changed |= ui.add(egui::Slider::new(&mut self.params.disk_scale_length, 1_000.0..=25_000.0).text("Scale length (ly)")).changed();
            changed |= ui.add(egui::Slider::new(&mut self.params.disk_scale_height, 100.0..=5_000.0).text("Scale height (ly)")).changed();
            changed |= ui.add(egui::Slider::new(&mut self.params.disk_central_density, 0.0..=0.05).text("Central density")).changed();

            // ── Arms ──
            ui.separator();
            ui.label("Spiral Arms");
            changed |= ui.add(egui::Slider::new(&mut self.params.arm_count, 0..=8).text("Arm count")).changed();
            changed |= ui.add(egui::Slider::new(&mut self.params.arm_pitch, 0.05..=0.8).text("Pitch angle (rad)")).changed();
            changed |= ui.add(egui::Slider::new(&mut self.params.arm_concentration, 1.0..=15.0).text("Concentration")).changed();
            changed |= ui.add(egui::Slider::new(&mut self.params.arm_strength, 0.0..=3.0).text("Strength")).changed();

            // ── Bulge ──
            ui.separator();
            ui.label("Bulge");
            changed |= ui.add(egui::Slider::new(&mut self.params.bulge_radius, 500.0..=15_000.0).text("Radius (ly)")).changed();
            changed |= ui.add(egui::Slider::new(&mut self.params.bulge_central_density, 0.0..=0.2).text("Central density")).changed();

            // ── Halo ──
            ui.separator();
            ui.label("Halo");
            changed |= ui.add(egui::Slider::new(&mut self.params.halo_radius, 10_000.0..=200_000.0).text("Radius (ly)")).changed();
            changed |= ui.add(egui::Slider::new(&mut self.params.halo_central_density, 0.0..=1e-6).text("Central density")).changed();
            changed |= ui.add(egui::Slider::new(&mut self.params.halo_slope, -5.0..=-1.5).text("Slope")).changed();

            if changed {
                self.needs_render = true;
            }

            // ── Brightness ──
            ui.separator();
            ui.label("Brightness");
            let exp_changed = ui.add(egui::Slider::new(&mut self.exposure, 0.0..=1.0).text("Exposure")).changed();
            let con_changed = ui.add(egui::Slider::new(&mut self.contrast, 0.0..=0.2).text("Contrast")).changed();
            if exp_changed || con_changed {
                self.update_title();
                self.needs_render = true;
            }

            // ── Actions ──
            ui.separator();
            if ui.button("Save Screenshot").clicked() {
                if let (Some(device), Some(queue)) = (self.device.as_ref(), self.queue.as_ref()) {
                    self.save_snapshot(device, queue);
                }
            }

            if ui.button("Reset Camera").clicked() {
                self.center_x = 0.0;
                self.center_y = 0.0;
                self.extent_ly = INITIAL_EXTENT_LY;
                self.needs_render = true;
            }
        });
});
```

Then handle the platform output and finish the egui frame:

```rust
// Handle platform output (cursor changes, clipboard, etc.)
if let Some(window) = &self.window {
    self.egui_state.as_mut().unwrap().handle_platform_output(
        &window,
        full_output.platform_output,
    );
}
```

**IMPORTANT**: Variables accessed inside the egui closure (like `self.params`, `self.exposure`, etc.) are captured by `&mut self` since `redraw()` takes `&mut self`. But the `save_snapshot` call needs `&self` — inside the closure `ctx.run()`, you only have `&egui::Context`. Use `let needs_screenshot = ...` pattern: set a flag inside the closure, check it after `ctx.run()` returns.

Refactor the snapshot trigger:

```rust
let mut request_screenshot = false;
let mut request_render_from_ui = false;

let full_output = self.egui_ctx.run(raw_input, |ctx| {
    egui::SidePanel::right("galaxy_controls")
        // ...
        .show(ctx, |ui| {
            // ... all the sliders ...
            if ui.button("Save Screenshot").clicked() {
                request_screenshot = true;
            }
            if ui.button("Reset Camera").clicked() {
                self.center_x = 0.0;
                self.center_y = 0.0;
                self.extent_ly = INITIAL_EXTENT_LY;
                self.needs_render = true;
            }
        });
});

if request_screenshot {
    if let (Some(device), Some(queue)) = (self.device.as_ref(), self.queue.as_ref()) {
        self.save_snapshot(device, queue);
    }
}
```

**Verify**: `cargo check` → exit 0. Expect unused-variable warnings for `request_render_from_ui` (not yet wired — reserve it).

### Step 6: Render the egui UI in the render pass

After building the UI (Step 5) and before `queue.submit()`, add the egui render call using the SAME encoder and frame view. The egui pass uses `LoadOp::Load` to preserve the galaxy content underneath.

The rendering code replaces the current single-pass submit+present block. The current block (approximately lines 355–385):

```rust
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rpass.set_pipeline(pipeline);
            rpass.set_bind_group(0, bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
        frame.present();
```

Becomes:

```rust
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        // Pass 1: galaxy display (clears to black, draws fullscreen quad)
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("galaxy-display"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rpass.set_pipeline(pipeline);
            rpass.set_bind_group(0, bind_group, &[]);
            rpass.draw(0..3, 0..1);
        } // rpass dropped here

        // ── Build egui UI ──────────────────────────────────
        // (Insert Step 5 code here — egui_ctx.run() + SidePanel)

        // Prepare egui meshes for rendering
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.render_w, self.render_h],
            pixels_per_point: window.scale_factor() as f32,
        };
        let tessellated = self.egui_ctx.tessellate(
            full_output.shapes,
            full_output.pixels_per_point,
        );
        let egui_renderer = self.egui_renderer.as_mut().unwrap();
        egui_renderer.update_buffers(device, queue, &mut encoder, &tessellated, &screen_descriptor);

        // Pass 2: egui (Load to preserve galaxy underneath)
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,  // preserve galaxy content
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            egui_renderer.render(&mut rpass, &tessellated, &screen_descriptor);
        }

        // Submit both passes at once
        queue.submit(Some(encoder.finish()));
        frame.present();

        // ── Screenshot (if requested by UI) ─────
        if request_screenshot {
            // Use the self.device/self.queue reference from the destructure above
            // ... but they may have moved. Re-borrow:
            let dev = self.device.as_ref().unwrap();
            let q = self.queue.as_ref().unwrap();
            self.save_snapshot(dev, q);
        }
```

**Important note on variable lifetimes**: The destructure `let (surface, device, queue, config, pipeline, bind_group, texture, gpu_compute) = (...)` at the top of `redraw()` borrows these immutably. The `queue` and `device` variables from this destructure are used in the egui render calls. Make sure they're still in scope. The destructure already happens at line ~326, so all variables are available.

However, the `window` variable is NOT in that destructure — it's `self.window.as_ref().unwrap()`. Add it to the destructure or access it inline. Existing code at lines ~318-325:

```rust
        let (surface, device, queue, config, pipeline, bind_group, texture, gpu_compute) = (
            self.surface.as_ref().unwrap(),
            self.device.as_ref().unwrap(),
            self.queue.as_ref().unwrap(),
            self.config.as_ref().unwrap(),
            self.render_pipeline.as_ref().unwrap(),
            self.bind_group.as_ref().unwrap(),
            self.texture.as_ref().unwrap(),
            self.gpu_compute.as_ref().unwrap(),
        );
```

Also add `window`:

```rust
        let window = self.window.as_ref().unwrap();
```

Then use `window` (not `self.window`) everywhere below. Update the destructure to include window.

**Verify**: `cargo check` → exit 0

### Step 7: Add frame timing display

Add a small overlay in the bottom-right corner showing the last frame time. Add this inside the `egui::SidePanel::right(...)` closure, at the bottom of the panel:

```rust
            // ── Frame timing ──
            ui.separator();
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.label(egui::RichText::new(format!(
                    "Frame: {:.1} ms",
                    ui.ctx().input(|i| i.unstable.frame_latency.map_or(0.0, |d| d * 1000.0))
                )).color(egui::Color32::GRAY).size(11.0));
            });
```

**Verify**: `cargo check` → exit 0

### Step 8: Run full verification suite

**Verify**:

- `cargo clippy -- -D warnings` → exit 0, no warnings
- `cargo test` → 21 tests pass (all existing tests; no new tests needed for UI code)
- `cargo build` → exit 0

Do a visual smoke test:

- Launch the app, confirm the sidebar appears on the right
- Change a slider (e.g. arm_count) and confirm the galaxy re-renders with the new parameter
- Switch presets (Milky Way ↔ NGC 628) and confirm the galaxy changes
- Click "Save Screenshot" and confirm `galaxy.png` is written
- Pan/zoom on the galaxy (outside the sidebar) and confirm it works
- Drag on the sidebar — pan should be suppressed
- Adjust exposure via arrow keys (when not focused on an egui text field) and confirm it works

## Test plan

No new unit tests — the egui UI is visual, and the GPU rendering path is unchanged. The existing 21 tests (`cargo test`) must continue to pass. The visual smoke test above is the functional verification.

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0 (no new warnings introduced)
- [ ] `cargo test` exits 0 with all 21 existing tests passing
- [ ] `cargo build` exits 0
- [ ] `Cargo.toml` contains `egui`, `egui-wgpu`, `egui-winit` dependencies
- [ ] `App` struct has `egui_ctx`, `egui_state`, `egui_renderer` fields
- [ ] `App::init()` creates and stores the egui state and renderer
- [ ] `App::redraw()` renders egui after the galaxy display pass, on the same encoder
- [ ] `App::window_event()` forwards events to egui and gates galaxy input behind `wants_pointer_input()` / `wants_keyboard_input()`
- [ ] Sidebar shows: preset selector, 12 parameter sliders, exposure/contrast sliders, screenshot button, reset camera button
- [ ] No files outside the in-scope list are modified (`git diff --stat` against scope)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back if:

- The code at the locations in "Current state" doesn't match the excerpts (the codebase has drifted since this plan was written).
- `egui`, `egui-wgpu`, or `egui-winit` fails to resolve at any version 0.31 through 0.33. This likely means a toolchain/dependency incompatibility — report the exact error.
- `egui_winit::State::new()` or `egui_wgpu::Renderer::new()` has a different signature than the one in Step 3 and you cannot determine the correct signature from `cargo doc`.
- The double-render-pass approach (two `begin_render_pass` calls on one encoder, Load+Store then Load+Store) produces visual artifacts (flickering, blank egui panel, galaxy disappearing). Report what you observe.
- The egui panel renders but resizing the window or the panel causes a crash.
- `cargo clippy -- -D warnings` produces a warning you cannot fix without touching out-of-scope files or degrading the code quality.
- A step's verification fails twice.

## Maintenance notes

- If new `GalaxyParams` fields are added, they need slider entries in the sidebar UI (Step 5).
- If new presets are added to `src/galaxy.rs`, they need entries in the preset `ComboBox` match in Step 5. Consider refactoring to an enum (e.g. `enum GalaxyPreset { MilkyWay, Ngc628 }`) with a `fn to_params() -> GalaxyParams` method — this is cleaner than the disk_scale_length hack, but deferred to keep this plan's scope bounded.
- The egui panel width (280px) and slider ranges are tuned for the current two presets. If much larger/smaller galaxies are added, slider ranges may need adjustment.
- The egui renderer holds GPU resources. If the wgpu device is lost/recreated (e.g. `SurfaceError::Lost`), the egui renderer must also be recreated. The current surface-lost handler only reconfigures the surface — add egui renderer recreation there if surface loss becomes common.
- The `egui::Context` stores style state. If the app gains theming or localization, build on `egui_ctx.set_style()` and `egui_ctx.set_visuals()`.
