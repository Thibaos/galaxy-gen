use std::sync::Arc;

use galaxy_gen::galaxy::GalaxyParams;
use galaxy_gen::gpu;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::Window,
};

const INITIAL_EXTENT_LY: f64 = 512_000.0;
const ZOOM_SPEED: f64 = 1.1;
const MIN_EXTENT_LY: f64 = 10.0;
const MAX_EXTENT_LY: f64 = 2_000_000.0;

const DEFAULT_EXPOSURE: f32 = 0.25;
const DEFAULT_CONTRAST: f32 = 0.04;
const EXPOSURE_STEP: f32 = 0.02;
const CONTRAST_STEP: f32 = 0.002;

struct App {
    params: GalaxyParams,

    // wgpu
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    surface: Option<wgpu::Surface<'static>>,
    config: Option<wgpu::SurfaceConfiguration>,
    render_pipeline: Option<wgpu::RenderPipeline>,

    // display resources (recreated on resize)
    texture: Option<wgpu::Texture>,
    bind_group: Option<wgpu::BindGroup>,

    // shared across bind-group recreations
    bind_group_layout: Option<wgpu::BindGroupLayout>,
    sampler: Option<wgpu::Sampler>,

    window: Option<Arc<Window>>,

    // image dimensions
    render_w: u32,
    render_h: u32,
    tex_w: u32,
    tex_h: u32,

    // camera (galactic coords, ly)
    center_x: f64,
    center_y: f64,
    extent_ly: f64,
    needs_render: bool,

    // brightness
    exposure: f32,
    contrast: f32,

    // compute pipeline (cached)
    gpu_compute: Option<gpu::GpuCompute>,
    uniform_buffer: Option<wgpu::Buffer>,
    rgba_buffer: Option<wgpu::Buffer>,
    rgba_buf_w: u32,
    rgba_buf_h: u32,

    // egui
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,

    // mouse
    dragging: bool,
    last_mouse: (f64, f64),

    // 3D mode
    render_mode: u32,
    camera_dist: f32,
    camera_azimuth: f32,
    camera_elevation: f32,
    camera_target_x: f32,
    camera_target_y: f32,
    camera_target_z: f32,
    fov_y: f32,
    orbit_dragging: bool,

    // instanced stars
    gpu_stars: Option<gpu::GpuStars>,
    show_stars: bool,
    show_glow: bool,
    star_brightness: f32,
    star_size: f32,
    star_catalogue: Vec<gpu::StarInstance>,
    star_catalogue_dirty: bool,
}

impl App {
    fn new(params: GalaxyParams) -> Self {
        Self {
            params,
            device: None,
            queue: None,
            surface: None,
            config: None,
            render_pipeline: None,
            texture: None,
            bind_group: None,
            bind_group_layout: None,
            sampler: None,
            window: None,
            render_w: 100,
            render_h: 100,
            tex_w: 0,
            tex_h: 0,
            center_x: 0.0,
            center_y: 0.0,
            extent_ly: INITIAL_EXTENT_LY,
            needs_render: true,
            exposure: DEFAULT_EXPOSURE,
            contrast: DEFAULT_CONTRAST,
            gpu_compute: None,
            uniform_buffer: None,
            rgba_buffer: None,
            rgba_buf_w: 0,
            rgba_buf_h: 0,
            egui_ctx: egui::Context::default(),
            egui_state: None,
            egui_renderer: None,
            dragging: false,
            last_mouse: (0.0, 0.0),
            render_mode: 0,
            camera_dist: 100_000.0,
            camera_azimuth: 0.0,
            camera_elevation: std::f32::consts::FRAC_PI_2 - 0.05, // near-top-down (avoid degenerate look-at)
            camera_target_x: 0.0,
            camera_target_y: 0.0,
            camera_target_z: 0.0,
            fov_y: 45.0,
            orbit_dragging: false,
            gpu_stars: None,
            show_stars: true,
            show_glow: true,
            star_brightness: 0.3,
            star_size: 1.0,
            star_catalogue: Vec::new(),
            star_catalogue_dirty: true,
        }
    }

    fn camera_position(&self) -> (f32, f32, f32) {
        let horiz = self.camera_dist * self.camera_elevation.cos();
        let x = self.camera_target_x + horiz * self.camera_azimuth.sin();
        let y = self.camera_target_y + self.camera_dist * self.camera_elevation.sin();
        let z = self.camera_target_z + horiz * self.camera_azimuth.cos();
        (x, y, z)
    }

