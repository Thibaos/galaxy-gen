use winit::{
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
};

use crate::app::App;

const ZOOM_2D_SPEED: f64 = 1.1;
const MIN_EXTENT_LY: f64 = 10.0;
const MAX_EXTENT_LY: f64 = 2_000_000.0;

/// Tracks mouse state for panning and orbiting.
pub struct InputState {
    pub last_mouse_x: f64,
    pub last_mouse_y: f64,
    pub dragging: bool,
    pub orbit_dragging: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            last_mouse_x: 0.0,
            last_mouse_y: 0.0,
            dragging: false,
            orbit_dragging: false,
        }
    }
}

/// Process a winit window event, mutating app state as needed.
pub fn handle_window_event(
    event: &WindowEvent,
    app: &mut App,
    event_loop: &ActiveEventLoop,
    window: &winit::window::Window,
) {
    let egui_response = app
        .egui_state
        .as_mut()
        .map(|s| s.on_window_event(window, event))
        .unwrap_or_default();
    if egui_response.repaint {
        window.request_redraw();
    }

    match event {
        WindowEvent::CloseRequested => event_loop.exit(),

        WindowEvent::Resized(new_size) if new_size.width > 0 && new_size.height > 0 => {
            app.render_w = new_size.width;
            app.render_h = new_size.height;

            if let (Some(surface), Some(device)) = (app.surface.as_ref(), app.device.as_ref())
                && let Some(config) = app.config.as_mut()
            {
                config.width = new_size.width;
                config.height = new_size.height;
                surface.configure(device, config);
            }
        }

        WindowEvent::MouseInput {
            state,
            button: MouseButton::Left,
            ..
        } => {
            let pressed = *state == ElementState::Pressed;
            if app.is_3d && !app.egui_ctx.wants_pointer_input() {
                app.input.orbit_dragging = pressed;
            } else {
                app.input.dragging = pressed;
            }
        }

        WindowEvent::CursorMoved { position, .. } => {
            let (cx, cy) = (position.x, position.y);
            let (lx, ly) = (app.input.last_mouse_x, app.input.last_mouse_y);
            app.input.last_mouse_x = cx;
            app.input.last_mouse_y = cy;

            if app.input.dragging && !app.egui_ctx.wants_pointer_input() {
                let dx = cx - lx;
                let dy = cy - ly;

                let ly_per_px = app.extent_ly / app.render_w as f64;

                app.center_x -= dx * ly_per_px;
                app.center_y += dy * ly_per_px;

                app.needs_render = true;
            }

            if app.input.orbit_dragging
                && app.is_3d
                && !app.egui_ctx.wants_pointer_input()
            {
                let dx = cx - lx;
                let dy = cy - ly;
                app.camera.orbit(
                    dx as f32 * crate::camera::ORBIT_SENSITIVITY,
                    dy as f32 * crate::camera::ORBIT_SENSITIVITY,
                );
                app.needs_render = true;
            }
        }

        WindowEvent::MouseWheel { delta, .. } => {
            if app.egui_ctx.wants_pointer_input() {
                return;
            }
            use winit::event::MouseScrollDelta;
            let scroll: f64 = match delta {
                MouseScrollDelta::LineDelta(_, y) => *y as f64,
                MouseScrollDelta::PixelDelta(pos) => pos.y,
            };

            if scroll == 0.0 {
                return;
            }

            if app.is_3d {
                let factor = if scroll > 0.0 {
                    1.0 / crate::camera::ZOOM_SPEED
                } else {
                    crate::camera::ZOOM_SPEED
                } as f32;
                app.camera.zoom(factor);
                app.needs_render = true;
            } else {
                let old_extent = app.extent_ly;
                let factor = if scroll > 0.0 {
                    1.0 / ZOOM_2D_SPEED
                } else {
                    ZOOM_2D_SPEED
                };

                app.extent_ly =
                    (app.extent_ly * factor).clamp(MIN_EXTENT_LY, MAX_EXTENT_LY);

                if (app.extent_ly - old_extent).abs() < 1.0 {
                    return;
                }

                if app.render_w > 0 && app.render_h > 0 {
                    let fx = app.input.last_mouse_x / app.render_w as f64;
                    let fy = app.input.last_mouse_y / app.render_h as f64;

                    let extent_delta = old_extent - app.extent_ly;
                    let y_aspect = app.render_h as f64 / app.render_w as f64;
                    app.center_x += (fx - 0.5) * extent_delta;
                    app.center_y -= (fy - 0.5) * extent_delta * y_aspect;
                }

                app.needs_render = true;
            }
        }

        _ => {}
    }
}
