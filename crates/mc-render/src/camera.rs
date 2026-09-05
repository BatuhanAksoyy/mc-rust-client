//! A minimal orbiting camera: enough to see a rendered chunk from outside
//! it. Look/movement input is later work (`docs/WORLD_PHYSICS_ASSETS.md`
//! assigns first-person movement to `mc-client`'s tick loop).

use glam::{Mat4, Vec3};

/// Orbits a fixed target at a fixed radius and height, looking inward.
///
/// `advance` moves it a bit further around the target each call so a static
/// screenshot still shows the chunk isn't just a flat billboard.
#[derive(Debug, Clone, Copy)]
pub struct OrbitCamera {
    /// World-space point the camera looks at (typically the chunk's center).
    pub target: Vec3,
    /// Distance from `target`.
    pub radius: f32,
    /// Height above `target`.
    pub height: f32,
    /// Current angle around `target`, in radians.
    pub angle: f32,
    /// Vertical field of view, in radians.
    pub fov_y: f32,
}

impl OrbitCamera {
    /// Frame `target` (typically a chunk's horizontal center) from far
    /// enough away and high enough to see the whole thing at once.
    #[must_use]
    pub const fn framing(target: Vec3, radius: f32, height: f32) -> Self {
        Self { target, radius, height, angle: 0.0, fov_y: 60_f32.to_radians() }
    }

    /// Step the orbit angle forward by `radians`.
    pub fn advance(&mut self, radians: f32) {
        self.angle += radians;
    }

    fn eye(&self) -> Vec3 {
        self.target
            + Vec3::new(self.angle.cos() * self.radius, self.height, self.angle.sin() * self.radius)
    }

    /// The combined view-projection matrix for the given viewport aspect ratio.
    #[must_use]
    pub fn view_projection(&self, aspect_ratio: f32) -> Mat4 {
        let view = glam::camera::rh::view::look_at_mat4(self.eye(), self.target, Vec3::Y);
        // wgpu's NDC Z range is [0, 1] regardless of backend (Metal/Vulkan/DX12/GL
        // are all normalized to this by wgpu itself), matching the "directx" convention.
        let projection =
            glam::camera::rh::proj::directx::perspective(self.fov_y, aspect_ratio, 0.1, 1000.0);
        projection * view
    }
}
