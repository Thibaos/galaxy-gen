use galaxy_gen::gpu;

use crate::app::App;
use crate::ui;

/// Run a single render frame.
pub fn redraw(app: &mut App) {
    if app.surface.is_none()
        || app.device.is_none()
        || app.queue.is_none()
        || app.config.is_none()
        || app.render_pipeline.is_none()
        || app.bind_group_layout.is_none()
        || app.sampler.is_none()
        || app.gpu_compute.is_none()
    {
        return;
    }

    if app.tex_w != app.render_w || app.tex_h != app.render_h {
        app.recreate_texture();
    }

    if app.rgba_buf_w != app.render_w || app.rgba_buf_h != app.render_h {
        let device = app.device.as_ref().unwrap();
        let padded_w = app.render_w.div_ceil(64) * 64;
        let pixel_count = (padded_w * app.render_h) as usize;
        let u32_byte_size = (pixel_count * std::mem::size_of::<u32>()) as wgpu::BufferAddress;
        app.rgba_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rgba"),
            size: u32_byte_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        }));
        app.rgba_buf_w = app.render_w;
        app.rgba_buf_h = app.render_h;
        if let Some(ref mut compute) = app.gpu_compute {
            compute.invalidate_scene_bind_group();
        }
    }

    let window = app.window.as_ref().unwrap();
    let window_scale = window.scale_factor() as f32;

    let mut raw_input = app.egui_state.as_mut().unwrap().take_egui_input(window);
    let inner_size = window.inner_size();
    if let Some(vi) = raw_input
        .viewports
        .get_mut(&egui::viewport::ViewportId::default())
    {
        vi.inner_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(inner_size.width as f32, inner_size.height as f32) / window_scale,
        ));
    }
    let mut request_screenshot = false;
    let egui_ctx = app.egui_ctx.clone();
    let full_output = egui_ctx.run(raw_input, |ctx| {
        egui::SidePanel::right("galaxy_controls")
            .default_width(280.0)
            .resizable(true)
            .show(ctx, |ui| {
                request_screenshot = ui::build_sidebar(ui, app);
            });
    });

    // Generate star catalogue before destructuring to avoid borrow conflicts
    let needs_stars = app.render_mode == 1 && app.show_stars;
    if needs_stars {
        app.ensure_star_catalogue();
    }

    let (surface, device, queue, config, pipeline, bind_group, texture) = (
        app.surface.as_ref().unwrap(),
        app.device.as_ref().unwrap(),
        app.queue.as_ref().unwrap(),
        app.config.as_ref().unwrap(),
        app.render_pipeline.as_ref().unwrap(),
        app.bind_group.as_ref().unwrap(),
        app.texture.as_ref().unwrap(),
    );

    let egui_renderer = app.egui_renderer.as_mut().unwrap();
    for (id, delta) in full_output.textures_delta.set {
        egui_renderer.update_texture(device, queue, id, &delta);
    }
    for id in full_output.textures_delta.free {
        egui_renderer.free_texture(&id);
    }

    // Pre-compute camera values to avoid borrow conflicts
    let cam_pos = app.camera.position();
    let cam_target = app.camera.target;
    let render_mode = app.render_mode;
    let fov_y = app.camera.fov_y_deg;
    let show_glow = app.show_glow;

    let do_compute = render_mode != 1 || show_glow;
    if app.needs_render && do_compute {
        let rgba_buf = app.rgba_buffer.as_ref().unwrap();
        let uniform_buf = app.uniform_buffer.get_or_insert_with(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("uniforms"),
                size: std::mem::size_of::<gpu::GalaxyUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let uniform_data = gpu::GalaxyUniform::from_params(
            &app.params,
            app.render_w,
            app.render_h,
            app.extent_ly,
            app.center_x,
            app.center_y,
            app.exposure,
            app.contrast,
            render_mode,
            (cam_pos.x, cam_pos.y, cam_pos.z),
            (cam_target.x, cam_target.y, cam_target.z),
            fov_y,
            app.dust_tau,
        );
        queue.write_buffer(uniform_buf, 0, bytemuck::bytes_of(&uniform_data));

        // Ensure cached scene bind group
        let scene_bg = app.gpu_compute.as_mut().unwrap()
            .ensure_scene_bind_group(device, uniform_buf, rgba_buf);

        gpu::compute_galaxy(
            device,
            queue,
            app.gpu_compute.as_ref().unwrap(),
            rgba_buf,
            uniform_buf,
            &scene_bg,
            app.render_w,
            app.render_h,
            texture,
        );
    }

    // ── Star pass: upload catalogue + view-proj matrix ──
    if app.render_mode == 1 && app.show_stars && !app.star_catalogue.is_empty() {
        let gpu_stars = app.gpu_stars.as_ref().unwrap();
        app.write_view_proj_matrix(queue, gpu_stars);

        if !app.star_catalogue_uploaded {
            let mut header = [0u32; 2];
            header[0] = app.star_catalogue.len() as u32;
            header[1] = gpu::MAX_STARS;
            queue.write_buffer(&gpu_stars.instance_buffer, 0, bytemuck::cast_slice(&header));
            let data_bytes = bytemuck::cast_slice(&app.star_catalogue);
            queue.write_buffer(&gpu_stars.instance_buffer, 8, data_bytes);
            app.star_catalogue_uploaded = true;
        }
    }

    let frame = match surface.get_current_texture() {
        Ok(f) => f,
        Err(wgpu::SurfaceError::Lost) => {
            surface.configure(device, config);
            return;
        }
        Err(wgpu::SurfaceError::Timeout) => {
            return;
        }
        Err(e) => {
            eprintln!("surface error: {e:?}");
            return;
        }
    };
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    let star_draw_data =
        if app.render_mode == 1 && app.show_stars && !app.star_catalogue.is_empty() {
            let gpu_stars = app.gpu_stars.as_ref().unwrap();
            let n = app.star_catalogue.len();
            Some((
                &gpu_stars.pipeline,
                &gpu_stars.bind_group as &wgpu::BindGroup,
                n,
            ))
        } else {
            None
        };

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

    if let Some(window) = &app.window {
        app.egui_state
            .as_mut()
            .unwrap()
            .handle_platform_output(window, full_output.platform_output);
    }

    let screen_descriptor = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [app.render_w, app.render_h],
        pixels_per_point: window_scale,
    };
    let tessellated = app
        .egui_ctx
        .tessellate(full_output.shapes, full_output.pixels_per_point);
    let egui_renderer = app.egui_renderer.as_mut().unwrap();
    egui_renderer.update_buffers(
        device,
        queue,
        &mut encoder,
        &tessellated,
        &screen_descriptor,
    );

    {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("galaxy+egui"),
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
        })
        .forget_lifetime();

        if render_mode != 1 || show_glow {
            rpass.set_pipeline(pipeline);
            rpass.set_bind_group(0, bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }

        if let Some((stars_pipe, stars_bg, star_count)) = star_draw_data {
            rpass.set_pipeline(stars_pipe);
            rpass.set_bind_group(0, stars_bg, &[]);
            rpass.draw(0..6, 0..star_count as u32);
        }

        egui_renderer.render(&mut rpass, &tessellated, &screen_descriptor);
    }

    queue.submit(Some(encoder.finish()));
    frame.present();
    app.needs_render = false;

    if request_screenshot {
        let dev = app.device.as_ref().unwrap();
        let q = app.queue.as_ref().unwrap();
        app.save_snapshot(dev, q);
    }
}
