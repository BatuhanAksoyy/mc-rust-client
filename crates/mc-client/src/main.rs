// SPDX-License-Identifier: MIT OR Apache-2.0
//! Headless client entry point. Start with `mc-client status --help`.

use std::{path::PathBuf, process::ExitCode, time::Duration};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Minecraft protocol 776 headless client")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Probe offline login through Play Login, print context, then disconnect.
    Join {
        /// Offline server hostname or IP address.
        #[arg(default_value = "localhost")]
        host: String,
        /// Server TCP port.
        #[arg(long, default_value_t = 25565, value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
        /// Development username (1–16 ASCII letters, digits or underscores).
        #[arg(long, default_value = "RustPlayer")]
        name: String,
        /// Overall deadline through Play Login, in milliseconds.
        #[arg(long, default_value_t = 30_000, value_parser = clap::value_parser!(u64).range(1..=300_000))]
        timeout_ms: u64,
    },
    /// Start a managed local Pumpkin server (headless; gameplay client follows).
    Local {
        /// Path to the pinned Pumpkin executable.
        #[arg(long)]
        pumpkin: PathBuf,
        /// New or previously managed session directory outside any Git checkout.
        #[arg(long)]
        session: PathBuf,
        /// Local TCP port.
        #[arg(long, default_value_t = 25565, value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
        /// Maximum time to wait for server readiness.
        #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=600))]
        startup_seconds: u64,
        /// Stop after verifying readiness instead of waiting for Ctrl-C.
        #[arg(long)]
        check: bool,
    },
    /// Join and render the initial chunk batch in a window.
    Render {
        /// Offline server hostname or IP address.
        #[arg(default_value = "localhost")]
        host: String,
        /// Server TCP port.
        #[arg(long, default_value_t = 25565, value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
        /// Development username (1–16 ASCII letters, digits or underscores).
        #[arg(long, default_value = "RustPlayer")]
        name: String,
        /// Overall join deadline, in milliseconds.
        #[arg(long, default_value_t = 30_000, value_parser = clap::value_parser!(u64).range(1..=300_000))]
        timeout_ms: u64,
        /// Chunk radius requested from the server (the managed server caps this at 4).
        #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u8).range(2..=8))]
        render_distance: u8,
    },
    /// Query server status and measure ping latency without logging in.
    Status {
        /// Hostname or IP address (IPv6 addresses do not need brackets).
        #[arg(default_value = "localhost")]
        host: String,
        /// TCP port, also sent in the handshake.
        #[arg(long, default_value_t = 25565, value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
        /// Overall timeout in milliseconds, including DNS and connect.
        #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..=300_000))]
        timeout_ms: u64,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Join { host, port, name, timeout_ms } => {
            run_join(host, port, name, timeout_ms).await
        }
        Command::Local { pumpkin, session, port, startup_seconds, check } => {
            run_local(pumpkin, session, port, startup_seconds, check).await
        }
        Command::Render { host, port, name, timeout_ms, render_distance } => {
            run_render(host, port, name, timeout_ms, render_distance).await
        }
        Command::Status { host, port, timeout_ms } => run_status(host, port, timeout_ms).await,
    }
}

