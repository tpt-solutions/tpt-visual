// transform.wgsl — position / scale / rotate a source over the canvas via
// inverse-mapped UV sampling. Outside the source footprint the fragment is
// transparent.
//
// uniform: matrix = target-UV → source-UV affine (3x3, last row implied),
// p0 = (src_aspect, dst_aspect, 0, 0) reserved.

struct NodeParams {
    matrix : mat3x3<f32>,
    p0 : vec4<f32>,
    p1 : vec4<f32>,
    texel : vec2<f32>,
    mode : u32,
    progress : f32,
};

@group(0) @binding(0) var input_texture : texture_2d<f32>;
@group(0) @binding(1) var input_sampler : sampler;
@group(0) @binding(2) var<uniform> params : NodeParams;
@group(0) @binding(3) var input_texture_b : texture_2d<f32>;

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

@fragment
fn fs_main(input : VertexOutput) -> @location(0) vec4<f32> {
    let target_uv = vec3<f32>(input.tex_coords, 1.0);
    let source_uv = (params.matrix * target_uv).xy;
    if (source_uv.x < 0.0 || source_uv.x > 1.0 ||
        source_uv.y < 0.0 || source_uv.y > 1.0) {
        return vec4<f32>(0.0);
    }
    return textureSampleLevel(input_texture, input_sampler, source_uv, 0.0);
}
