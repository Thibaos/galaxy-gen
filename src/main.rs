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

// ── App ─────────────────────────────────────────────────────

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

    // ── image dimensions ───────────────────
    render_w: u32,
    render_h: u32,
    tex_w: u32,
    tex_h: u32,

    // ── camera (galactic coords, ly) ───────
    center_x: f64,
    center_y: f64,
    extent_ly: f64,
    needs_render: bool,

    // ── brightness ────────────────────────
    exposure: f32,
    contrast: f32,

    // ── compute pipeline (cached) ────────
    gpu_compute: Option<gpu::GpuCompute>,
    uniform_buffer: Option<wgpu::Buffer>,
    rgba_buffer: Option<wgpu::Buffer>,
    rgba_buf_w: u32,
    rgba_buf_h: u32,

    // ── mouse ─────────────────────────────
    dragging: bool,
    last_mouse: (f64, f64),
    saved_startup_image: bool,
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
            dragging: false,
            last_mouse: (0.0, 0.0),
            saved_startup_image: false,
        }
    }
}

// ── init ────────────────────────────────────────────────────

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

        // create cached compute pipeline resources
        self.gpu_compute = Some(gpu::GpuCompute::new(self.device.as_ref().unwrap()));

        // create the initial texture + bind-group
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

    /// Allocate (or re-allocate) the texture and bind-group at `render_w × render_h`.
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

// ── redraw ───────────────────────────────────────────────────

impl App {
    fn redraw(&mut self) {
        // quick bail-out
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

        // ── resize texture if dimensions changed ──────
        if self.tex_w != self.render_w || self.tex_h != self.render_h {
            self.recreate_texture();
        }

        // ── recreate rgba buffer if dimensions changed ─
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

        // ── re-render galaxy (all on GPU) ────────────
        if self.needs_render {
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

        // ── display ──────────────────────────────────
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

        if !self.saved_startup_image {
            self.save_snapshot(device, queue);
            self.saved_startup_image = true;
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

// ── event handling ──────────────────────────────────────────

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

            // ── keyboard (brightness) ──────────────
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state.is_pressed()
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

            // ── drag ─────────────────────────────────
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.dragging = state == ElementState::Pressed;
            }

            WindowEvent::CursorMoved { position, .. } => {
                let (cx, cy) = (position.x, position.y);
                let (lx, ly) = self.last_mouse;
                self.last_mouse = (cx, cy);

                if self.dragging {
                    let dx = cx - lx;
                    let dy = cy - ly;

                    let ly_per_px = self.extent_ly / self.render_w as f64;

                    self.center_x -= dx * ly_per_px;
                    self.center_y += dy * ly_per_px;

                    self.needs_render = true;
                }
            }

            // ── zoom (wheel) ─────────────────────────
            WindowEvent::MouseWheel { delta, .. } => {
                use winit::event::MouseScrollDelta;
                let scroll: f64 = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(pos) => pos.y,
                };

                if scroll == 0.0 {
                    return;
                }

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

            WindowEvent::RedrawRequested => self.redraw(),

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Only keep redrawing while we have work to do,
        // otherwise wait for user input.
        if self.needs_render
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
    }
}

// ── entry point ─────────────────────────────────────────────

fn main() {
    let params = GalaxyParams::milky_way();
    let event_loop = EventLoop::new().unwrap();

    let mut app = App::new(params);
    event_loop.set_control_flow(ControlFlow::Wait);

    event_loop.run_app(&mut app).unwrap();
}
