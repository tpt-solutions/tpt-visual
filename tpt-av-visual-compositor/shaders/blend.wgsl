// blend.wgsl — composite `input_texture_b` (foreground) over `input_texture`
// (background) with a blend mode and a foreground opacity multiplier.
//
// p0.x = foreground opacity (0..1)
// mode = BlendMode index (0 normal .. 8 exclusion), matching
//        tpt-av-visual-timeline::BlendMode::as_u32

struct NodeParams {
    matrix : mat3x3<f32>,
    p0 : vec4<f32>,
    p1 : vec4<f32>,
    texel : vec2<f32>,
    mode : u32,
    progress : f32,
};

@group(0) @binding(0) var bg_texture : texture_2d<f32>;
@group(0) @binding(1) var input_sampler : sampler;
@group(0) @binding(2) var<uniform> params : NodeParams;
@group(0) @binding(3) var fg_texture : texture_2d<f32>;

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

fn blend_channel(dst : vec3<f32>, src : vec3<f32>, mode : u32) -> vec3<f32> {
    switch mode {
        case 1u: { return dst * src; }                                     // multiply
        case 2u: { return vec3<f32>(1.0) - (vec3<f32>(1.0) - dst) * (vec3<f32>(1.0) - src); } // screen
        case 3u: { // overlay
            return select(
                2.0 * dst * src + 2.0 * dst * (1.0 - dst) - dst * dst,
                1.0 - 2.0 * (vec3<f32>(1.0) - dst) * (vec3<f32>(1.0) - src),
                dst > vec3<f32>(0.5),
            );
        }
        case 4u: { return min(dst, src); }                                 // darken
        case 5u: { return max(dst, src); }                                 // lighten
        case 6u: { // hard light
            return select(
                1.0 - 2.0 * (vec3<f32>(1.0) - dst) * (vec3<f32>(1.0) - src),
                2.0 * src * dst,
                src > vec3<f32>(0.5),
            );
        }
        case 7u: { return abs(dst - src); }                                // difference
        case 8u: { return dst + src - 2.0 * dst * src; }                   // exclusion
        default: { return src; }                                           // normal
    }
}

@fragment
fn fs_main(input : VertexOutput) -> @location(0) vec4<f32> {
    let bg = textureSampleLevel(bg_texture, input_sampler, input.tex_coords, 0.0);
    let fg = textureSampleLevel(fg_texture, input_sampler, input.tex_coords, 0.0);
    let opacity = params.p0.x * fg.a;

    let blended = blend_channel(bg.rgb, fg.rgb, params.mode);
    // Standard alpha compositing of the blended color over the backdrop.
    let out_a = fg.a * params.p0.x + bg.a * (1.0 - fg.a * params.p0.x);
    var rgb = (blended * fg.a * params.p0.x + bg.rgb * bg.a * (1.0 - fg.a * params.p0.x))
        / max(out_a, 1e-5);
    rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb, out_a);
}
