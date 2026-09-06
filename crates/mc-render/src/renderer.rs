//! `wgpu` device/surface/pipeline ownership. No game logic, no windowing —
//! `app.rs` owns the window and drives this per frame.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::mesh::{Mesh, Vertex};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
}

/// Owns the GPU device, surface and pipelines for one window.
///
/// `render` draws one frame of a single mesh (opaque/cutout plus
/// translucent halves) with one view-projection matrix — this client has
/// one chunk batch and no scene graph yet.
///
/// Two pipelines share one shader/bind group, differing only in depth
/// writes: `pipeline` (opaque + cutout — every real or debug-color block,
/// lava included) writes depth normally, while `translucent_pipeline`
/// (water only, `Mesh::translucent`) tests depth without writing it. Its
/// quads are sorted back-to-front from the interpolated camera whenever the
/// camera moves, preserving correct alpha accumulation with a stable order.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    translucent_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
    translucent_vertex_buffer: wgpu::Buffer,
    translucent_vertex_count: u32,
    translucent_vertices: Vec<Vertex>,
    translucent_sort_position: Option<glam::Vec3>,
}

/// Construction or GPU-adapter failure; there is no fallback renderer.
#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    /// `wgpu` could not create a surface for this window.
    #[error("failed to create a GPU surface: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
    /// No adapter satisfied the (modest) requirements requested.
    #[error("no suitable GPU adapter: {0}")]
    Adapter(#[from] wgpu::RequestAdapterError),
    /// The adapter could not open a logical device.
    #[error("failed to open a GPU device: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    /// The surface does not support any format this adapter offers.
    #[error("the window surface is not supported by this GPU adapter")]
    UnsupportedSurface,
}

