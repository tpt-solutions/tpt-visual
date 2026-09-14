// yuv_to_rgb.wgsl — planar YUV (BT.709 limited range) → packed RGBA.
//
// Bindings: 0 = Y plane, 1 = U plane, 2 = V plane, 3 = sampler.
// Chroma subsampling is resolved by normalized UV sampling of the
// (possibly smaller) chroma planes.

@group(0) @binding(0) var y_plane : texture_2d<f32>;
@group(0) @binding(1) var u_plane : texture_2d<f32>;
@group(0) @binding(2) var v_plane : texture_2d<f32>;
@group(0) @binding(3) var plane_sampler : sampler;

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
    let y = textureSampleLevel(y_plane, plane_sampler, input.tex_coords, 0.0).r;
    let u = textureSampleLevel(u_plane, plane_sampler, input.tex_coords, 0.0).r;
    let v = textureSampleLevel(v_plane, plane_sampler, input.tex_coords, 0.0).r;

    // BT.709 limited-range expansion (Y in [16, 235], chroma around 128).
    let y_full = (y - 0.0627451) * 1.1643836; // (y - 16/255) * 255/219
    let cb = u - 0.5;
    let cr = v - 0.5;
    let r = y_full + 1.7927411 * cr;
    let g = y_full - 0.2132486 * cb - 0.5328095 * cr;
    let b = y_full + 2.1124018 * cb;
    return vec4<f32>(clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
