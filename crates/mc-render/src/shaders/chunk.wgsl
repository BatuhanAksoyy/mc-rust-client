// Chunk shader: transform by one view-projection uniform, sample the block
// atlas and multiply by the pre-shaded per-vertex tint (real texture ×
// per-face brightness for resolved blocks, or a flat debug color on the
// atlas's reserved white texel for unresolved ones). See docs/RENDER.md
// milestone 3.

struct Uniforms {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;
@group(0) @binding(1)
var atlas_texture: texture_2d<f32>;
@group(0) @binding(2)
var atlas_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tint: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.tint = in.tint;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(atlas_texture, atlas_sampler, in.uv);
    // Binary cutouts (leaves/plants) must not write blended fringe pixels;
    // water remains translucent because its alpha is well above this edge.
    if sampled.a < 0.5 {
        discard;
    }
    return vec4<f32>(sampled.rgb * in.tint, sampled.a);
}
