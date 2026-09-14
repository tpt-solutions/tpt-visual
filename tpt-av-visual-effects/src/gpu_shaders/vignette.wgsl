// vignette.wgsl — radial darkening from a center point.
//
// p0 = (amount, radius, softness, 0); tex coords normalized 0..1, center
// assumed (0.5, 0.5).

struct EffectParams {
    p0 : vec4<f32>,
    p1 : vec4<f32>,
    texel : vec2<f32>,
    mode : u32,
    seed : f32,
    _pad : vec3<f32>,
};

@group(0) @binding(0) var input_texture : texture_2d<f32>;
@group(0) @binding(1) var input_sampler : sampler;
@group(0) @binding(2) var<uniform> params : EffectParams;

struct VertexOutput {
    @builtin(position) position : vec4<f32>,
    @location(0) tex_coords : vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index : u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output : VertexOutput;
    output.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    // Flip V: texture row 0 is the top, NDC +Y is up.
    output.tex_coords = vec2<f32>(
        (positions[vertex_index].x + 1.0) * 0.5,
        1.0 - (positions[vertex_index].y + 1.0) * 0.5,
    );
    return output;
}

fn smoothstep_like(edge0 : f32, edge1 : f32, x : f32) -> f32 {
    let t = clamp((x - edge0) / max(edge1 - edge0, 1e-5), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

@fragment
fn fs_main(input : VertexOutput) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(input_texture, input_sampler, input.tex_coords, 0.0);
    let amount = params.p0.x;
    let radius = params.p0.y;
    let softness = params.p0.z;

    let d = distance(input.tex_coords, vec2<f32>(0.5, 0.5)) / max(radius, 1e-4);
    let falloff = smoothstep_like(1.0 - softness, 1.0 + softness, d);
    let factor = 1.0 - amount * falloff;
    return vec4<f32>(c.rgb * factor, c.a);
}
