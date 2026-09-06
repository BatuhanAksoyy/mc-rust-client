// SPDX-License-Identifier: MIT OR Apache-2.0
//! Drives the render window each frame: mouse-look, fixed-20-TPS movement
//! physics via [`crate::tick::TickScheduler`], and camera interpolation
//! between ticks.
//!
//! `mc-render` holds no game logic (`docs/RENDER.md`) — [`RenderGame`] is the
//! [`mc_render::Game`] implementation that gives `mc-client render`'s window
//! WASD-relative-to-look movement, sprint/sneak speed, gravity, jump, and
//! collision against the loaded initial chunk view (`AI-GUIDE.md` steps 7-8).

use std::{num::NonZeroU32, time::Duration};

use glam::{Mat4, Vec3};
use mc_render::{Game, InputState};
use mc_world::{BlockRegistry, World};

use crate::{
    physics::{self, PlayerController},
    tick::TickScheduler,
};

/// Radians turned per raw mouse unit at Java Edition's default 100%
/// sensitivity: `(0.5 * 0.6 + 0.2)^3 * 8 * 0.15` degrees.
const MOUSE_SENSITIVITY: f32 = std::f32::consts::PI / 1_200.0;
/// Keeps the look direction from flipping over at the poles.
const MAX_PITCH: f32 = 89_f32.to_radians();
/// Vertical field of view.
const FOV_Y: f32 = 70_f32.to_radians();
/// A tick backlog beyond this is dropped rather than caught up (e.g. a
/// window stalled by being dragged should not fast-forward physics).
const MAX_CATCH_UP_TICKS: u32 = 5;

/// The `mc-client render` window's [`mc_render::Game`]: owns player physics
/// state against a resolved chunk view and turns it into a camera each frame.
pub struct RenderGame {
    world: World,
    registry: BlockRegistry,
    scheduler: TickScheduler,
    previous: PlayerController,
    current: PlayerController,
    yaw: f32,
    pitch: f32,
    alpha: f64,
}

impl RenderGame {
    /// Spawn a player standing (feet) at `position`, chunk-local coordinates,
    /// looking down -Z, inside `world`.
    #[must_use]
    pub const fn new(world: World, registry: BlockRegistry, position: Vec3) -> Self {
        let controller = PlayerController::spawn(position);
        let scheduler = TickScheduler::new(
            NonZeroU32::new(MAX_CATCH_UP_TICKS).expect("MAX_CATCH_UP_TICKS is nonzero"),
        );
        Self {
            world,
            registry,
            scheduler,
            previous: controller,
            current: controller,
            yaw: 0.0,
            pitch: 0.0,
            alpha: 0.0,
        }
    }
}

impl Game for RenderGame {
    fn update(&mut self, elapsed: Duration, input: &InputState) {
        // Mouse look is applied immediately, every frame — not gated by the
        // fixed tick — matching vanilla's feel (movement is simulated at
        // 20 TPS, camera turning is not).
        self.yaw = input.look_delta.0.mul_add(MOUSE_SENSITIVITY, self.yaw);
        self.pitch =
            input.look_delta.1.mul_add(-MOUSE_SENSITIVITY, self.pitch).clamp(-MAX_PITCH, MAX_PITCH);

        let batch = self.scheduler.advance(elapsed);
        for _ in 0..batch.ticks {
            self.previous = self.current;
            let wish = physics::Input {
                forward: input.forward,
                back: input.back,
                left: input.left,
                right: input.right,
                jump: input.jump,
                sneak: input.sneak,
                sprint: input.sprint,
                yaw: self.yaw,
            };
            self.current.tick(wish, &self.world, &self.registry);
        }
        self.alpha = batch.alpha;
    }

    fn view_projection(&self, aspect_ratio: f32) -> Mat4 {
        #[allow(clippy::cast_possible_truncation)] // `alpha` is always in [0, 1).
        let alpha = self.alpha as f32;
        let eye = self.previous.eye().lerp(self.current.eye(), alpha);
        let forward = forward_vector(self.yaw, self.pitch);
        let view = glam::camera::rh::view::look_at_mat4(eye, eye + forward, Vec3::Y);
        // wgpu's NDC Z range is [0, 1] regardless of backend (Metal/Vulkan/DX12/GL
        // are all normalized to this by wgpu itself), matching the "directx" convention.
        //
        // Reversed-Z (`perspective_infinite_reverse`, `Renderer`'s depth
        // pipeline state) instead of a finite far plane: a chunk-batch scene
        // has no natural "far" distance to guess at, and a standard
        // (non-reversed) depth buffer concentrates almost all of a float32
        // depth's precision within the first few meters of `near` regardless
        // of how far the chosen far plane actually is — anything at a real
        // render distance away, especially viewed at a grazing angle (a
        // water surface stretching toward the horizon), is left fighting
        // for what precision remains, visible as flickering noise wherever
        // two surfaces are close in world space but far from the camera.
        let projection = glam::camera::rh::proj::directx::perspective_infinite_reverse(
            FOV_Y,
            aspect_ratio,
            0.05,
        );
        projection * view
    }
}

/// The unit look direction for `yaw` (0 faces -Z, increasing turns toward
/// +X — matches [`physics::PlayerController::tick`]'s input rotation) and
/// `pitch` (positive looks up).
fn forward_vector(yaw: f32, pitch: f32) -> Vec3 {
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    Vec3::new(sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch)
}
