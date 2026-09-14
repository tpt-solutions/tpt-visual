// color_convert.wgsl — fused color pipeline pass for tpt-av-visual-color.
//
// One fragment pass implements the five ColorPipeline steps:
//   1. decode the input transfer function to linear light
//   2. gamut-convert via the composed src→dst matrix
//   3. tone map (optional; disabled by `tonemap_mode == 0`)
//   4. apply a 3D LUT (optional; disabled by `use_lut == 0`)
//   5. encode to the output transfer function

struct ColorParams {
    src_to_dst : mat3x3<f32>,
    in_gamma : f32,
    out_gamma : f32,
    in_scale : f32,
    in_transfer : u32,
    out_transfer : u32,
    tonemap_mode : u32,
    use_lut : u32,
    lut_size : f32,
    _pad : vec3<f32>,
};

@group(0) @binding(0) var input_texture : texture_2d<f32>;
@group(0) @binding(1) var input_sampler : sampler;
@group(0) @binding(2) var<uniform> params : ColorParams;
@group(0) @binding(3) var lut_texture : texture_3d<f32>;
@group(0) @binding(4) var lut_sampler : sampler;

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

fn decode_transfer(code : f32, mode : u32, gamma : f32) -> f32 {
    let v = max(code, 0.0);
    switch mode {
        case 0u: { // sRGB
            return select(pow((v + 0.055) / 1.055, 2.4), v / 12.92, v <= 0.04045);
        }
        case 1u: { // linear
            return v;
        }
        case 2u: { // PQ (ST 2084); 1.0 == 10000 nits
            let m1 = 0.1593017578125;
            let m2 = 78.84375;
            let c1 = 0.8359375;
            let c2 = 18.8515625;
            let c3 = 18.6875;
            let ep = pow(v, 1.0 / m2);
            return pow(max(ep - c1, 0.0) / (c2 - c3 * ep), 1.0 / m1);
        }
        case 3u: { // HLG
            let a = 0.17883277;
            let b = 1.0 - 4.0 * a;
            let c = 0.55991073;
            return select(
                (exp((v - c) / a) + b) / 12.0,
                v * v / 3.0,
                v <= 0.5
            );
        }
        case 4u: { // gamma
            return pow(v, gamma);
        }
        default: {
            return v;
        }
    }
}

fn encode_transfer(linear : f32, mode : u32, gamma : f32) -> f32 {
    let v = max(linear, 0.0);
    switch mode {
        case 0u: { // sRGB
            return select(1.055 * pow(v, 1.0 / 2.4) - 0.055, 12.92 * v, v <= 0.0031308);
        }
        case 1u: { // linear
            return v;
        }
        case 2u: { // PQ
            let m1 = 0.1593017578125;
            let m2 = 78.84375;
            let c1 = 0.8359375;
            let c2 = 18.8515625;
            let c3 = 18.6875;
            let y = pow(v, m1);
            return pow((c1 + c2 * y) / (1.0 + c3 * y), m2);
        }
        case 3u: { // HLG
            let a = 0.17883277;
            let b = 1.0 - 4.0 * a;
            let c = 0.55991073;
            return select(a * log(12.0 * v - b) + c, sqrt(3.0 * v), v <= 1.0 / 12.0);
        }
        case 4u: { // gamma
            return pow(v, 1.0 / gamma);
        }
        default: {
            return v;
        }
    }
}

fn tonemap(x : f32, mode : u32) -> f32 {
    switch mode {
        case 1u: { // Reinhard (white at 4.0)
            let scaled = max(x, 0.0) * 4.0;
            return scaled / (1.0 + scaled);
        }
        case 2u: { // ACES filmic (Narkowicz fit)
            let v = max(x, 0.0);
            return clamp((v * (2.51 * v + 0.03)) / (v * (2.43 * v + 0.59) + 0.14), 0.0, 1.0);
        }
        default: {
            return x;
        }
    }
}

@fragment
fn fs_main(input : VertexOutput) -> @location(0) vec4<f32> {
    let code = textureSampleLevel(input_texture, input_sampler, input.tex_coords, 0.0).rgb;

    // 1. Linearize (plus optional HDR range scale).
    var linear = vec3<f32>(
        decode_transfer(code.r, params.in_transfer, params.in_gamma),
        decode_transfer(code.g, params.in_transfer, params.in_gamma),
        decode_transfer(code.b, params.in_transfer, params.in_gamma),
    ) * params.in_scale;

    // 2. Gamut conversion.
    linear = params.src_to_dst * linear;

    // 3. Tone map.
    if (params.tonemap_mode != 0u) {
        linear = vec3<f32>(
            tonemap(linear.r, params.tonemap_mode),
            tonemap(linear.g, params.tonemap_mode),
            tonemap(linear.b, params.tonemap_mode),
        );
    }

    // 4. 3D LUT (trilinear on the GPU via texture_3d filtering).
    if (params.use_lut != 0u) {
        linear = textureSampleLevel(
            lut_texture,
            lut_sampler,
            clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0)),
            0.0,
        ).rgb;
    }

    // 5. Encode to output transfer.
    let out = vec3<f32>(
        encode_transfer(linear.r, params.out_transfer, params.out_gamma),
        encode_transfer(linear.g, params.out_transfer, params.out_gamma),
        encode_transfer(linear.b, params.out_transfer, params.out_gamma),
    );
    return vec4<f32>(out, 1.0);
}