    fn ensure_star_catalogue(&mut self) {
        if self.star_catalogue_dirty {
            self.star_catalogue =
                gpu::generate_star_catalogue(&self.params, gpu::MAX_STARS as usize);
            // Debug: print spatial extent
            if !self.star_catalogue.is_empty() {
                let mut min_x = f32::MAX;
                let mut max_x = f32::MIN;
                let mut min_z = f32::MAX;
                let mut max_z = f32::MIN;
                for s in &self.star_catalogue {
                    min_x = min_x.min(s.pos_x);
                    max_x = max_x.max(s.pos_x);
                    min_z = min_z.min(s.pos_z);
                    max_z = max_z.max(s.pos_z);
                }
                println!(
                    "Star catalogue: {} stars, X=[{:.0}, {:.0}], Z=[{:.0}, {:.0}]",
                    self.star_catalogue.len(),
                    min_x,
                    max_x,
                    min_z,
                    max_z,
                );
            }
            self.star_catalogue_dirty = false;
        }
    }

    fn write_view_proj_matrix(&self, queue: &wgpu::Queue, gpu_stars: &gpu::GpuStars) {
        let cam_pos = self.camera_position();
        let eye = glam::Vec3::new(cam_pos.0, cam_pos.1, cam_pos.2);
        let target = glam::Vec3::new(
            self.camera_target_x,
            self.camera_target_y,
            self.camera_target_z,
        );
        let mut up = glam::Vec3::Y;

        // Avoid degenerate look-at when view direction is parallel to up.
        let dir = (target - eye).normalize();
        if dir.dot(up).abs() > 0.9999 {
            up = glam::Vec3::Z;
        }

        let view = glam::Mat4::look_at_rh(eye, target, up);
        let aspect = self.render_w as f32 / self.render_h as f32;
        let fov_rad = self.fov_y.to_radians();
        let proj = glam::Mat4::perspective_rh(fov_rad, aspect, 100.0, 1_000_000.0);
        let vp = proj * view;

        // Star params uniform (vec4 layout: brightness, aspect, star_size, _pad)
        let star_uniform: [f32; 4] = [
            self.star_brightness,
            self.render_w as f32 / self.render_h as f32,
            self.star_size,
            0.0,
        ];
        queue.write_buffer(
            &gpu_stars.brightness_buffer,
            0,
            bytemuck::cast_slice(&star_uniform),
        );

        queue.write_buffer(
            &gpu_stars.camera_buffer,
            0,
            bytemuck::cast_slice(&vp.to_cols_array()),
        );
    }
}

impl App {
    fn init(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Galaxy Gen")
                        .with_maximized(true),
                )
                .unwrap(),
        );
        let size = window.inner_size();
        self.render_w = size.width;
        self.render_h = size.height;
        self.window = Some(window.clone());

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window.clone())
            .expect("Failed to create surface");

        let adapter = pollster::block_on(wgpu::util::initialize_adapter_from_env_or_default(
            &instance,
            Some(&surface),
        ))
        .expect("No suitable GPU adapter found");

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats[0];

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
                .expect("Failed to create GPU device");

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("display"),
            source: wgpu::ShaderSource::Wgsl(include_str!("display.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("display"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("display"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("display"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(surface_format.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.config = Some(config);
        self.render_pipeline = Some(render_pipeline);
        self.bind_group_layout = Some(bind_group_layout);
        self.sampler = Some(sampler);

        self.gpu_compute = Some(gpu::GpuCompute::new(self.device.as_ref().unwrap()));

        self.gpu_stars = Some(gpu::GpuStars::new(
            self.device.as_ref().unwrap(),
            self.config.as_ref().unwrap().format,
        ));

        let egui_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::viewport::ViewportId::default(),
            &window,
            None,
            None,
            None,
        );

        let egui_renderer = egui_wgpu::Renderer::new(
            self.device.as_ref().unwrap(),
            self.config.as_ref().unwrap().format,
            None,
            1,
            false,
        );

        self.egui_state = Some(egui_state);
        self.egui_renderer = Some(egui_renderer);

        self.recreate_texture();
    }

    fn update_title(&self) {
        if let Some(window) = &self.window {
            window.set_title(&format!(
                "Galaxy Gen — exp: {:.3}  con: {:.4}",
                self.exposure, self.contrast
            ));
        }
    }

    fn recreate_texture(&mut self) {
        let device = self.device.as_ref().unwrap();
        let bgl = self.bind_group_layout.as_ref().unwrap();
        let sampler = self.sampler.as_ref().unwrap();

        let size = wgpu::Extent3d {
            width: self.render_w,
            height: self.render_h,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("galaxy"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("display"),
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });

        self.texture = Some(texture);
        self.bind_group = Some(bg);
        self.tex_w = self.render_w;
        self.tex_h = self.render_h;
        self.needs_render = true;
    }
}

