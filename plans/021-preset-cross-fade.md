# Plan 021: Preset cross-fade transition

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

> **Drift check (run first)**:
> `git diff --stat HEAD~1..HEAD -- src/main.rs src/display.wgsl src/gpu.rs`
> If the render pipeline or display shader changed significantly since this
> plan was written, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: M
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

```rust
fade_t: f32,           // 0.0 = fully prev, 1.0 = fully current
fade_duration: f32,    // seconds (e.g. 0.8)
is_fading: bool,
prev_tex: Option<wgpu::Texture>,
```

Initialize `fade_t = 1.0`, `fade_duration = 0.8`, `is_fading = false`.

### Step 2: Trigger fade on preset change

In the egui preset dropdown handler, after switching params + setting
`needs_render = true`, also:

```rust
if preset_changed {
    // Swap textures
    if let Some(tex) = self.prev_tex.take() { /* drop old */ }
    self.prev_tex = Some(self.render_tex.take().unwrap());
    self.render_tex = Some(recreate_texture(device, render_w, render_h, ...));
    self.is_fading = true;
    self.fade_t = 0.0;
}
```

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
    if fade_t >= 1.0 {
        // ... existing color processing ...
    } else {
        let color_prev = textureSample(tex_prev, smp, in.uv);
        let color = mix(color_prev, color_cur, fade_t);
        // ... processing ...
    }
}
```

**Alternative (simpler):** If a second texture binding is too invasive,
do the blend on the CPU side by rendering into a staging buffer and
blending there. But that's slower and more complex. Stick with GPU blend.

### Step 5: Wire bindings

Add `tex_prev` binding and `fade_t` uniform buffer to the display pipeline's
bind group layout in `GpuCompute` or the display pipeline setup.

### Step 6: Validate

```bash
cargo check
cargo clippy -- -D warnings
cargo test
```

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
