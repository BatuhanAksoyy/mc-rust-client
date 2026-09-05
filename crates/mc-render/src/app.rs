//! Windowing glue: owns the `winit` event loop and hands frames to `Renderer`.

use std::sync::Arc;

use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowAttributes, WindowId},
};

use crate::{camera::OrbitCamera, mesh::Mesh, renderer::Renderer};

/// Failure opening a window or its renderer.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The platform's windowing system rejected window/event-loop creation.
    #[error("failed to create the window: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),
    /// See [`crate::renderer::RendererError`].
    #[error(transparent)]
    Renderer(#[from] crate::renderer::RendererError),
}

/// Open a window titled `title` and render `mesh` in it (an orbiting camera
/// framing `camera_target`/`camera_radius`) until the window is closed.
///
/// Blocks the calling thread: `winit` requires the platform's main thread on
/// macOS, so this must not run inside a Tokio runtime (fetch any network
/// data first, then call this).
pub fn run(
    mesh: Mesh,
    title: &str,
    camera_target: glam::Vec3,
    camera_radius: f32,
) -> Result<(), RunError> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        title: title.to_owned(),
        mesh,
        camera: OrbitCamera::framing(camera_target, camera_radius, camera_radius * 0.6),
        window: None,
        renderer: None,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App {
    title: String,
    mesh: Mesh,
    camera: OrbitCamera,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return; // Already set up (e.g. a redundant resume after a suspend).
        }
        let attributes = WindowAttributes::default()
            .with_title(self.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("failed to create window: {error}");
                event_loop.exit();
                return;
            }
        };
        let mut renderer = match Renderer::new(window.clone()) {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("failed to create renderer: {error}");
                event_loop.exit();
                return;
            }
        };
        renderer.set_mesh(&self.mesh);
        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(renderer) = &mut self.renderer else { return };
                self.camera.advance(0.003); // A slow orbit so the render is visibly 3D, not a still frame.
                let view_proj = self.camera.view_projection(renderer.aspect_ratio());
                renderer.render(view_proj);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}
