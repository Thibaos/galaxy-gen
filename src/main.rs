mod app;
mod camera;
mod input;
mod render;
mod ui;

use galaxy_gen::galaxy::GalaxyParams;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
};

use crate::app::App;

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
        if matches!(&event, WindowEvent::RedrawRequested) {
            render::redraw(self);
            return;
        }

        let window = self.window.clone();
        if let Some(window) = &window {
            input::handle_window_event(&event, self, event_loop, window);
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
