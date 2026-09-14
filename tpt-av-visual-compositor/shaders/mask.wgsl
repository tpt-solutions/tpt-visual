// mask.wgsl — alpha masking and chroma keying.
//
// Basic mode (mode 0): multiply input A's alpha by input B's luminance
// (luma matte). Chroma mode (mode 1): key input A against the key color
// (p1 = key rgb + tolerance, progress = softness) with spill suppression
// (p0.x).
//
// matrix: target-UV → mask-UV affine for the matte input (identity unless
// the mask has its own placement).

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
@group(0) @binding(3) var mask_texture : texture_2d<f32>;

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

    if (params.mode == 1u) {
        // Chroma key mode.
        let key = params.p1.rgb;
        let tolerance = params.p1.a;
        let softness = params.progress;
        let d = length((c.rgb - key) * vec3<f32>(0.6, 0.3, 0.6));
        let alpha = smoothstep_like(tolerance, tolerance + softness, d);
        let spill = params.p0.x;
        var rgb = c.rgb;
        if (spill > 0.0 && alpha < 1.0) {
            // Pull the dominant chroma channel toward neutral.
            let key_is_green = key.g >= max(key.r, key.b);
            let key_is_red = key.r >= max(key.g, key.b);
            let dominant = select(select(rgb.b, rgb.g, key_is_green), rgb.r, key_is_red);
            let neutral = select(select((rgb.r + rgb.g) * 0.5, (rgb.r + rgb.b) * 0.5, key_is_green), (rgb.g + rgb.b) * 0.5, key_is_red);
            let k = spill * (1.0 - alpha);
            rgb = mix(rgb, vec3<f32>(min(dominant, neutral)), k);
        }
        return vec4<f32>(rgb, c.a * alpha);
    }

    // Luma matte mode: mask luminance scales alpha.
    let mask_uv = (params.matrix * vec3<f32>(input.tex_coords, 1.0)).xy;
    let m = textureSampleLevel(mask_texture, input_sampler, mask_uv, 0.0);
    let luma = dot(m.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec4<f32>(c.rgb, c.a * luma * m.a);
}