impl Renderer {
    /// Create a renderer for `window`, sized to its current inner size, with
    /// `atlas` (a full mip chain, coarsest last) bound as the chunk shader's
    /// block texture — nearest-filtered within a level, matching
    /// Minecraft's blocky look, linearly blended between levels; see
    /// `atlas::Atlas`, `docs/RENDER.md` milestone 3.
    pub fn new(window: Arc<Window>, atlas: &[image::RgbaImage]) -> Result<Self, RendererError> {
        let size = window.inner_size().max(winit::dpi::PhysicalSize::new(1, 1));
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;

        let config = surface
            .get_default_config(&adapter, size.width, size.height)
            .ok_or(RendererError::UnsupportedSurface)?;
        surface.configure(&device, &config);

        let depth_view = create_depth_view(&device, size.width, size.height);

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("chunk-uniforms"),
            contents: bytemuck::bytes_of(&Uniforms { view_proj: [[0.0; 4]; 4] }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let (atlas_view, atlas_sampler) = create_atlas_texture(&device, &queue, atlas);
        let (bind_group_layout, bind_group) =
            create_bind_group(&device, &uniform_buffer, &atlas_view, &atlas_sampler);
        let pipeline =
            create_pipeline(&device, config.format, &bind_group_layout, true, "chunk-pipeline");
        let translucent_pipeline = create_pipeline(
            &device,
            config.format,
            &bind_group_layout,
            false,
            "chunk-translucent-pipeline",
        );

        let empty_vertex_buffer = |label| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: &[],
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            })
        };
        let vertex_buffer = empty_vertex_buffer("chunk-vertices");
        let translucent_vertex_buffer = empty_vertex_buffer("chunk-translucent-vertices");

        Ok(Self {
            surface,
            device,
            queue,
            config,
            depth_view,
            pipeline,
            translucent_pipeline,
            uniform_buffer,
            bind_group,
            vertex_buffer,
            vertex_count: 0,
            translucent_vertex_buffer,
            translucent_vertex_count: 0,
            translucent_vertices: Vec::new(),
            translucent_sort_position: None,
        })
    }

    /// Reconfigure the surface and depth buffer for a new window size.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, width, height);
    }

    /// Replace both vertex buffers' contents with `mesh`. Each reallocates
    /// only when its side of the mesh grows past its current capacity.
    pub fn set_mesh(&mut self, mesh: &Mesh) {
        let (buffer, count) = Self::upload(
            &self.device,
            &self.queue,
            &self.vertex_buffer,
            "chunk-vertices",
            &mesh.vertices,
        );
        self.vertex_buffer = buffer;
        self.vertex_count = count;
        self.translucent_vertices.clone_from(&mesh.translucent);
        self.translucent_sort_position = None;
    }

    /// Write `vertices` into `buffer` in place if it still fits, or allocate
    /// a new, larger buffer otherwise. Returns the buffer to keep (the same
    /// one, or the replacement) and the vertex count to draw.
    fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buffer: &wgpu::Buffer,
        label: &str,
        vertices: &[Vertex],
    ) -> (wgpu::Buffer, u32) {
        let bytes = bytemuck::cast_slice(vertices);
        let buffer = if bytes.len() as wgpu::BufferAddress > buffer.size() {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytes,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            })
        } else {
            queue.write_buffer(buffer, 0, bytes);
            buffer.clone()
        };
        (buffer, u32::try_from(vertices.len()).unwrap_or(u32::MAX))
    }

    /// The current surface's width/height aspect ratio, for camera projection.
    #[must_use]
    pub fn aspect_ratio(&self) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        // Window dimensions never approach f32's precision limit.
        let ratio = self.config.width as f32 / self.config.height.max(1) as f32;
        ratio
    }

    /// Draw one frame: clear, upload the view-projection matrix, draw the
    /// current mesh, present. Every recoverable surface hiccup (a resize
    /// race, an occluded/minimized window, one dropped frame) is swallowed
    /// here — none of them are this client's problem to recover from at
    /// this milestone (`docs/RENDER.md`); a future frame just tries again.
    pub fn render(&mut self, view_proj: glam::Mat4, camera_position: glam::Vec3) {
        self.sort_translucent(camera_position);
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&Uniforms { view_proj: view_proj.to_cols_array_2d() }),
        );

        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Validation => {
                eprintln!("dropped a frame: surface lost or a validation error occurred");
                return;
            }
        };
        let view = surface_texture.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("chunk-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.53,
                            g: 0.80,
                            b: 0.92,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        // Reversed-Z (`play.rs`'s `perspective_infinite_reverse`):
                        // "infinitely far" is depth 0, not 1.
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.draw(0..self.vertex_count, 0..1);
            // Same pass and depth buffer, drawn after opaque geometry. Water
            // is already back-to-front and tests depth without writing it.
            pass.set_pipeline(&self.translucent_pipeline);
            pass.set_vertex_buffer(0, self.translucent_vertex_buffer.slice(..));
            pass.draw(0..self.translucent_vertex_count, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(surface_texture);
    }

    fn sort_translucent(&mut self, camera_position: glam::Vec3) {
        const RESORT_DISTANCE_SQUARED: f32 = 1.0 / 256.0;
        if self.translucent_sort_position.is_some_and(|previous| {
            previous.distance_squared(camera_position) < RESORT_DISTANCE_SQUARED
        }) {
            return;
        }
        let sorted = sorted_translucent(&self.translucent_vertices, camera_position);
        let (buffer, count) = Self::upload(
            &self.device,
            &self.queue,
            &self.translucent_vertex_buffer,
            "chunk-translucent-vertices",
            &sorted,
        );
        self.translucent_vertex_buffer = buffer;
        self.translucent_vertex_count = count;
        self.translucent_sort_position = Some(camera_position);
    }
}

/// Return six-vertex quads ordered farthest-to-nearest for alpha blending.
/// Fluid meshing always emits one independent quad as two triangles.
fn sorted_translucent(vertices: &[Vertex], camera: glam::Vec3) -> Vec<Vertex> {
    let (quads, remainder) = vertices.as_chunks::<6>();
    debug_assert!(remainder.is_empty());
    let mut quads = quads.to_vec();
    let distance = |quad: &[Vertex; 6]| {
        let center = quad
            .iter()
            .fold(glam::Vec3::ZERO, |sum, vertex| sum + glam::Vec3::from_array(vertex.position))
            / 6.0;
        center.distance_squared(camera)
    };
    quads.sort_by(|left, right| distance(right).total_cmp(&distance(left)));
    quads.into_iter().flatten().collect()
}

