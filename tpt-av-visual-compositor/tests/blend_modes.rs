//! Blend-mode verification: the GPU blend shader is checked against a CPU
//! reference implementation of the standard formulas for every mode.
//! GPU tests skip without an adapter.

use compositor::{SolidDecoder, TimelineRenderer};
use tpt_av_visual_compositor as compositor;
use tpt_av_visual_timeline as timeline;
use tpt_av_visual_timeline::{AssetId, BlendMode, Clip, Session};
use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution};

/// CPU reference of the blend formulas in `blend.wgsl` (per channel).
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

fn blend_session(mode: BlendMode) -> (Session, timeline::VideoAsset, timeline::VideoAsset) {
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
    // Foreground color chosen to exercise both overlay/hard-light branches:
    // backdrop channels straddle 0.5, source straddles 0.5.
    let track = session.add_track("Layer");
    session
        .track_checked_mut(track)
        .unwrap()
        .insert_clip(fg)
        .unwrap();
    (session, base, overlay)
}

fn run_blend(mode: BlendMode) -> Option<[f32; 3]> {
    let (mut session, base, overlay) = blend_session(mode);
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
    ])
}

#[test]
fn gpu_blend_matches_cpu_reference_for_all_modes() {
    for mode in BlendMode::ALL {
        let Some(gpu) = run_blend(mode) else {
            return; // first iteration already printed the skip notice
        };
        // Foreground 0.6 gray over the (0.2, 0.4, 0.8) backdrop.
        let expected: Vec<f32> = (0..3)
            .map(|ch| {
                let dst = [0.2_f32, 0.4, 0.8][ch];
                let src = 0.6_f32;
                blend_channel(dst, src, mode.as_u32())
            })
            .collect();

        for (ch, e) in expected.iter().enumerate() {
            // 8-bit quantization on both ends of the pipeline.
            assert!(
                (gpu[ch] - e).abs() < 0.02,
                "{mode:?} channel {ch}: gpu {} vs cpu {e}",
                gpu[ch]
            );
        }
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
