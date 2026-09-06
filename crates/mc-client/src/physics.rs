// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fixed-tick player movement physics: deterministic, testable without a
//! renderer or network.
//!
//! `docs/WORLD_PHYSICS_ASSETS.md` assigns this to `mc-client`'s tick loop;
//! `crate::play` drives it at 20 TPS and interpolates between ticks for
//! smooth rendering.
//!
//! This reproduces vanilla Java Edition's *feel* — walk/sprint/sneak speed,
//! jump height, gravity, and solid-block collision — from the wiki-documented
//! per-tick constants below. It is not a byte-for-byte port of
//! `LivingEntity.travel()`: ground/air acceleration is a simplified two-constant
//! model (real vanilla derives it from block slipperiness), and ice, water,
//! ladders, and slime bounce are not modeled yet.
//!
//! Collision is AABB vs. a single resolved chunk's voxel grid
//! (`mc_world::Chunk`); coordinates are chunk-local, matching `mc_render::mesh`
//! (x/z in `0..16`, y from the first decoded section's bottom). Multi-chunk
//! collision is future work once chunk streaming lands (`AI-GUIDE.md` step 8).

use glam::Vec3;
use mc_world::{BlockRegistry, Chunk};

/// Per-tick constants (20 TPS), in blocks unless noted. Speeds are the
/// wiki's blocks/second values divided by 20.
pub mod constants {
    /// Half-width of the player's collision box on X and Z (full width 0.6).
    pub const HALF_WIDTH: f32 = 0.3;
    /// Standing collision box height.
    pub const HEIGHT: f32 = 1.8;
    /// Eye height above the feet position, standing.
    pub const EYE_HEIGHT: f32 = 1.62;
    /// Walking speed: 4.317 blocks/s.
    pub const WALK_SPEED: f32 = 4.317 / 20.0;
    /// Sprinting speed: 5.612 blocks/s.
    pub const SPRINT_SPEED: f32 = 5.612 / 20.0;
    /// Sneaking speed: 1.31 blocks/s.
    pub const SNEAK_SPEED: f32 = 1.31 / 20.0;
    /// Instantaneous upward velocity applied on jump.
    pub const JUMP_VELOCITY: f32 = 0.42;
    /// Downward acceleration applied every tick, before drag.
    pub const GRAVITY: f32 = 0.08;
    /// Vertical velocity multiplier applied every tick, after gravity.
    pub const VERTICAL_DRAG: f32 = 0.98;
    /// Fraction of the gap to the target horizontal speed closed per tick
    /// while on the ground.
    ///
    /// Not a vanilla constant (real vanilla derives this from block
    /// slipperiness) — tuned for "stops almost immediately on the ground,
    /// slides in the air", vanilla's most visible feel.
    pub const GROUND_ACCEL: f32 = 0.6;
    /// Same, while airborne: much less control than on the ground.
    pub const AIR_ACCEL: f32 = 0.2;
}

/// One tick's movement intent, decoupled from any specific input backend
/// (`mc_render::InputState` is translated into this by `crate::play`).
#[allow(clippy::struct_excessive_bools)] // Mirrors vanilla's own fixed set of movement keys.
#[derive(Debug, Clone, Copy, Default)]
pub struct Input {
    /// Move in `+yaw`'s look direction.
    pub forward: bool,
    /// Move opposite `+yaw`'s look direction.
    pub back: bool,
    /// Strafe left relative to `yaw`.
    pub left: bool,
    /// Strafe right relative to `yaw`.
    pub right: bool,
    /// Jump if standing on the ground.
    pub jump: bool,
    /// Move at sneaking speed (takes priority over `sprint`).
    pub sneak: bool,
    /// Move at sprinting speed.
    pub sprint: bool,
    /// Look yaw, radians; 0 faces -Z, increasing turns toward +X. Rotates
    /// `forward`/`left`/`right`/`back` into world space.
    pub yaw: f32,
}

/// A player's physical state: feet position, velocity, and whether it is
/// currently resting on a solid block.
#[derive(Debug, Clone, Copy)]
pub struct PlayerController {
    /// Feet position, chunk-local block coordinates.
    pub position: Vec3,
    /// Current velocity, blocks/tick.
    pub velocity: Vec3,
    /// Whether the previous tick's downward movement was stopped by a solid block.
    pub on_ground: bool,
}

