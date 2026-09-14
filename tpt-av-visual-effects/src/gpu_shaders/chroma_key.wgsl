// chroma_key.wgsl — green/blue screen keying with spill suppression.
//
// p0.rgb = key color (0..1), p0.a = tolerance (chroma distance)
// p1.x = softness (added to tolerance for the feather falloff)
// p1.y = spill suppression (0 = off, 1 = full desaturation toward neutral)

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

// Chroma distance: compares the (Cb, Cr)-like difference between the pixel
// and the key, ignoring luma.
fn chroma_distance(c : vec3<f32>, key : vec3<f32>) -> f32 {
    let d = (c - key) * vec3<f32>(0.6, 0.3, 0.6);
    return length(d);
}

fn smoothstep_like(edge0 : f32, edge1 : f32, x : f32) -> f32 {
    let t = clamp((x - edge0) / max(edge1 - edge0, 1e-5), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

@fragment
fn fs_main(input : VertexOutput) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(input_texture, input_sampler, input.tex_coords, 0.0);
    let key = params.p0.rgb;
    let tolerance = params.p0.a;
    let softness = params.p1.x;
    let spill = params.p1.y;

    let d = chroma_distance(c.rgb, key);
    // Fully keyed below `tolerance`, opaque above tolerance + softness.
    let alpha = smoothstep_like(tolerance, tolerance + softness, d);

    var out = c.rgb;
    // Spill suppression: pull the channel that dominates the key color back
    // toward the average of the other two.
    if (spill > 0.0) {
        let spill_mix = spill * (1.0 - alpha);
        let key_is_green = key.g >= max(key.r, key.b);
        if (key_is_green) {
            let neutral = (out.r + out.b) * 0.5;
            let g = mix(out.g, min(out.g, neutral), spill_mix);
            out = vec3<f32>(out.r, g, out.b);
        } else if (key.b > key.r) {
            let neutral = (out.r + out.g) * 0.5;
            let b = mix(out.b, min(out.b, neutral), spill_mix);
            out = vec3<f32>(out.r, out.g, b);
        } else {
            let neutral = (out.g + out.b) * 0.5;
            let r = mix(out.r, min(out.r, neutral), spill_mix);
            out = vec3<f32>(r, out.g, out.b);
        }
    }

    return vec4<f32>(out, c.a * alpha);
}
