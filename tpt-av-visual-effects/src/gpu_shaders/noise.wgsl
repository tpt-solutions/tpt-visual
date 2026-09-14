// noise.wgsl — film-grain generation and noise reduction.
//
// mode 0: generate — p0.x = amount, p0.y = monochrome (0/1), seed
// mode 1: reduce   — p0.x = strength; 3x3 gaussian-neighborhood blend

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

fn hash(p : vec2<f32>, seed : f32) -> f32 {
    var h = dot(p, vec2<f32>(127.1, 311.7)) + seed * 74.7;
    h = fract(sin(h) * 43758.5453);
    return h;
}

@fragment
fn fs_main(input : VertexOutput) -> @location(0) vec4<f32> {
    if (params.mode == 0u) {
        let c = textureSampleLevel(input_texture, input_sampler, input.tex_coords, 0.0);
        let amount = params.p0.x;
        let monochrome = params.p0.y;
        let px = floor(input.tex_coords / params.texel);
        let n0 = hash(px, params.seed);
        let n1 = hash(px + vec2<f32>(17.0, 43.0), params.seed);
        let n2 = hash(px + vec2<f32>(91.0, 7.0), params.seed);
        let grain = vec3<f32>(n0, select(n1, n0, monochrome > 0.5), select(n2, n0, monochrome > 0.5));
        let noise = (grain - 0.5) * amount;
        return vec4<f32>(clamp(c.rgb + noise, vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
    }

    // Noise reduction: blend toward a gaussian-ish 3x3 average.
    let src = textureSampleLevel(input_texture, input_sampler, input.tex_coords, 0.0);
    var sum = vec3<f32>(0.0);
    var weight_sum = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let offset = vec2<f32>(f32(dx), f32(dy)) * params.texel;
            let sample = textureSampleLevel(
                input_texture, input_sampler, input.tex_coords + offset, 0.0);
            // Bilateral-ish: down-weight samples far from the center color.
            let color_dist = distance(sample.rgb, src.rgb);
            let w = exp(-color_dist * color_dist * 32.0);
            sum += sample.rgb * w;
            weight_sum += w;
        }
    }
    let filtered = sum / weight_sum;
    let strength = clamp(params.p0.x, 0.0, 1.0);
    return vec4<f32>(mix(src.rgb, filtered, strength), src.a);
}