impl App {
    fn redraw(&mut self) {
        if self.surface.is_none()
            || self.device.is_none()
            || self.queue.is_none()
            || self.config.is_none()
            || self.render_pipeline.is_none()
            || self.bind_group_layout.is_none()
            || self.sampler.is_none()
            || self.gpu_compute.is_none()
        {
            return;
        }

        if self.tex_w != self.render_w || self.tex_h != self.render_h {
            self.recreate_texture();
        }

        if self.rgba_buf_w != self.render_w || self.rgba_buf_h != self.render_h {
            let device = self.device.as_ref().unwrap();
            let padded_w = self.render_w.div_ceil(64) * 64;
            let pixel_count = (padded_w * self.render_h) as usize;
            let u32_byte_size = (pixel_count * std::mem::size_of::<u32>()) as wgpu::BufferAddress;
            self.rgba_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rgba"),
                size: u32_byte_size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }));
            self.rgba_buf_w = self.render_w;
            self.rgba_buf_h = self.render_h;
        }

        let window = self.window.as_ref().unwrap();
        let window_scale = window.scale_factor() as f32;

        let mut raw_input = self.egui_state.as_mut().unwrap().take_egui_input(window);
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
        let egui_ctx = self.egui_ctx.clone();
        let full_output = egui_ctx.run(raw_input, |ctx| {
            egui::SidePanel::right("galaxy_controls")
                .default_width(280.0)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.heading("Galaxy Controls");
                    ui.separator();
                    ui.label("Preset");
                    let initial_idx: usize = if self.params.disk_scale_length < 9_000.0 {
                        0
                    } else {
                        1
                    };
                    let mut preset_idx = initial_idx;
                    egui::ComboBox::from_id_salt("preset")
                        .selected_text(match preset_idx {
                            0 => "Milky Way",
                            _ => "NGC 628 (M74)",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut preset_idx, 0, "Milky Way");
                            ui.selectable_value(&mut preset_idx, 1, "NGC 628 (M74)");
                        });
                    if preset_idx != initial_idx {
                        self.params = match preset_idx {
                            0 => GalaxyParams::milky_way(),
                            _ => GalaxyParams::ngc628(),
                        };
                        self.needs_render = true;
                        self.star_catalogue_dirty = true;
                    }

                    ui.separator();
                    ui.label("Disk");
                    let mut changed = false;
                    changed |= ui
                        .add(
                            egui::Slider::new(
                                &mut self.params.disk_scale_length,
                                1_000.0..=25_000.0,
                            )
                            .text("Scale length (ly)"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut self.params.disk_scale_height, 100.0..=5_000.0)
                                .text("Scale height (ly)"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut self.params.disk_central_density, 0.0..=0.05)
                                .text("Central density"),
                        )
                        .changed();

                    // Spiral Arms sliders
                    ui.separator();
                    ui.label("Spiral Arms");
                    changed |= ui
                        .add(egui::Slider::new(&mut self.params.arm_count, 0..=8).text("Arm count"))
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut self.params.arm_pitch, 0.05..=0.8)
                                .text("Pitch angle (rad)"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut self.params.arm_concentration, 1.0..=15.0)
                                .text("Concentration"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut self.params.arm_strength, 0.0..=3.0)
                                .text("Strength"),
                        )
                        .changed();

                    // Bulge sliders
                    ui.separator();
                    ui.label("Bulge");
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut self.params.bulge_radius, 500.0..=15_000.0)
                                .text("Radius (ly)"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut self.params.bulge_central_density, 0.0..=0.2)
                                .text("Central density"),
                        )
                        .changed();

                    // Halo sliders
                    ui.separator();
                    ui.label("Halo");
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut self.params.halo_radius, 10_000.0..=200_000.0)
                                .text("Radius (ly)"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut self.params.halo_central_density, 0.0..=1e-6)
                                .text("Central density"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut self.params.halo_slope, -5.0..=-1.5)
                                .text("Slope"),
                        )
                        .changed();

                    if changed {
                        self.needs_render = true;
                    }

                    // Brightness sliders
                    ui.separator();
                    ui.label("Brightness");
                    let exp_changed = ui
                        .add(egui::Slider::new(&mut self.exposure, 0.0..=1.0).text("Exposure"))
                        .changed();
                    let con_changed = ui
                        .add(egui::Slider::new(&mut self.contrast, 0.0..=0.2).text("Contrast"))
                        .changed();
                    if exp_changed || con_changed {
                        self.update_title();
                        self.needs_render = true;
                    }

                    // 3D controls
                    ui.separator();
                    ui.label("3D View");
                    let mode_changed = ui
                        .add(egui::Checkbox::new(&mut (self.render_mode == 1), "3D Mode"))
                        .changed();
                    if mode_changed {
                        self.render_mode = if self.render_mode == 0 { 1 } else { 0 };
                        self.needs_render = true;
                    }
                    if self.render_mode == 1 {
                        let dist_changed = ui
                            .add(
                                egui::Slider::new(&mut self.camera_dist, 5_000.0..=500_000.0)
                                    .text("Distance (ly)"),
                            )
                            .changed();
                        let fov_changed = ui
                            .add(egui::Slider::new(&mut self.fov_y, 5.0..=120.0).text("FOV"))
                            .changed();
                        let glow_changed = ui
                            .add(egui::Checkbox::new(&mut self.show_glow, "Show Glow"))
                            .changed();
                        let stars_changed = ui
                            .add(egui::Checkbox::new(&mut self.show_stars, "Show Stars"))
                            .changed();
                        let brightness_changed = ui
                            .add(
                                egui::Slider::new(&mut self.star_brightness, 0.0..=2.0)
                                    .text("Star brightness"),
                            )
                            .changed();
                        let size_changed = ui
                            .add(
                                egui::Slider::new(&mut self.star_size, 0.01..=1.0)
                                    .text("Star size"),
                            )
                            .changed();
                        if dist_changed
                            || fov_changed
                            || glow_changed
                            || stars_changed
                            || brightness_changed
                            || size_changed
                        {
                            self.needs_render = true;
                        }
                    }

                    // Actions
                    ui.separator();
                    if ui.button("Save Screenshot").clicked() {
                        request_screenshot = true;
                    }
                    if ui.button("Reset Camera").clicked() {
                        self.center_x = 0.0;
                        self.center_y = 0.0;
                        self.extent_ly = INITIAL_EXTENT_LY;
                        self.needs_render = true;
                    }

                    // Frame timing
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
                });
        });

        // Generate star catalogue before destructuring to avoid borrow conflicts
        let needs_stars = self.render_mode == 1 && self.show_stars;
        if needs_stars {
            self.ensure_star_catalogue();
        }

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

        let egui_renderer = self.egui_renderer.as_mut().unwrap();
        for (id, delta) in full_output.textures_delta.set {
            egui_renderer.update_texture(device, queue, id, &delta);
        }
        for id in full_output.textures_delta.free {
            egui_renderer.free_texture(&id);
        }

        // Pre-compute camera values to avoid borrow conflicts
        let cam_pos = self.camera_position();
        let cam_target = (
            self.camera_target_x,
            self.camera_target_y,
            self.camera_target_z,
        );
        let render_mode = self.render_mode;
        let fov_y = self.fov_y;
        let show_glow = self.show_glow;

        let do_compute = render_mode != 1 || show_glow;
        if self.needs_render && do_compute {
            let rgba_buf = self.rgba_buffer.as_ref().unwrap();
            let uniform_buf = self.uniform_buffer.get_or_insert_with(|| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("uniforms"),
                    size: std::mem::size_of::<gpu::GalaxyUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            });
            let uniform_data = gpu::GalaxyUniform::from_params(
                &self.params,
                self.render_w,
                self.render_h,
                self.extent_ly,
                self.center_x,
                self.center_y,
                self.exposure,
                self.contrast,
                render_mode,
                cam_pos,
                cam_target,
                fov_y,
            );
            queue.write_buffer(uniform_buf, 0, bytemuck::bytes_of(&uniform_data));

            gpu::compute_galaxy(
                device,
                queue,
                gpu_compute,
                rgba_buf,
                uniform_buf,
                self.render_w,
                self.render_h,
                texture,
            );
            self.needs_render = false;
        }

        // ── Star pass: upload catalogue + view-proj matrix ──
        // catalogue was generated above (before destructuring) if render_mode==1 && show_stars
        if self.render_mode == 1 && self.show_stars && !self.star_catalogue.is_empty() {
            let gpu_stars = self.gpu_stars.as_ref().unwrap();

            // Write camera view-proj matrix
            self.write_view_proj_matrix(queue, gpu_stars);

            // Upload star catalogue to GPU
            // Write header: [count: u32, capacity: u32]
            let mut header = [0u32; 2];
            header[0] = self.star_catalogue.len() as u32;
            header[1] = gpu::MAX_STARS;
            queue.write_buffer(&gpu_stars.instance_buffer, 0, bytemuck::cast_slice(&header));
            // Write star data at byte offset 8
            let data_bytes = bytemuck::cast_slice(&self.star_catalogue);
            queue.write_buffer(&gpu_stars.instance_buffer, 8, data_bytes);
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

        // Pre-extract star draw data before render pass to avoid borrow conflicts
        let star_draw_data =
            if self.render_mode == 1 && self.show_stars && !self.star_catalogue.is_empty() {
                let gpu_stars = self.gpu_stars.as_ref().unwrap();
                let n = self.star_catalogue.len();
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

        if let Some(window) = &self.window {
            self.egui_state
                .as_mut()
                .unwrap()
                .handle_platform_output(window, full_output.platform_output);
        }

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.render_w, self.render_h],
            pixels_per_point: window_scale,
        };
        let tessellated = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let egui_renderer = self.egui_renderer.as_mut().unwrap();
        egui_renderer.update_buffers(
            device,
            queue,
            &mut encoder,
            &tessellated,
            &screen_descriptor,
        );

        {
            let rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
            });
            let mut rpass = rpass.forget_lifetime();

            // ── Fullscreen quad (2D mode, or 3D mode with glow enabled) ──
            if render_mode != 1 || show_glow {
                rpass.set_pipeline(pipeline);
                rpass.set_bind_group(0, bind_group, &[]);
                rpass.draw(0..3, 0..1);
            }

            // ── Draw instanced stars (3D mode + additive blend) ──
            if let Some((stars_pipe, stars_bg, star_count)) = star_draw_data {
                rpass.set_pipeline(stars_pipe);
                rpass.set_bind_group(0, stars_bg, &[]);
                rpass.draw(0..6, 0..star_count as u32);
            }

            egui_renderer.render(&mut rpass, &tessellated, &screen_descriptor);
        }

        queue.submit(Some(encoder.finish()));
        frame.present();

        if request_screenshot {
            let dev = self.device.as_ref().unwrap();
            let q = self.queue.as_ref().unwrap();
            self.save_snapshot(dev, q);
        }
    }

    fn save_snapshot(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let render_w = self.render_w;
        let render_h = self.render_h;
        let padded_w = render_w.div_ceil(64) * 64;
        let buf_size = (padded_w as u64) * (render_h as u64) * 4;

        let rgba_buf = match self.rgba_buffer.as_ref() {
            Some(b) => b,
            None => return,
        };

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging"),
            size: buf_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("snapshot-copy"),
        });
        encoder.copy_buffer_to_buffer(rgba_buf, 0, &staging, 0, buf_size);
        let idx = queue.submit(Some(encoder.finish()));

        device.poll(wgpu::Maintain::WaitForSubmissionIndex(idx));

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::Maintain::Wait);

        if let Ok(Ok(())) = rx.recv() {
            let data = slice.get_mapped_range();
            let bytes: &[u8] = &data;

            let row_bytes = (render_w as usize) * 4;
            let padded_row_bytes = (padded_w as usize) * 4;
            let mut flat = Vec::with_capacity((render_w * render_h) as usize * 4);
            for y in 0..render_h {
                let offset = (y as usize) * padded_row_bytes;
                flat.extend_from_slice(&bytes[offset..offset + row_bytes]);
            }
            drop(data);
            staging.unmap();

            if let Some(img) = image::RgbaImage::from_raw(render_w, render_h, flat) {
                match img.save("galaxy.png") {
                    Ok(_) => println!("Saved galaxy.png ({render_w}×{render_h})"),
                    Err(e) => eprintln!("Failed to save galaxy.png: {e}"),
                }
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.init(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if let (Some(egui_state), Some(window)) = (&mut self.egui_state, &self.window) {
            let response = egui_state.on_window_event(window, &event);
            if response.repaint {
                window.request_redraw();
            }
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(new_size) if new_size.width > 0 && new_size.height > 0 => {
                self.render_w = new_size.width;
                self.render_h = new_size.height;

                if let (Some(surface), Some(device)) = (self.surface.as_ref(), self.device.as_ref())
                    && let Some(config) = self.config.as_mut()
                {
                    config.width = new_size.width;
                    config.height = new_size.height;
                    surface.configure(device, config);
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state.is_pressed()
                    && !self.egui_ctx.wants_keyboard_input()
                    && let PhysicalKey::Code(key) = event.physical_key
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

            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let pressed = state == ElementState::Pressed;
                if self.render_mode == 1 && !self.egui_ctx.wants_pointer_input() {
                    self.orbit_dragging = pressed;
                } else {
                    self.dragging = pressed;
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let (cx, cy) = (position.x, position.y);
                let (lx, ly) = self.last_mouse;
                self.last_mouse = (cx, cy);

                if self.dragging && !self.egui_ctx.wants_pointer_input() {
                    let dx = cx - lx;
                    let dy = cy - ly;

                    let ly_per_px = self.extent_ly / self.render_w as f64;

                    self.center_x -= dx * ly_per_px;
                    self.center_y += dy * ly_per_px;

                    self.needs_render = true;
                }

                if self.orbit_dragging
                    && self.render_mode == 1
                    && !self.egui_ctx.wants_pointer_input()
                {
                    let dx = cx - lx;
                    let dy = cy - ly;
                    self.camera_azimuth -= dx as f32 * 0.005;
                    self.camera_elevation = (self.camera_elevation + dy as f32 * 0.005).clamp(
                        -std::f32::consts::FRAC_PI_2 + 0.01,
                        std::f32::consts::FRAC_PI_2 - 0.01,
                    );
                    self.needs_render = true;
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if self.egui_ctx.wants_pointer_input() {
                    return;
                }
                use winit::event::MouseScrollDelta;
                let scroll: f64 = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(pos) => pos.y,
                };

                if scroll == 0.0 {
                    return;
                }

                if self.render_mode == 1 {
                    let factor = if scroll > 0.0 { 1.0 / 1.1_f64 } else { 1.1_f64 };
                    self.camera_dist =
                        (self.camera_dist as f64 * factor).clamp(5_000.0, 500_000.0) as f32;
                    self.needs_render = true;
                } else {
                    let old_extent = self.extent_ly;
                    let factor = if scroll > 0.0 {
                        1.0 / ZOOM_SPEED
                    } else {
                        ZOOM_SPEED
                    };

                    self.extent_ly = (self.extent_ly * factor).clamp(MIN_EXTENT_LY, MAX_EXTENT_LY);

                    if (self.extent_ly - old_extent).abs() < 1.0 {
                        return;
                    }

                    if self.render_w > 0 && self.render_h > 0 {
                        let fx = self.last_mouse.0 / self.render_w as f64;
                        let fy = self.last_mouse.1 / self.render_h as f64;

                        let extent_delta = old_extent - self.extent_ly;
                        let y_aspect = self.render_h as f64 / self.render_w as f64;
                        self.center_x += (fx - 0.5) * extent_delta;
                        self.center_y -= (fy - 0.5) * extent_delta * y_aspect;
                    }

                    self.needs_render = true;
                }
            }

            WindowEvent::RedrawRequested => self.redraw(),

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.needs_render
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
    }
}

fn main() {
    let params = GalaxyParams::milky_way();
    let event_loop = EventLoop::new().unwrap();

    let mut app = App::new(params);
    event_loop.set_control_flow(ControlFlow::Wait);

    event_loop.run_app(&mut app).unwrap();
}
