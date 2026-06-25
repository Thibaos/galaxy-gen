# Plan 021: Preset cross-fade transition

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat 1647567..HEAD -- src/main.rs src/display.wgsl src/gpu.rs`
> If the render pipeline or display shader changed significantly since this
> plan was written, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: M
- **Risk**: MEDIUM
- **Category**: direction (polish)
- **Depends on**: none
- **Planned at**: commit `1647567`, 2026-06-25

## Why this matters

Switching galaxy presets currently causes an instant jarring jump — one
frame shows the old galaxy, the next frame shows the new one. A 0.5–1s
cross-fade between the two renders would feel polished and make the preset
switcher a satisfying demo feature.

## Design

Two render textures (current + previous), alpha-blended in the display
shader during the transition.

**State machine:**

```
IDLE ──(preset switch)──► FADING { t: 0.0..1.0 }
                               │
                    (each frame: t += dt)
                               │
                    (t >= 1.0) ▼
                          IDLE (swap textures, old texture dropped)
```

**Texture management:**

- `render_tex_0` / `render_tex_1` — ping-pong texture pair
- `current_tex` — index into the pair (0 or 1)
- On preset switch: `prev_tex = current_tex`, `current_tex = 1 - prev_tex`,
  compute new galaxy into `render_tex[current_tex]`, start fade
- Display shader blends: `mix(prev, current, fade_t)`

## Current state

**Render texture field** on `App` (~line 60):
```rust
render_tex: Option<wgpu::Texture>,  // currently a single texture
```
This becomes the "current" in the ping-pong pair. `prev_tex` is new.

**Recreate helper** — a free function in `main.rs` that allocates a texture:
```rust
fn recreate_texture(device: &wgpu::Device, w: u32, h: u32,
    format: wgpu::TextureFormat) -> wgpu::Texture { ... }
