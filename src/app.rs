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

    pub window: Option<std::sync::Arc<winit::window::Window>>,

    // image dimensions
    pub render_w: u32,
    pub render_h: u32,

    // camera (galactic coords, ly) — 2D pan/zoom
    pub center_x: f64,
    pub center_y: f64,
    pub extent_ly: f64,
    #[allow(dead_code)]
    pub needs_render: bool,

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
            window: None,
            render_w: 100,
            render_h: 100,
            center_x: 0.0,
            center_y: 0.0,
            extent_ly: INITIAL_EXTENT_LY,
            needs_render: true,
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

        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.config = Some(config);

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
}
