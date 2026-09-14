// blur.wgsl — separable gaussian / box blur, plus motion blur.
//
// Pass layout: gaussian and box blur run twice (horizontal pass with
// p1.xy = (1,0), then vertical with (0,1)); motion blur is a single pass
// with p1.xy = (cos a, sin a).
//
// p0 = (radius_px, length_px, 0, 0)
// p1.xy = blur direction (unit texel-space)
// mode = 0 gaussian, 1 box, 2 motion

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

const TAPS : i32 = 9;

fn weight(i : i32, mode : u32) -> f32 {
    let x = f32(i) / 4.0;
    return select(1.0, exp(-0.5 * x * x / 0.36), mode == 0u);
}

@fragment
fn fs_main(input : VertexOutput) -> @location(0) vec4<f32> {
    let radius = select(params.p0.x, params.p0.y, params.mode == 2u);
    let direction = params.p1.xy * params.texel;
    var color = vec4<f32>(0.0);
    var total = 0.0;
    for (var i = -(TAPS - 1) / 2; i <= (TAPS - 1) / 2; i++) {
        let t = f32(i) / 4.0;
        let offset = direction * t * radius;
        let w = weight(i, params.mode);
        color += textureSampleLevel(
            input_texture, input_sampler, input.tex_coords + offset, 0.0) * w;
        total += w;
    }
    return color / total;
}