```
The plan reuses this to create the new "current" texture on preset switch.

**Current display shader** — `src/display.wgsl` (full file, ~35 lines):
```wgsl
struct VertexOutput { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(...) -> VertexOutput { ... }
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(tex, smp, in.uv);
    return vec4(color.b, color.g, color.r, color.a);  // Bgra8 swap
}
```
Bindings 0 and 1 are taken. New bindings start at 2 (prev texture) and 3 (fade uniform).

**Preset dropdown** — in the egui sidebar (~line 900):
```rust
if ui.selectable_value(&mut self.current_preset, GalaxyPreset::MilkyWay, "Milky Way").clicked()
    || ui.selectable_value(...).clicked()
{
    self.params = GalaxyParams::from_preset(self.current_preset);
    self.star_catalogue_dirty = true;
    self.needs_render = true;
}
```
The `if preset_changed { ... }` block from step 2 goes inside the `.clicked()`
branch, after `self.needs_render = true;`. The `render_w`, `render_h`,
`surface_format`, and `device` variables are available in `redraw()` scope
via local bindings destructured from `self`.

## Commands

| Purpose | Command                        | Expected              |
|---------|--------------------------------|-----------------------|
| Check   | `cargo check`                  | exit 0, no errors     |
| Lint    | `cargo clippy -- -D warnings`  | exit 0, no warnings   |
| Tests   | `cargo test`                   | all pass              |

## Scope

**In scope:**
- `src/main.rs` — add texture pair, fade state, trigger on preset change
- `src/display.wgsl` — add second texture binding + blend uniform
- `src/gpu.rs` — extend `DisplayPipeline` with second texture + blend params

**Out of scope:**
- Cross-fading individual parameter sliders (only preset changes)
- Fading the instanced star pass (compute texture only)
- Audio or other transition effects

## Steps

### Step 1: Add cross-fade state to App

Add to `App` struct near `render_tex`:
```rust
fade_t: f32,
fade_duration: f32,
is_fading: bool,
prev_tex: Option<wgpu::Texture>,
```

In `App::new()`:
```rust
fade_t: 1.0,
fade_duration: 0.8,
is_fading: false,
prev_tex: None,
```

**Verify**: `cargo check` → exit 0

### Step 2: Trigger fade on preset change

In the egui preset dropdown handler, after switching params + setting
`needs_render = true`, also:

```rust
if preset_changed {
    // Swap textures: current becomes previous, create new current
    let old_render_tex = std::mem::replace(
        &mut self.render_tex,
        Some(recreate_texture(device, render_w, render_h, surface_format)),
    );
    // Drop the old prev_tex (if any) and move current → prev
    self.prev_tex = old_render_tex;
    self.is_fading = true;
    self.fade_t = 0.0;
}
```

`std::mem::replace` atomically swaps: new texture goes into `render_tex`,
old texture is returned and assigned to `prev_tex`. Previous `prev_tex`
is dropped via `Option` reassignment.

### Step 3: Advance fade each frame

In `redraw()`, before rendering:

```rust
if self.is_fading {
    self.fade_t += dt / self.fade_duration;
    if self.fade_t >= 1.0 {
        self.fade_t = 1.0;
        self.is_fading = false;
        self.prev_tex = None; // free old texture
    }
}
```

### Step 4: Update display shader

Add to `display.wgsl`:

```wgsl
@group(0) @binding(2) var tex_prev: texture_2d<f32>;
@group(0) @binding(3) var<uniform> fade_t: f32;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color_cur = textureSample(tex, smp, in.uv);
    let color_prev = textureSample(tex_prev, smp, in.uv);
    let color = mix(color_prev, color_cur, fade_t);
    // Apply R↔B swap (surface format compensation) on the blended result
    // ... existing color processing ...
}
```

**Alternative (simpler):** If a second texture binding is too invasive,
do the blend on the CPU side by rendering into a staging buffer and
blending there. But that's slower and more complex. Stick with GPU blend.

### Step 5: Wire bindings to DisplayPipeline

In `src/main.rs`, update the `DisplayPipeline` struct and initialization:

**A)** Add `prev_tex: wgpu::Texture` and `fade_buffer: wgpu::Buffer` fields.

**B)** In the bind group layout, add entries for binding 2 (texture_2d<f32>)
and binding 3 (uniform buffer, 4 bytes).

**C)** In the bind group, add entries pointing to `prev_tex` (create a
`textureView` for it) and `fade_buffer`.

**D)** Each frame, write `self.fade_t` to `fade_buffer` via
`queue.write_buffer(&self.display.fade_buffer, 0, &self.fade_t.to_ne_bytes());`.

**Verify**: `cargo check` → exit 0 (no bind group layout mismatches)

### Step 5B: Write fade_t each frame

In `redraw()`, after the fade advance logic from step 3, write the current
fade value to the GPU buffer:

```rust
queue.write_buffer(&self.display.fade_buffer, 0, &self.fade_t.to_ne_bytes());
```

**Verify**: `cargo check` → exit 0

### Step 6: Full validation

```bash
cargo clippy -- -D warnings
cargo test
```

All 45 tests pass. Manual test: switch presets, verify ~0.8s cross-fade.

## Git workflow

- Branch: `advisor/021-preset-cross-fade`
- Commit message: `feat: cross-fade between presets on switch`
- Do NOT push or open a PR unless instructed.

## Test plan

- All 45 existing tests pass
- Manual test: switch presets, verify smooth ~0.8s cross-fade with no
  flicker, no frame drops, no texture leaks
- Switch presets rapidly — each switch should start a new fade from the
  current mid-fade state (no crash)

## Done criteria

- [ ] `cargo check` exits 0
- [ ] `cargo clippy -- -D warnings` exits 0
- [ ] `cargo test` exits 0 (all 45 pass)
- [ ] Preset switch triggers a visible cross-fade
- [ ] Previous texture is freed after fade completes
- [ ] Rapid preset switching doesn't crash
- [ ] `plans/README.md` updated

## STOP conditions

- The code at the locations in "Design" doesn't match the current codebase
  (the codebase has drifted since this plan was written).
- Display shader compilation fails (binding count mismatch)
- Texture allocation fails on low-VRAM devices
- Flicker or artifacts during fade
- Any existing test fails

## Maintenance notes

- The star instanced pass is NOT faded — only the compute texture is.
  Stars will still jump instantly. This is acceptable because stars are
  bright points; fading them would look like dimming/flickering during
  the transition, which is worse than an instant update.
- `fade_duration = 0.8` is a reasonable default. Consider exposing it as
  an egui slider in the future.