/// Upload `levels` (a full mip chain, coarsest last — `atlas::Atlas::build`)
/// as one mipmapped `Rgba8UnormSrgb` texture. The sampler stays
/// nearest-filtered within a level (Minecraft's textures are hand-authored
/// pixel art — linear filtering would blur the blocky look up close) but
/// blends linearly between levels, matching vanilla's own default mipmap
/// setting: without it, any textured surface more than a couple of tiles
/// away, or seen at a grazing angle, aliases into shimmering noise — worst
/// of all for water's own thin, height-varying quads, the only geometry
/// here that ever spans less than a full block vertically.
fn create_atlas_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    levels: &[image::RgbaImage],
) -> (wgpu::TextureView, wgpu::Sampler) {
    let base = &levels[0];
    let size =
        wgpu::Extent3d { width: base.width(), height: base.height(), depth_or_array_layers: 1 };
    let mip_level_count = u32::try_from(levels.len()).unwrap_or(1);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("atlas-texture"),
        size,
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, image) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: u32::try_from(level).unwrap_or(0),
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * image.width()),
                rows_per_image: Some(image.height()),
            },
            wgpu::Extent3d {
                width: image.width(),
                height: image.height(),
                depth_or_array_layers: 1,
            },
        );
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("atlas-sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    (view, sampler)
}

/// Build the uniform/texture bind group (and its layout) shared by both
/// pipelines — only their depth-write state differs, never what they bind.
fn create_bind_group(
    device: &wgpu::Device,
    uniform_buffer: &wgpu::Buffer,
    atlas_view: &wgpu::TextureView,
    atlas_sampler: &wgpu::Sampler,
) -> (wgpu::BindGroupLayout, wgpu::BindGroup) {
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("chunk-bind-group-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("chunk-bind-group"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniform_buffer.as_entire_binding() },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(atlas_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(atlas_sampler),
            },
        ],
    });
    (bind_group_layout, bind_group)
}

/// Build one render pipeline against the shared bind group layout.
/// `depth_write_enabled` is the only thing that ever differs between the
/// opaque/cutout pipeline and the translucent one (see `Renderer`'s doc
/// comment) — same shader and blend state; only translucent depth writes are
/// disabled because those quads are explicitly sorted back-to-front.
fn create_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
    depth_write_enabled: bool,
    label: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("chunk-shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/chunk.wgsl").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("chunk-pipeline-layout"),
        bind_group_layouts: &[Some(bind_group_layout)],
        immediate_size: 0,
    });
    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &Vertex::ATTRIBUTES,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(vertex_layout)],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            // No greedy meshing/backface-aware winding yet (docs/RENDER.md):
            // rendering both sides is cheap for one chunk and side-steps
            // getting six manual winding orders right.
            cull_mode: None,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(depth_write_enabled),
            // Reversed-Z (`play.rs`'s `perspective_infinite_reverse`): nearer
            // is a *larger* depth value, so "passes" is GreaterEqual, not
            // LessEqual. Still an inclusive compare for the same reason the
            // non-reversed version was: layered resource models (grass side +
            // tinted overlay) intentionally emit coplanar quads in model order,
            // and the later one must still win the tie.
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

#[cfg(test)]
mod tests {
    use super::{Vertex, sorted_translucent};

    fn quad(z: f32) -> [Vertex; 6] {
        [Vertex { position: [0.0, 0.0, z], uv: [0.0; 2], tint: [1.0; 3] }; 6]
    }

    #[test]
    #[allow(clippy::float_cmp)] // Fixture coordinates are copied, not calculated.
    fn translucent_quads_are_sorted_back_to_front() {
        let vertices: Vec<_> = [quad(2.0), quad(8.0), quad(4.0)].into_iter().flatten().collect();
        let sorted = sorted_translucent(&vertices, glam::Vec3::ZERO);
        assert_eq!(
            [sorted[0].position[2], sorted[6].position[2], sorted[12].position[2]],
            [8.0, 4.0, 2.0]
        );
    }
}
