//! Blend-mode verification: the GPU blend shader is checked against a CPU
//! reference implementation of the standard formulas for every mode.
//! GPU tests skip without an adapter.

use compositor::{SolidDecoder, TimelineRenderer};
use tpt_av_visual_compositor as compositor;
use tpt_av_visual_timeline as timeline;
use tpt_av_visual_timeline::{AssetId, BlendMode, Clip, Session};
use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution};

/// CPU reference of the HSL helper group in `blend.wgsl`.
fn blend_channel_hsl(dst: [f32; 3], src: [f32; 3], mode: u32) -> [f32; 3] {
    fn lum(c: [f32; 3]) -> f32 {
        0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
    }
    fn clip_color(mut c: [f32; 3]) -> [f32; 3] {
        let l = lum(c);
        let n = c[0].min(c[1]).min(c[2]);
        let x = c[0].max(c[1]).max(c[2]);
        if n < 0.0 {
            let f = l / (l - n).max(1e-5);
            c = [l + (c[0] - l) * f, l + (c[1] - l) * f, l + (c[2] - l) * f];
        }
        if x > 1.0 {
            let f = (1.0 - l) / (x - l).max(1e-5);
            c = [l + (c[0] - l) * f, l + (c[1] - l) * f, l + (c[2] - l) * f];
        }
        c
    }
    fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
        let d = l - lum(c);
        clip_color([c[0] + d, c[1] + d, c[2] + d])
    }
    fn sat(c: [f32; 3]) -> f32 {
        c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
    }
    fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
        let mn = c[0].min(c[1]).min(c[2]);
        let mx = c[0].max(c[1]).max(c[2]);
        if mx <= mn + 1e-5 {
            return [0.0; 3];
        }
        let mid = c[0] + c[1] + c[2] - mn - mx;
        let scaled = ((mid - mn) * s / (mx - mn).max(1e-5)).clamp(0.0, s);
        let pick = |channel: f32| -> f32 {
            if (channel - mn).abs() < 1e-7 {
                0.0
            } else if (channel - mx).abs() < 1e-7 {
                s
            } else {
                scaled
            }
        };
        [pick(c[0]), pick(c[1]), pick(c[2])]
    }
    match mode {
        9 => set_lum(set_sat(src, sat(dst)), lum(dst)),
        10 => set_lum(set_sat(dst, sat(src)), lum(dst)),
        11 => set_lum(src, lum(dst)),
        12 => set_lum(dst, lum(src)),
        _ => src,
    }
}

/// CPU reference of the blend formulas in `blend.wgsl`.
fn blend_channel(dst: f32, src: f32, mode: u32) -> f32 {
    match mode {
        1 => dst * src,
        2 => 1.0 - (1.0 - dst) * (1.0 - src),
        3 => {
            if dst > 0.5 {
                1.0 - 2.0 * (1.0 - dst) * (1.0 - src)
            } else {
                2.0 * dst * src
            }
        }
        4 => dst.min(src),
        5 => dst.max(src),
        6 => {
            if src > 0.5 {
                1.0 - 2.0 * (1.0 - src) * (1.0 - dst)
            } else {
                2.0 * src * dst
            }
        }
        7 => (dst - src).abs(),
        8 => dst + src - 2.0 * dst * src,
        _ => src,
    }
}

fn blend_pixel(dst: [f32; 3], src: [f32; 3], mode: u32) -> [f32; 3] {
    if mode >= 9 {
        blend_channel_hsl(dst, src, mode)
    } else {
        [
            blend_channel(dst[0], src[0], mode),
            blend_channel(dst[1], src[1], mode),
            blend_channel(dst[2], src[2], mode),
        ]
    }
}

fn blend_session(
    mode: BlendMode,
    track_opacity: f32,
    clip_opacity: f32,
) -> (Session, timeline::VideoAsset, timeline::VideoAsset) {
    let mut session = Session::new(
        "blend test",
        FrameRate::film(),
        Resolution::new(16, 16).unwrap(),
    );
    let base = session.register_asset(timeline::VideoAsset::new(
        AssetId(0),
        "solid://backdrop",
        10,
        FrameRate::film(),
        Resolution::new(16, 16).unwrap(),
        PixelFormat::Rgba8,
        "Rec709",
    ));
    let overlay = session.register_asset(timeline::VideoAsset::new(
        AssetId(0),
        "solid://layer",
        10,
        FrameRate::film(),
        Resolution::new(16, 16).unwrap(),
        PixelFormat::Rgba8,
        "Rec709",
    ));
    let base_clip = Clip::new(session.allocate_clip_id(), base.id, 0, 0, 10);
    session.tracks[0].insert_clip(base_clip).unwrap();
    let mut fg = Clip::new(session.allocate_clip_id(), overlay.id, 0, 0, 10);
    fg.blend_mode = mode;
    fg.opacity = clip_opacity;
    // Foreground color chosen to exercise both overlay/hard-light branches:
    // backdrop channels straddle 0.5, source straddles 0.5.
    let track = session.add_track("Layer");
    {
        let track = session.track_checked_mut(track).unwrap();
        track.opacity = track_opacity;
        track.insert_clip(fg).unwrap();
    }
    (session, base, overlay)
}

