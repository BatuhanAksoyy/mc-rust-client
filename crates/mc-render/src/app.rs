// SPDX-License-Identifier: MIT OR Apache-2.0
//! Windowing glue: owns the `winit` event loop and hands frames to `Renderer`.
//!
//! Holds no game logic (`docs/RENDER.md`): each frame it collects input
//! (held movement keys, accumulated raw mouse motion, elapsed wall-clock
//! time) and hands it to a caller-supplied [`Game`], which owns physics,
//! camera state, and any fixed-tick interpolation. `mc-client`'s `play`
//! module is the concrete `Game` for `mc-client render`.

use std::{sync::Arc, time::Instant};

use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, DeviceEvents, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowAttributes, WindowId},
};

use crate::{mesh::Mesh, renderer::Renderer};

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

/// Which movement keys are currently held, and raw mouse-look motion
/// accumulated since the last [`Game::update`] call.
///
/// A per-frame snapshot, decoupled from `winit` so `Game` implementations
/// stay testable.
#[allow(clippy::struct_excessive_bools)]
// Each field is an independent held-key flag, not a state machine — this is
// a plain input snapshot, matching vanilla's own fixed key bindings.
#[derive(Debug, Clone, Copy, Default)]
pub struct InputState {
    /// `W` held.
    pub forward: bool,
    /// `S` held.
    pub back: bool,
    /// `A` held.
    pub left: bool,
    /// `D` held.
    pub right: bool,
    /// Space held.
    pub jump: bool,
    /// Left Shift held.
    pub sneak: bool,
    /// Left Ctrl held.
    pub sprint: bool,
    /// Accumulated `DeviceEvent::MouseMotion` delta since the last call, in
    /// unspecified device units (`(right, down)`) — reset every frame.
    pub look_delta: (f32, f32),
}

/// Game-side logic driven once per rendered frame.
///
/// Implementations own physics, camera, and any fixed-tick scheduling; this
/// module only ever supplies input and elapsed wall-clock time, never reads
/// game state back out except through [`Self::view_projection`].
pub trait Game {
    /// Advance game state by `elapsed` wall-clock time, given this frame's input.
    fn update(&mut self, elapsed: std::time::Duration, input: &InputState);
    /// The view-projection matrix to render this frame.
    fn view_projection(&self, aspect_ratio: f32) -> glam::Mat4;
}

/// Open a window titled `title`, capture the mouse for FPS-style look, and
/// drive `game` once per frame, rendering `mesh`.
///
/// `mesh` is textured from `atlas` (see `crate::atlas::Atlas`) until the
/// window closes or Escape is pressed.
///
/// Blocks the calling thread: `winit` requires the platform's main thread on
/// macOS, so this must not run inside a Tokio runtime (fetch any network
/// data first, then call this).
pub fn run(
    mesh: Mesh,
    atlas: Vec<image::RgbaImage>,
    title: &str,
    game: impl Game + 'static,
) -> Result<(), RunError> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    // A no-op on macOS (device events are always delivered there) but needed
    // on X11/Windows to receive `DeviceEvent::MouseMotion` for mouse-look.
    event_loop.listen_device_events(DeviceEvents::Always);
    let mut app = App {
        title: title.to_owned(),
        mesh,
        atlas,
        game,
        window: None,
        renderer: None,
        keys: Keys::default(),
        look_delta: (0.0, 0.0),
        last_frame: None,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[allow(clippy::struct_excessive_bools)] // Mirrors `InputState`'s held-key flags.
#[derive(Debug, Clone, Copy, Default)]
struct Keys {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    jump: bool,
    sneak: bool,
    sprint: bool,
}

struct App<G: Game> {
    title: String,
    mesh: Mesh,
    atlas: Vec<image::RgbaImage>,
    game: G,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    keys: Keys,
    look_delta: (f32, f32),
    last_frame: Option<Instant>,
}

impl<G: Game> ApplicationHandler for App<G> {
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
        let mut renderer = match Renderer::new(window.clone(), &self.atlas) {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("failed to create renderer: {error}");
                event_loop.exit();
                return;
            }
        };
        renderer.set_mesh(&self.mesh);
        capture_cursor(&window);
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.last_frame = Some(Instant::now());
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => self.handle_key(event_loop, &event),
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta } = event {
            #[allow(clippy::cast_possible_truncation)]
            // Per-frame mouse deltas are tiny; f32 precision is plenty.
            {
                self.look_delta.0 += delta.0 as f32;
                self.look_delta.1 += delta.1 as f32;
            }
        }
    }
}

impl<G: Game> App<G> {
    fn handle_key(&mut self, event_loop: &ActiveEventLoop, event: &KeyEvent) {
        if event.repeat {
            return;
        }
        let PhysicalKey::Code(code) = event.physical_key else { return };
        let pressed = event.state == ElementState::Pressed;
        match code {
            KeyCode::KeyW => self.keys.forward = pressed,
            KeyCode::KeyS => self.keys.back = pressed,
            KeyCode::KeyA => self.keys.left = pressed,
            KeyCode::KeyD => self.keys.right = pressed,
            KeyCode::Space => self.keys.jump = pressed,
            KeyCode::ShiftLeft => self.keys.sneak = pressed,
            KeyCode::ControlLeft => self.keys.sprint = pressed,
            KeyCode::Escape if pressed => event_loop.exit(),
            _ => {}
        }
    }

    fn redraw(&mut self) {
        let Some(renderer) = &mut self.renderer else { return };
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_frame.unwrap_or(now));
        self.last_frame = Some(now);
        let input = InputState {
            forward: self.keys.forward,
            back: self.keys.back,
            left: self.keys.left,
            right: self.keys.right,
            jump: self.keys.jump,
            sneak: self.keys.sneak,
            sprint: self.keys.sprint,
            look_delta: std::mem::take(&mut self.look_delta),
        };
        self.game.update(elapsed, &input);
        let view_proj = self.game.view_projection(renderer.aspect_ratio());
        renderer.render(view_proj);
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

/// Lock the cursor to the window and hide it, FPS-style. Best-effort: not
/// every platform supports every grab mode (macOS/Wayland support `Locked`;
/// X11 only `Confined`), and a visible, unlocked cursor beats a crash.
fn capture_cursor(window: &Window) {
    if window.set_cursor_grab(CursorGrabMode::Locked).is_err()
        && window.set_cursor_grab(CursorGrabMode::Confined).is_err()
    {
        eprintln!("cursor grab unsupported on this platform; mouse-look will fight the OS cursor");
        return;
    }
    window.set_cursor_visible(false);
}
