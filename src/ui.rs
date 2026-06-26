use galaxy_gen::galaxy::GalaxyParams;

use crate::app::{App, INITIAL_EXTENT_LY};
use crate::camera;

const MIN_EXTENT_LY: f64 = 10.0;
const MAX_EXTENT_LY: f64 = 2_000_000.0;

/// Build the egui sidebar panel.
/// Returns `true` if the screenshot button was clicked.
pub fn build_sidebar(ui: &mut egui::Ui, app: &mut App) -> bool {
    ui.heading("Galaxy Controls");
    ui.separator();
    ui.label("Preset");

    type PresetEntry = (fn() -> GalaxyParams, &'static str);
    let presets: &[PresetEntry] = &[
        (GalaxyParams::milky_way, "Milky Way"),
        (GalaxyParams::ngc628, "NGC 628 (M74)"),
        (GalaxyParams::m31, "M31 (Andromeda)"),
        (GalaxyParams::m51, "M51 (Whirlpool)"),
        (GalaxyParams::m101, "M101 (Pinwheel)"),
    ];
    let current_idx = presets
        .iter()
        .position(|(maker, _)| {
            let p = maker();
            app.params.disk_scale_length == p.disk_scale_length
                && app.params.disk_scale_height == p.disk_scale_height
        })
        .unwrap_or(0);
    let mut preset_idx = current_idx;

    egui::ComboBox::from_id_salt("preset")
        .selected_text(presets[preset_idx].1)
        .show_ui(ui, |ui| {
            for (idx, (_maker, name)) in presets.iter().enumerate() {
                ui.selectable_value(&mut preset_idx, idx, *name);
            }
        });
    if preset_idx != current_idx {
        app.params = (presets[preset_idx].0)();
        app.needs_render = true;
        app.star_catalogue_dirty = true;
    }

    ui.separator();
    ui.label("Brightness");
    let brightness_changed = ui
        .add(
            egui::Slider::new(&mut app.star_brightness_2d, 0.0..=2.0)
                .text("Star brightness (2D)"),
        )
        .changed();
    if brightness_changed {
        app.needs_render = true;
    }

    ui.separator();
    ui.label("3D View");
    if ui.add(egui::Checkbox::new(&mut app.is_3d, "3D Mode")).changed() {
        if app.is_3d {
            // 2D → 3D: map center/extent to camera
            app.camera.target = glam::Vec3::new(
                app.center_x as f32, 0.0, app.center_y as f32
            );
            let fov_rad = app.camera.fov_y_deg.to_radians();
            app.camera.dist = (app.extent_ly as f32 / (2.0 * (0.5 * fov_rad).tan()))
                .clamp(camera::CAMERA_DIST_MIN, camera::CAMERA_DIST_MAX);
            app.camera.azimuth = 0.0;
            app.camera.elevation = std::f32::consts::FRAC_PI_2 - 0.05;
        } else {
            // 3D → 2D: project camera FOV onto XZ plane
            let fov_rad = app.camera.fov_y_deg.to_radians();
            app.extent_ly = (2.0_f64 * app.camera.dist as f64 * (0.5 * fov_rad).tan() as f64)
                .clamp(MIN_EXTENT_LY, MAX_EXTENT_LY);
            app.center_x = app.camera.target.x as f64;
            app.center_y = app.camera.target.z as f64;
        }
        app.needs_render = true;
    }
    if app.is_3d {
        let dist_changed = ui
            .add(
                egui::Slider::new(&mut app.camera.dist, 5_000.0..=500_000.0)
                    .text("Distance (ly)"),
            )
            .changed();
        let fov_changed = ui
            .add(egui::Slider::new(&mut app.camera.fov_y_deg, 5.0..=120.0).text("FOV"))
            .changed();
        if dist_changed || fov_changed {
            app.needs_render = true;
        }
    }
    let size_changed = ui
        .add(
            egui::Slider::new(&mut app.star_size, 0.01..=0.5).text("Star size"),
        )
        .changed();
    if size_changed {
        app.needs_render = true;
    }

    ui.separator();
    let mut request_screenshot = false;
    if ui.button("Save Screenshot").clicked() {
        request_screenshot = true;
    }
    if ui.button("Reset Camera").clicked() {
        app.center_x = 0.0;
        app.center_y = 0.0;
        app.extent_ly = INITIAL_EXTENT_LY;
        app.needs_render = true;
    }

    ui.separator();
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.label(
            egui::RichText::new(format!(
                "Frame: {:.1} ms",
                ui.ctx().input(|i| i.unstable_dt * 1000.0)
            ))
            .color(egui::Color32::GRAY)
            .size(11.0),
        );
    });

    request_screenshot
}
