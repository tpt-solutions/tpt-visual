// color_correct.wgsl — brightness / contrast / saturation / hue, levels
// (in black/white, gamma, out black/white), and an optional 256x1 curve
// LUT texture (binding 3).
//
// p0 = (brightness, contrast, saturation, hue_degrees)
// p1 = (in_black, in_white, out_black, out_white)   [levels mode]
// `seed` carries the levels gamma.
// mode bit 0 (1u): apply levels
// mode bit 1 (2u): apply the 256x1 curve LUT (binding 3)

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
@group(0) @binding(3) var curve_texture : texture_2d<f32>;
@group(0) @binding(4) var curve_sampler : sampler;

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

fn rgb2hsv(c : vec3<f32>) -> vec3<f32> {
    let v = max(max(c.r, c.g), c.b);
    let min_c = min(min(c.r, c.g), c.b);
    let delta = v - min_c;
    var h = 0.0;
    if (delta > 0.0) {
        if (v == c.r) {
            h = (c.g - c.b) / delta;
        } else if (v == c.g) {
            h = 2.0 + (c.b - c.r) / delta;
        } else {
            h = 4.0 + (c.r - c.g) / delta;
        }
        h = h / 6.0;
        if (h < 0.0) {
            h = h + 1.0;
        }
    }
    let s = select(0.0, delta / v, v > 0.0);
    return vec3<f32>(h, s, v);
}

fn hsv2rgb(c : vec3<f32>) -> vec3<f32> {
    let h = fract(c.x) * 6.0;
    let i = floor(h);
    let f = h - i;
    let p = c.z * (1.0 - c.y);
    let q = c.z * (1.0 - c.y * f);
    let t = c.z * (1.0 - c.y * (1.0 - f));
    var rgb = vec3<f32>(0.0);
    if (i < 0.5) {
        rgb = vec3<f32>(c.z, t, p);
    } else if (i < 1.5) {
        rgb = vec3<f32>(q, c.z, p);
    } else if (i < 2.5) {
        rgb = vec3<f32>(p, c.z, t);
    } else if (i < 3.5) {
        rgb = vec3<f32>(p, q, c.z);
    } else if (i < 4.5) {
        rgb = vec3<f32>(t, p, c.z);
    } else {
        rgb = vec3<f32>(c.z, p, q);
    }
    return rgb;
}

fn apply_levels(x : f32, in_black : f32, in_white : f32, gamma : f32,
                out_black : f32, out_white : f32) -> f32 {
    let span = max(in_white - in_black, 1e-5);
    var v = clamp((x - in_black) / span, 0.0, 1.0);
    v = pow(v, 1.0 / gamma);
    return out_black + v * (out_white - out_black);
}

@fragment
fn fs_main(input : VertexOutput) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(input_texture, input_sampler, input.tex_coords, 0.0);

    let brightness = params.p0.x;
    let contrast = params.p0.y;
    let saturation = params.p0.z;
    let hue_degrees = params.p0.w;

    var rgb = c.rgb + brightness;
    // Contrast around 0.5 pivot.
    rgb = (rgb - 0.5) * (1.0 + contrast) + 0.5;

    // Saturation vs BT.709 luma.
    let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    rgb = mix(vec3<f32>(luma), rgb, saturation);

    // Hue rotation in HSV.
    if (abs(hue_degrees) > 0.001) {
        let hsv = rgb2hsv(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
        let rotated = vec3<f32>(hsv.x + hue_degrees / 360.0, hsv.y, hsv.z);
        rgb = hsv2rgb(rotated);
    }

    // Levels.
    if ((params.mode & 1u) != 0u) {
        rgb = vec3<f32>(
            apply_levels(rgb.r, params.p1.x, params.p1.y, params.seed,
                         params.p1.z, params.p1.w),
            apply_levels(rgb.g, params.p1.x, params.p1.y, params.seed,
                         params.p1.z, params.p1.w),
            apply_levels(rgb.b, params.p1.x, params.p1.y, params.seed,
                         params.p1.z, params.p1.w),
        );
    }

    // Baked tone curve (256x1 LUT, R=G=B entries).
    if ((params.mode & 2u) != 0u) {
        rgb = vec3<f32>(
            textureSampleLevel(curve_texture, curve_sampler,
                               vec2<f32>(clamp(rgb.r, 0.0, 1.0), 0.5), 0.0).r,
            textureSampleLevel(curve_texture, curve_sampler,
                               vec2<f32>(clamp(rgb.g, 0.0, 1.0), 0.5), 0.0).g,
            textureSampleLevel(curve_texture, curve_sampler,
                               vec2<f32>(clamp(rgb.b, 0.0, 1.0), 0.5), 0.0).b,
        );
    }

    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
}
