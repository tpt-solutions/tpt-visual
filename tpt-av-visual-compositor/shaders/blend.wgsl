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

fn lum3(c : vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.3, 0.59, 0.11));
}

fn clip_color(c : vec3<f32>) -> vec3<f32> {
    let l = lum3(c);
    let n = min(min(c.r, c.g), c.b);
    let x = max(max(c.r, c.g), c.b);
    var out = c;
    if (n < 0.0) {
        out = l + (out - l) * l / max(l - n, 1e-5);
    }
    if (x > 1.0) {
        out = l + (out - l) * (1.0 - l) / max(x - l, 1e-5);
    }
    return out;
}

fn set_lum(c : vec3<f32>, l : f32) -> vec3<f32> {
    return clip_color(c + vec3<f32>(l - lum3(c)));
}

fn sat3(c : vec3<f32>) -> f32 {
    return max(max(c.r, c.g), c.b) - min(min(c.r, c.g), c.b);
}

fn set_sat(c : vec3<f32>, s : f32) -> vec3<f32> {
    let mn = min(min(c.r, c.g), c.b);
    let mx = max(max(c.r, c.g), c.b);
    if (mx <= mn + 1e-5) {
        return vec3<f32>(0.0);
    }
    let mid = c.r + c.g + c.b - mn - mx;
    let scaled = clamp((mid - mn) * s / max(mx - mn, 1e-5), 0.0, s);
    var out = vec3<f32>(0.0);
    if (c.r == mn) { out.r = 0.0; } else if (c.r == mx) { out.r = s; } else { out.r = scaled; }
    if (c.g == mn) { out.g = 0.0; } else if (c.g == mx) { out.g = s; } else { out.g = scaled; }
    if (c.b == mn) { out.b = 0.0; } else if (c.b == mx) { out.b = s; } else { out.b = scaled; }
    return out;
}

fn blend_channel(dst : vec3<f32>, src : vec3<f32>, mode : u32) -> vec3<f32> {
    switch mode {
        case 1u: { return dst * src; }                                     // multiply
        case 2u: { return vec3<f32>(1.0) - (vec3<f32>(1.0) - dst) * (vec3<f32>(1.0) - src); } // screen
        case 3u: { // overlay: dark backdrop multiplies, light backdrop screens
            return select(
                2.0 * dst * src,
                1.0 - 2.0 * (vec3<f32>(1.0) - dst) * (vec3<f32>(1.0) - src),
                dst > vec3<f32>(0.5),
            );
        }
        case 4u: { return min(dst, src); }                                 // darken
        case 5u: { return max(dst, src); }                                 // lighten
        case 6u: { // hard light: overlay with the roles swapped
            return select(
                2.0 * src * dst,
                1.0 - 2.0 * (vec3<f32>(1.0) - src) * (vec3<f32>(1.0) - dst),
                src > vec3<f32>(0.5),
            );
        }
        case 7u: { return abs(dst - src); }                                // difference
        case 8u: { return dst + src - 2.0 * dst * src; }                   // exclusion
        case 9u: { return set_lum(set_sat(src, sat3(dst)), lum3(dst)); }   // hue
        case 10u: { return set_lum(set_sat(dst, sat3(src)), lum3(dst)); }  // saturation
        case 11u: { return set_lum(src, lum3(dst)); }                      // color
        case 12u: { return set_lum(dst, lum3(src)); }                      // luminosity
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