impl PlayerController {
    /// A player at rest at `position`, not yet known to be grounded (the
    /// first tick resolves it).
    #[must_use]
    pub const fn spawn(position: Vec3) -> Self {
        Self { position, velocity: Vec3::ZERO, on_ground: false }
    }

    /// The camera eye position: feet plus [`constants::EYE_HEIGHT`].
    #[must_use]
    pub fn eye(&self) -> Vec3 {
        self.position + Vec3::new(0.0, constants::EYE_HEIGHT, 0.0)
    }

    /// Advance one fixed 20 TPS tick: accelerate toward the wished horizontal
    /// velocity, apply gravity and jump, then resolve collisions against
    /// `chunk`'s solid blocks one axis at a time.
    pub fn tick(&mut self, input: Input, chunk: &Chunk, registry: &BlockRegistry) {
        let speed = if input.sneak {
            constants::SNEAK_SPEED
        } else if input.sprint {
            constants::SPRINT_SPEED
        } else {
            constants::WALK_SPEED
        };
        let mut wish = Vec3::ZERO;
        if input.forward {
            wish.z -= 1.0;
        }
        if input.back {
            wish.z += 1.0;
        }
        if input.right {
            wish.x += 1.0;
        }
        if input.left {
            wish.x -= 1.0;
        }
        if wish.length_squared() > 0.0 {
            wish = wish.normalize() * speed;
        }
        let (sin, cos) = input.yaw.sin_cos();
        let wished =
            Vec3::new(wish.z.mul_add(-sin, wish.x * cos), 0.0, wish.z.mul_add(cos, wish.x * sin));

        let accel = if self.on_ground { constants::GROUND_ACCEL } else { constants::AIR_ACCEL };
        self.velocity.x = (wished.x - self.velocity.x).mul_add(accel, self.velocity.x);
        self.velocity.z = (wished.z - self.velocity.z).mul_add(accel, self.velocity.z);

        if input.jump && self.on_ground {
            self.velocity.y = constants::JUMP_VELOCITY;
        }
        self.velocity.y -= constants::GRAVITY;
        self.velocity.y *= constants::VERTICAL_DRAG;

        self.on_ground = false;
        self.move_and_collide(chunk, registry);
    }

