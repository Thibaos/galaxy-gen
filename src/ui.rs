use galaxy_gen::galaxy::GalaxyParams;

use crate::app::{App, INITIAL_EXTENT_LY};

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
    ui.label("Disk");
    let mut changed = false;

    changed |= ui
        .add(
            egui::Slider::new(&mut app.params.disk_scale_length, 1_000.0..=25_000.0)
                .text("Scale length (ly)"),
        )
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(&mut app.params.disk_scale_height, 100.0..=5_000.0)
                .text("Scale height (ly)"),
        )
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(&mut app.params.disk_central_density, 0.0..=0.05)
                .text("Central density"),
        )
        .changed();

    ui.separator();
    ui.label("Spiral Arms");
    changed |= ui
        .add(egui::Slider::new(&mut app.params.arm_count, 0..=8).text("Arm count"))
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(&mut app.params.arm_pitch, 0.05..=0.8)
                .text("Pitch angle (rad)"),
        )
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(&mut app.params.arm_concentration, 1.0..=15.0)
                .text("Concentration"),
        )
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(&mut app.params.arm_strength, 0.0..=3.0)
                .text("Strength"),
        )
        .changed();

    ui.separator();
    ui.label("Bulge");
    changed |= ui
        .add(
            egui::Slider::new(&mut app.params.bulge_radius, 500.0..=15_000.0)
                .text("Radius (ly)"),
        )
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(&mut app.params.bulge_central_density, 0.0..=0.2)
                .text("Central density"),
        )
        .changed();

    ui.separator();
    ui.label("Halo");
    changed |= ui
        .add(
            egui::Slider::new(&mut app.params.halo_radius, 10_000.0..=200_000.0)
                .text("Radius (ly)"),
        )
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(&mut app.params.halo_central_density, 0.0..=1e-6)
                .text("Central density"),
        )
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(&mut app.params.halo_slope, -5.0..=-1.5)
                .text("Slope"),
        )
        .changed();

    if changed {
        app.needs_render = true;
    }

    ui.separator();
    ui.label("Brightness");
    let exp_changed = ui
        .add(egui::Slider::new(&mut app.exposure, 0.0..=1.0).text("Exposure"))
        .changed();
    let con_changed = ui
        .add(egui::Slider::new(&mut app.contrast, 0.0..=0.2).text("Contrast"))
        .changed();
    if exp_changed || con_changed {
        app.update_title();
        app.needs_render = true;
    }

    ui.separator();
    ui.label("3D View");
    let mode_changed = ui
        .add(egui::Checkbox::new(&mut (app.render_mode == 1), "3D Mode"))
        .changed();
    if mode_changed {
        app.render_mode = if app.render_mode == 0 { 1 } else { 0 };
        app.needs_render = true;
    }
    if app.render_mode == 1 {
        let dist_changed = ui
            .add(
                egui::Slider::new(&mut app.camera.dist, 5_000.0..=500_000.0)
                    .text("Distance (ly)"),
            )
            .changed();
        let fov_changed = ui
            .add(egui::Slider::new(&mut app.camera.fov_y_deg, 5.0..=120.0).text("FOV"))
            .changed();
        let glow_changed = ui
            .add(egui::Checkbox::new(&mut app.show_glow, "Show Glow"))
            .changed();
        let stars_changed = ui
            .add(egui::Checkbox::new(&mut app.show_stars, "Show Stars"))
            .changed();
        let brightness_changed = ui
            .add(
                egui::Slider::new(&mut app.star_brightness, 0.0..=2.0)
                    .text("Star brightness"),
            )
            .changed();
        let size_changed = ui
            .add(
                egui::Slider::new(&mut app.star_size, 0.01..=1.0)
                    .text("Star size"),
            )
            .changed();
        let dust_changed = ui
            .add(
                egui::Slider::new(&mut app.dust_tau, 0.0..=1.0)
                    .text("Dust τ"),
            )
            .changed();
        ui.label(format!(
            "Catalogue: {} stars {}",
            app.star_catalogue.len(),
            if app.star_catalogue_uploaded {
                "(synced)"
            } else {
                "(dirty)"
            }
        ));
        if dist_changed
            || fov_changed
            || glow_changed
            || stars_changed
            || dust_changed
            || brightness_changed
            || size_changed
        {
            app.needs_render = true;
        }
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