fn run_blend(mode: BlendMode) -> Option<[f32; 3]> {
    run_blend_with(mode, 1.0, 1.0).map(|px| [px[0], px[1], px[2]])
}

fn run_blend_with(mode: BlendMode, track_opacity: f32, clip_opacity: f32) -> Option<[f32; 4]> {
    let (mut session, base, overlay) = blend_session(mode, track_opacity, clip_opacity);
    let mut renderer = match TimelineRenderer::headless(session.clone()) {
        Ok(Some(renderer)) => renderer,
        Ok(None) => {
            eprintln!("skipping: no GPU adapter available");
            return None;
        }
        Err(e) => panic!("{e}"),
    };
    let _ = &mut session;
    renderer.attach_asset(
        base,
        Box::new(SolidDecoder::new(
            [51, 102, 204, 255], // 0.2, 0.4, 0.8
            FrameRate::film(),
            Resolution::new(16, 16).unwrap(),
        )),
    );
    renderer.attach_asset(
        overlay,
        Box::new(SolidDecoder::new(
            [153, 153, 153, 255], // 0.6 gray
            FrameRate::film(),
            Resolution::new(16, 16).unwrap(),
        )),
    );

    let rgba = renderer.render_frame_rgba().expect("render");
    let center = ((8 * 16 + 8) * 4) as usize;
    Some([
        f32::from(rgba[center]) / 255.0,
        f32::from(rgba[center + 1]) / 255.0,
        f32::from(rgba[center + 2]) / 255.0,
        f32::from(rgba[center + 3]) / 255.0,
    ])
}

#[test]
fn track_opacity_multiplies_clip_opacity() {
    // Track opacity 0.5 x clip opacity 0.5 => foreground factor 0.25 over
    // the opaque (0.2, 0.4, 0.8) backdrop.
    let Some(result) = run_blend_with(BlendMode::Normal, 0.5, 0.5) else {
        return;
    };
    let [r, g, b, a] = result;
    let fc = 0.25_f32;
    let bg = [0.2_f32, 0.4, 0.8];
    let fg = 0.6_f32;
    let expected: Vec<f32> = (0..3).map(|ch| fg * fc + bg[ch] * (1.0 - fc)).collect();
    for (ch, e) in expected.iter().enumerate() {
        assert!(
            (result[ch] - e).abs() < 0.02,
            "channel {ch}: {result:?} vs {e}"
        );
    }
    assert!((a - 1.0).abs() < 0.02, "alpha {a}");
    let _ = (r, g, b);
}

#[test]
fn multiplied_opacity_scales_multiply_blend() {
    // Multiply blend at quarter opacity over an opaque backdrop:
    // out = blend*fc + backdrop*(1-fc), fully opaque.
    let Some(result) = run_blend_with(BlendMode::Multiply, 1.0, 0.25) else {
        return;
    };
    let [r, g, b, a] = result;
    let fc = 0.25_f32;
    let bg = [0.2_f32, 0.4, 0.8];
    let fg = 0.6_f32;
    let expected: Vec<f32> = (0..3)
        .map(|ch| bg[ch] * fg * fc + bg[ch] * (1.0 - fc))
        .collect();
    for (ch, e) in expected.iter().enumerate() {
        assert!(
            (result[ch] - e).abs() < 0.02,
            "channel {ch}: {result:?} vs {e}"
        );
    }
    assert!((a - 1.0).abs() < 0.02, "alpha {a}");
    let _ = (r, g, b);
}

#[test]
fn gpu_blend_matches_cpu_reference_for_all_modes() {
    for mode in BlendMode::ALL {
        let Some(gpu) = run_blend(mode) else {
            return; // first iteration already printed the skip notice
        };
        // Foreground 0.6 gray over the (0.2, 0.4, 0.8) backdrop.
        let expected = blend_pixel([0.2, 0.4, 0.8], [0.6, 0.6, 0.6], mode.as_u32());

        for (ch, e) in expected.iter().enumerate() {
            // 8-bit quantization on both ends of the pipeline.
            assert!(
                (gpu[ch] - e).abs() < 0.02,
                "{mode:?} channel {ch}: gpu {} vs cpu {e}",
                gpu[ch]
            );
        }
        eprintln!("{mode:?}: gpu {gpu:?} cpu {expected:?}");
    }
}

#[test]
fn blend_formula_reference_values() {
    // Spot-check the CPU reference against hand-computed values.
    assert!((blend_channel(0.2, 0.6, 3) - 0.24).abs() < 1e-6); // overlay dark
    assert!((blend_channel(0.8, 0.6, 3) - 0.84).abs() < 1e-6); // overlay light
    assert!((blend_channel(0.8, 0.2, 6) - 0.32).abs() < 1e-6); // hard light dark src
    assert!((blend_channel(0.8, 0.6, 6) - 0.84).abs() < 1e-6); // hard light light src
    assert!((blend_channel(0.8, 0.6, 1) - 0.48).abs() < 1e-6); // multiply
    assert!((blend_channel(0.2, 0.6, 2) - 0.68).abs() < 1e-6); // screen
    assert!((blend_channel(0.8, 0.6, 7) - 0.2).abs() < 1e-6); // difference
}