    fn move_and_collide(&mut self, chunk: &Chunk, registry: &BlockRegistry) {
        let (dy, hit) = move_axis(chunk, registry, self.position, self.velocity.y, Axis::Y);
        self.position.y += dy;
        if hit {
            if self.velocity.y < 0.0 {
                self.on_ground = true;
            }
            self.velocity.y = 0.0;
        }
        let (dx, hit) = move_axis(chunk, registry, self.position, self.velocity.x, Axis::X);
        self.position.x += dx;
        if hit {
            self.velocity.x = 0.0;
        }
        let (dz, hit) = move_axis(chunk, registry, self.position, self.velocity.z, Axis::Z);
        self.position.z += dz;
        if hit {
            self.velocity.z = 0.0;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
}

fn is_solid(chunk: &Chunk, registry: &BlockRegistry, x: i32, y: i32, z: i32) -> bool {
    chunk.block_at(x, y, z).is_some_and(|id| !registry.is_air(id))
}

/// Clamp `delta` (movement along `axis`, starting from `position`) so the
/// player's AABB does not enter a solid block, checking every block the
/// swept AABB could touch.
///
/// Discrete per-axis resolution, not a continuous sweep — adequate at these
/// speeds and this milestone's single-chunk scope (see the module doc's
/// scoped-limitations note). Returns the allowed delta and whether it was
/// clamped short of `delta` by a collision.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
// Chunk-local coordinates and per-tick deltas are always small.
fn move_axis(
    chunk: &Chunk,
    registry: &BlockRegistry,
    position: Vec3,
    delta: f32,
    axis: Axis,
) -> (f32, bool) {
    const EPSILON: f32 = 1e-4;
    if delta == 0.0 {
        return (delta, false);
    }
    let half = constants::HALF_WIDTH;
    // The two fixed axes' block ranges the swept AABB spans, and the moving
    // axis's near/far edge (relative to `position`) before this delta.
    let (fixed1, fixed2, near, far) = match axis {
        Axis::X => (
            (position.y, position.y + constants::HEIGHT),
            (position.z - half, position.z + half),
            position.x - half,
            position.x + half,
        ),
        Axis::Y => (
            (position.x - half, position.x + half),
            (position.z - half, position.z + half),
            position.y,
            position.y + constants::HEIGHT,
        ),
        Axis::Z => (
            (position.x - half, position.x + half),
            (position.y, position.y + constants::HEIGHT),
            position.z - half,
            position.z + half,
        ),
    };
    let (f1_lo, f1_hi) = (fixed1.0.floor() as i32, (fixed1.1 - EPSILON).floor() as i32);
    let (f2_lo, f2_hi) = (fixed2.0.floor() as i32, (fixed2.1 - EPSILON).floor() as i32);
    let (lo, hi) = if delta > 0.0 {
        (far.floor() as i32, (far + delta).floor() as i32)
    } else {
        ((near + delta).floor() as i32, near.floor() as i32)
    };

    let mut allowed = delta;
    for moving in lo..=hi {
        for a in f1_lo..=f1_hi {
            for b in f2_lo..=f2_hi {
                let blocked = match axis {
                    Axis::X => is_solid(chunk, registry, moving, a, b),
                    Axis::Y => is_solid(chunk, registry, a, moving, b),
                    Axis::Z => is_solid(chunk, registry, a, b, moving),
                };
                if !blocked {
                    continue;
                }
                let limit =
                    if delta > 0.0 { moving as f32 - far } else { (moving as f32 + 1.0) - near };
                allowed = if delta > 0.0 { allowed.min(limit) } else { allowed.max(limit) };
            }
        }
    }
    // Exact comparison is deliberate: `allowed` starts as a bit-for-bit copy
    // of `delta` and is only ever reassigned (via `min`/`max`) when a block
    // clamps it — no arithmetic drift to guard against.
    #[allow(clippy::float_cmp)]
    let clamped = allowed != delta;
    (allowed, clamped)
}

#[cfg(test)]
mod tests {
    use mc_protocol::chunk::{ChunkSection, LevelChunk, LightData, Palette, PalettedContainer};
    use mc_world::{BlockRegistry, Chunk};

    use super::{Input, PlayerController, Vec3, constants};

    const fn no_light() -> LightData {
        LightData {
            sky_light_mask: Vec::new(),
            block_light_mask: Vec::new(),
            empty_sky_light_mask: Vec::new(),
            empty_block_light_mask: Vec::new(),
            sky_light: Vec::new(),
            block_light: Vec::new(),
        }
    }

    /// One section; `is_solid(x, y, z)` decides each entry (palette `[air, stone]`).
    fn chunk_from(is_solid: impl Fn(i32, i32, i32) -> bool) -> Chunk {
        let mut indices = vec![0_u32; 4096];
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    if is_solid(x, y, z) {
                        let index = usize::try_from((y * 16 + z) * 16 + x).unwrap();
                        indices[index] = 1;
                    }
                }
            }
        }
        let section = ChunkSection {
            block_count: 0,
            fluid_count: 0,
            block_states: PalettedContainer {
                bits_per_entry: 4,
                palette: Palette::Indirect(vec![0, 1]),
                indices,
            },
            biomes: PalettedContainer {
                bits_per_entry: 0,
                palette: Palette::Single(0),
                indices: Vec::new(),
            },
        };
        let level = LevelChunk {
            x: 0,
            z: 0,
            heightmaps: Vec::new(),
            sections: vec![section],
            block_entities: Vec::new(),
            light: no_light(),
        };
        Chunk::from_level(&level)
    }

    fn empty_chunk() -> Chunk {
        chunk_from(|_, _, _| false)
    }

    /// Solid floor across the whole section at y = 0 (occupies world space `y in [0, 1)`).
    fn floor_chunk() -> Chunk {
        chunk_from(|_, y, _| y == 0)
    }

    fn registry() -> BlockRegistry {
        BlockRegistry::from_names(vec!["minecraft:air".to_owned(), "minecraft:stone".to_owned()])
    }

    fn still(yaw: f32) -> Input {
        Input { yaw, ..Input::default() }
    }

    #[test]
    fn free_fall_matches_gravity_then_drag() {
        let mut player = PlayerController::spawn(Vec3::new(8.0, 100.0, 8.0));
        player.tick(still(0.0), &empty_chunk(), &registry());
        let expected = -constants::GRAVITY * constants::VERTICAL_DRAG;
        assert!((player.velocity.y - expected).abs() < 1e-6, "{}", player.velocity.y);
        assert!(!player.on_ground);
    }

    #[test]
    fn standing_on_a_floor_stops_falling_and_reports_grounded() {
        let mut player = PlayerController::spawn(Vec3::new(8.0, 1.0, 8.0)); // Feet resting on the y=0 block.
        for _ in 0..10 {
            player.tick(still(0.0), &floor_chunk(), &registry());
        }
        assert!(player.on_ground);
        assert_eq!(player.position.y.to_bits(), 1.0_f32.to_bits());
        assert_eq!(player.velocity.y.to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn jump_leaves_the_ground_and_falls_back() {
        let mut player = PlayerController::spawn(Vec3::new(8.0, 1.0, 8.0));
        let chunk = floor_chunk();
        let registry = registry();
        player.tick(still(0.0), &chunk, &registry); // Settle onto the floor first.
        assert!(player.on_ground);
        player.tick(Input { jump: true, ..still(0.0) }, &chunk, &registry);
        assert!(player.velocity.y > 0.0, "jump should give upward velocity");
        assert!(!player.on_ground, "leaving the ground this tick");
        for _ in 0..40 {
            player.tick(still(0.0), &chunk, &registry);
        }
        assert!(player.on_ground, "should have landed again");
        assert_eq!(player.position.y.to_bits(), 1.0_f32.to_bits());
    }

    #[test]
    fn walking_forward_approaches_walk_speed_and_moves_negative_z() {
        let mut player = PlayerController::spawn(Vec3::new(8.0, 1.0, 8.0));
        let chunk = floor_chunk();
        let registry = registry();
        for _ in 0..40 {
            player.tick(Input { forward: true, ..still(0.0) }, &chunk, &registry);
        }
        assert!(player.velocity.z < 0.0);
        assert!(
            (-player.velocity.z - constants::WALK_SPEED).abs() < 0.01,
            "velocity.z={}",
            player.velocity.z
        );
        assert!(player.position.z < 8.0);
    }

    #[test]
    fn sprint_is_faster_than_walk() {
        let chunk = floor_chunk();
        let registry = registry();
        let mut walker = PlayerController::spawn(Vec3::new(8.0, 1.0, 8.0));
        let mut sprinter = PlayerController::spawn(Vec3::new(8.0, 1.0, 8.0));
        for _ in 0..40 {
            walker.tick(Input { forward: true, ..still(0.0) }, &chunk, &registry);
            sprinter.tick(Input { forward: true, sprint: true, ..still(0.0) }, &chunk, &registry);
        }
        assert!(sprinter.velocity.z.abs() > walker.velocity.z.abs());
    }

    #[test]
    fn a_wall_blocks_horizontal_movement() {
        // A floor (so the player isn't also falling through the open world
        // below y=0, which would fall clean past the wall's y-range) plus a
        // wall filling the whole column at x = 9 (chunk-local `x in [9, 10)`).
        // The player (half-width 0.3) starts with its edge just before it.
        let chunk = chunk_from(|x, y, _| x == 9 || y == 0);
        let registry = registry();
        let mut player = PlayerController::spawn(Vec3::new(8.6, 1.0, 8.0));
        for _ in 0..20 {
            player.tick(Input { right: true, sprint: true, ..still(0.0) }, &chunk, &registry);
        }
        assert!(player.position.x + constants::HALF_WIDTH <= 9.0 + 1e-4, "{}", player.position.x);
    }
}
