use galaxy_gen::galaxy::GalaxyParams;
use galaxy_gen::gpu;

use crate::camera::Camera;
use crate::input::InputState;

/// Application state and GPU resources.
pub struct App {
    pub params: GalaxyParams,

    // wgpu
    pub device: Option<wgpu::Device>,
    pub queue: Option<wgpu::Queue>,
    pub surface: Option<wgpu::Surface<'static>>,
    pub config: Option<wgpu::SurfaceConfiguration>,
    pub render_pipeline: Option<wgpu::RenderPipeline>,

    // display resources (recreated on resize)
    pub texture: Option<wgpu::Texture>,
    pub bind_group: Option<wgpu::BindGroup>,

    // shared across bind-group recreations
    pub bind_group_layout: Option<wgpu::BindGroupLayout>,
    pub sampler: Option<wgpu::Sampler>,
    pub display_params_buffer: Option<wgpu::Buffer>,

    pub window: Option<std::sync::Arc<winit::window::Window>>,

    // image dimensions
    pub render_w: u32,
    pub render_h: u32,
    pub tex_w: u32,
    pub tex_h: u32,

    // camera (galactic coords, ly) — 2D pan/zoom
    pub center_x: f64,
    pub center_y: f64,
    pub extent_ly: f64,
    pub needs_render: bool,

    // compute pipeline (cached)
    pub gpu_compute: Option<gpu::GpuCompute>,
    pub uniform_buffer: Option<wgpu::Buffer>,
    pub rgba_buffer: Option<wgpu::Buffer>,
    pub rgba_buf_w: u32,
    pub rgba_buf_h: u32,

    // egui
    pub egui_ctx: egui::Context,
    pub egui_state: Option<egui_winit::State>,
    pub egui_renderer: Option<egui_wgpu::Renderer>,

    // input state
    pub input: InputState,

    // 3D mode
    pub is_3d: bool,
    pub camera: Camera,

    // instanced stars
    pub gpu_stars: Option<gpu::GpuStars>,
    pub star_brightness_2d: f32,
    pub star_size: f32,
    pub star_catalogue: Vec<gpu::StarInstance>,
    pub star_catalogue_dirty: bool,
    pub star_catalogue_uploaded: bool,
}

pub const INITIAL_EXTENT_LY: f64 = 512_000.0;

impl App {
    pub fn new(params: GalaxyParams) -> Self {
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
            display_params_buffer: None,
            window: None,
            render_w: 100,
            render_h: 100,
            tex_w: 0,
            tex_h: 0,
            center_x: 0.0,
            center_y: 0.0,
            extent_ly: INITIAL_EXTENT_LY,
            needs_render: true,
            gpu_compute: None,
            uniform_buffer: None,
            rgba_buffer: None,
            rgba_buf_w: 0,
            rgba_buf_h: 0,
            egui_ctx: egui::Context::default(),
            egui_state: None,
            egui_renderer: None,
            input: InputState::default(),
            is_3d: false,
            camera: Camera::default(),
            gpu_stars: None,
            star_brightness_2d: 1.0,
            star_size: 0.05,
            star_catalogue: Vec::new(),
            star_catalogue_dirty: true,
            star_catalogue_uploaded: false,
        }
    }

    pub fn init(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = std::sync::Arc::new(
            event_loop
                .create_window(
                    winit::window::Window::default_attributes()
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

        let adapter =
            pollster::block_on(wgpu::util::initialize_adapter_from_env_or_default(
                &instance,
                Some(&surface),
            ))
            .expect("No suitable GPU adapter found");

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats[0];

        let (device, queue) = pollster::block_on(
            adapter.request_device(&wgpu::DeviceDescriptor::default(), None),
        )
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

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("display"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let render_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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

        self.render_pipeline = Some(render_pipeline);
        self.bind_group_layout = Some(bind_group_layout);
        self.sampler = Some(sampler);

        // Display params buffer (surface_is_bgra flag) — create before moving device/queue
        let display_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("display_params"),
            size: 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let surface_is_bgra = surface_format == wgpu::TextureFormat::Bgra8Unorm;
        queue.write_buffer(&display_params_buffer, 0, &(surface_is_bgra as u32).to_ne_bytes());
        self.display_params_buffer = Some(display_params_buffer);

        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.config = Some(config);
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


    pub fn recreate_texture(&mut self) {
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.display_params_buffer.as_ref().unwrap().as_entire_binding(),
                },
            ],
        });

        self.texture = Some(texture);
        self.bind_group = Some(bg);
        self.tex_w = self.render_w;
        self.tex_h = self.render_h;
        self.needs_render = true;
    }

    pub fn ensure_star_catalogue(&mut self) {
        if self.star_catalogue_dirty {
            self.star_catalogue =
                gpu::generate_star_catalogue(&self.params, gpu::MAX_STARS as usize);
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
            self.star_catalogue_uploaded = false;
        }
    }


    pub fn save_snapshot(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
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
