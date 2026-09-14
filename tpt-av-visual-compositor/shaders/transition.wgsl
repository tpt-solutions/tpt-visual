// transition.wgsl — crossfade, wipe, and dissolve between input A (from)
// and input B (to).
//
// progress = transition progress 0..1 (already curve-eased on the CPU)
// mode = 0 crossfade, 1 wipe (left→right), 2 dissolve (hash noise)
// p0.x = wipe edge softness (fraction of width)

struct NodeParams {
    matrix : mat3x3<f32>,
    p0 : vec4<f32>,
    p1 : vec4<f32>,
    texel : vec2<f32>,
    mode : u32,
    progress : f32,
};

@group(0) @binding(0) var from_texture : texture_2d<f32>;
@group(0) @binding(1) var input_sampler : sampler;
@group(0) @binding(2) var<uniform> params : NodeParams;
@group(0) @binding(3) var to_texture : texture_2d<f32>;

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

fn hash(p : vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

@fragment
fn fs_main(input : VertexOutput) -> @location(0) vec4<f32> {
    let from = textureSampleLevel(from_texture, input_sampler, input.tex_coords, 0.0);
    let to = textureSampleLevel(to_texture, input_sampler, input.tex_coords, 0.0);
    let p = clamp(params.progress, 0.0, 1.0);

    switch params.mode {
        case 1u: { // wipe with soft edge
            let softness = max(params.p0.x, 1e-4);
            let edge = input.tex_coords.x;
            let mix_t = clamp((p * (1.0 + softness) - edge) / softness, 0.0, 1.0);
            return mix(from, to, mix_t);
        }
        case 2u: { // dissolve
            let px = floor(input.tex_coords / params.texel);
            let n = hash(px);
            let mix_t = select(0.0, 1.0, n < p);
            return mix(from, to, mix_t);
        }
        default: { // crossfade
            return mix(from, to, p);
        }
    }
}