async fn run_join(host: String, port: u16, name: String, timeout_ms: u64) -> ExitCode {
    match mc_client::join::connect(&host, port, &name, Duration::from_millis(timeout_ms)).await {
        Ok(joined) => {
            println!(
                "Reached spawn: player={}, entity={}, dimension={}, registries={}, entries={}, \
                 position=({:.1}, {:.1}, {:.1})",
                joined.profile.name,
                joined.world.entity_id,
                joined.world.dimension_name,
                joined.registries.len(),
                joined.registries.entry_count(),
                joined.spawn.x,
                joined.spawn.y,
                joined.spawn.z
            );
            eprintln!(
                "Headless join probe complete; disconnecting. Rendering/gameplay are not implemented yet."
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run_local(
    pumpkin: PathBuf,
    session: PathBuf,
    port: u16,
    startup_seconds: u64,
    check: bool,
) -> ExitCode {
    // Install the handler before startup, so Ctrl-C also cancels startup
    // and drops any partially started child.
    let interrupted = tokio::signal::ctrl_c();
    tokio::pin!(interrupted);
    let startup =
        mc_client::local::start(&pumpkin, &session, port, Duration::from_secs(startup_seconds));
    let result = tokio::select! {
        result = startup => result,
        result = &mut interrupted => {
            eprintln!("Local startup interrupted: {result:?}");
            return ExitCode::FAILURE;
        }
    };
    let (mut server, status) = match result {
        Ok(started) => started,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    println!("{}", status.json);
    eprintln!("Pumpkin ready on 127.0.0.1:{}. Ctrl-C saves and stops.", server.port());
    if !check {
        loop {
            tokio::select! {
                result = &mut interrupted => {
                    if let Err(error) = result { eprintln!("Signal handler failed: {error}"); }
                    break;
                }
                () = tokio::time::sleep(Duration::from_millis(100)) => {
                    match server.try_wait() {
                        Ok(None) => {},
                        result => {
                            eprintln!("Pumpkin stopped unexpectedly: {result:?}");
                            return ExitCode::FAILURE;
                        }
                    }
                }
            }
        }
    }
    match server.shutdown(Duration::from_secs(30)).await {
        Ok(()) => {
            eprintln!("Pumpkin stopped cleanly.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Join, render the initial received chunk batch, and block until the window closes.
///
/// `mc_render::run` is a blocking, synchronous call (`winit` requires the
/// platform's main thread on macOS); calling it here is safe only because
/// nothing else is scheduled on this single-threaded runtime by the time we
/// reach it — the join future has already resolved.
async fn run_render(
    host: String,
    port: u16,
    name: String,
    timeout_ms: u64,
    render_distance: u8,
) -> ExitCode {
    let mut joined = match mc_client::join::connect_with_render_distance(
        &host,
        port,
        &name,
        render_distance,
        Duration::from_millis(timeout_ms),
    )
    .await
    {
        Ok(joined) => joined,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = joined.load_initial_chunks(render_distance, Duration::from_secs(10)).await {
        eprintln!("failed while loading the initial chunk view: {error}");
        return ExitCode::FAILURE;
    }
    if joined.chunks.is_empty() {
        eprintln!(
            "reached spawn but received no chunk beforehand; nothing to render (try again, or a larger view distance)"
        );
        return ExitCode::FAILURE;
    }
    let registry = mc_world::BlockRegistry::load_cached("26.2");
    let chunks: Vec<_> = joined.chunks.iter().map(mc_world::Chunk::from_level).collect();
    let origin = select_origin_chunk(&chunks, joined.spawn.x, joined.spawn.z, joined.center_chunk)
        .expect("a nonempty chunk batch always selects an origin");
    let origin_chunk = chunks
        .iter()
        .find(|chunk| chunk.position == origin)
        .expect("the selected origin always belongs to the chunk batch");
    let (atlas, atlas_image, non_solid_ids) = build_atlas(&chunks, &registry);
    let mesh = mc_render::mesh::mesh_chunks(&chunks, origin, &registry, &atlas);
    eprintln!(
        "Rendering {} chunks around ({}, {}): {} vertices. WASD to move, mouse to look, \
         Space to jump, Shift to sneak, Ctrl to sprint. Escape or close the window to exit.",
        chunks.len(),
        origin.x,
        origin.z,
        mesh.vertices.len()
    );
    let spawn = spawn_position(origin_chunk, &registry);
    let world = mc_world::World::new(origin, chunks);
    let game =
        mc_client::play::RenderGame::new(world, registry.with_non_solid(non_solid_ids), spawn);
    match mc_render::run(mesh, atlas_image, "mc-rust-client", game) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Resolve real block textures for every distinct block name in `chunks`
/// (`docs/RENDER.md` milestone 3), reading the extracted client jar's
/// resource files from cache (`mc_render::atlas::assets_root`). Silently
/// yields an atlas with nothing resolved when the extraction is absent —
/// The mesher already falls back to `registry`'s solid debug colors, same
/// as before this milestone.
///
/// The third element is every ID the atlas resolved to a walk-through model
/// (cross-shaped plants, torches, redstone components, ...; see
/// `mc_render::atlas::Atlas::is_solid`) — feed it to
/// `BlockRegistry::with_non_solid` so collision matches what actually
/// rendered instead of treating every non-air block as a full solid cube.
fn build_atlas(
    chunks: &[mc_world::Chunk],
    registry: &mc_world::BlockRegistry,
) -> (mc_render::atlas::Atlas, mc_render::atlas::RgbaImage, std::collections::HashSet<u32>) {
    let mut state_ids = std::collections::HashSet::new();
    for chunk in chunks {
        if let Ok(height) = i32::try_from(chunk.section_count() * 16) {
            for y in 0..height {
                for z in 0..16 {
                    for x in 0..16 {
                        if let Some(id) = chunk.block_at(x, y, z) {
                            state_ids.insert(id);
                        }
                    }
                }
            }
        }
    }
    let Some(assets_root) = mc_render::atlas::assets_root("26.2") else {
        eprintln!(
            "no extracted client assets cached (see docs/WORLD_PHYSICS_ASSETS.md); \
             rendering with solid debug colors instead of real textures"
        );
        let (atlas, image) =
            mc_render::atlas::Atlas::build(std::path::Path::new(""), std::iter::empty());
        return (atlas, image, std::collections::HashSet::new());
    };
    let (atlas, image) = mc_render::atlas::Atlas::build(
        &assets_root,
        state_ids.iter().copied().filter_map(|id| Some((id, registry.state(id)?))),
    );
    let non_solid = state_ids.into_iter().filter(|&id| atlas.is_solid(id) == Some(false)).collect();
    (atlas, image, non_solid)
}

/// Prefer the chunk containing the server-confirmed spawn, then its declared
/// cache center, while only ever returning a chunk that was actually loaded.
fn select_origin_chunk(
    chunks: &[mc_world::Chunk],
    spawn_x: f64,
    spawn_z: f64,
    center: Option<(i32, i32)>,
) -> Option<mc_world::ChunkPos> {
    let spawn = world_to_chunk(spawn_x).zip(world_to_chunk(spawn_z));
    spawn
        .map(|(x, z)| mc_world::ChunkPos { x, z })
        .filter(|position| chunks.iter().any(|chunk| chunk.position == *position))
        .or_else(|| {
            center
                .map(|(x, z)| mc_world::ChunkPos { x, z })
                .filter(|position| chunks.iter().any(|chunk| chunk.position == *position))
        })
        .or_else(|| chunks.first().map(|chunk| chunk.position))
}

#[allow(clippy::cast_possible_truncation)]
// Range-checking makes the float-to-i32 conversion exact for chunk coordinates.
fn world_to_chunk(coordinate: f64) -> Option<i32> {
    let chunk = (coordinate / 16.0).floor();
    (chunk.is_finite() && chunk >= f64::from(i32::MIN) && chunk <= f64::from(i32::MAX))
        .then_some(chunk as i32)
}

/// Spawn above the chunk's center column, a few blocks over its highest
/// solid block so the player visibly drops in and lands (or, for an
/// all-air column, at mid-height — there's nothing to land on).
#[allow(clippy::cast_precision_loss)] // A chunk's height in blocks is at most a few hundred.
fn spawn_position(chunk: &mc_world::Chunk, registry: &mc_world::BlockRegistry) -> glam::Vec3 {
    let height = i32::try_from(chunk.section_count() * 16).unwrap_or(0);
    let feet_y = (0..height)
        .rev()
        .find(|&y| chunk.block_at(8, y, 8).is_some_and(|id| !registry.is_air(id)))
        .map_or(height as f32 * 0.5, |ground| ground as f32 + 3.0);
    glam::Vec3::new(8.0, feet_y, 8.0)
}

async fn run_status(host: String, port: u16, timeout_ms: u64) -> ExitCode {
    match mc_client::status::query(&host, port, Duration::from_millis(timeout_ms)).await {
        Ok(result) => {
            println!("{}", result.json);
            eprintln!("Ping: {:.2} ms", result.latency.as_secs_f64() * 1000.0);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
