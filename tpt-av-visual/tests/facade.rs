//! Facade end-to-end: builder → renderer → RGBA/AVI, using only the
//! `tpt-av-visual` front door. GPU tests skip without an adapter.

use tpt_av_visual::prelude::*;
use tpt_av_visual::timeline::AssetId;

fn demo_session() -> Session {
    SessionBuilder::new(
        "facade test",
        FrameRate::film(),
        Resolution::new(64, 64).unwrap(),
    )
    .add_video_asset_with(
        "procedural://base",
        30,
        FrameRate::film(),
        Resolution::new(64, 64).unwrap(),
        PixelFormat::Rgba8,
        "Rec709",
    )
    .add_clip(ClipSpec::new(0, 0, 0).duration(30))
    .build()
    .expect("valid edit")
}

#[test]
fn builder_produces_expected_session() {
    let session = demo_session();
    assert_eq!(session.tracks.len(), 1);
    assert_eq!(session.assets.len(), 1);
    assert_eq!(session.duration_frames(), 30);
    let clip = &session.tracks[0].clips[0];
    assert_eq!(clip.effects.len(), 0);
    assert_eq!(clip.asset_id, AssetId(1));
}

#[test]
fn render_frame_rgba_matches_manual_target() {
    let session = demo_session();
    let asset = session.assets.values().next().unwrap().clone();

    let mut renderer = match TimelineRenderer::headless(session.clone()) {
        Ok(Some(renderer)) => renderer,
        Ok(None) => {
            eprintln!("skipping: no GPU adapter available");
            return;
        }
        Err(e) => panic!("{e}"),
    };
    renderer.attach_asset(
        asset,
        Box::new(SolidDecoder::new(
            [120, 60, 30, 255],
            FrameRate::film(),
            Resolution::new(64, 64).unwrap(),
        )),
    );

    let rgba = renderer.render_frame_rgba().expect("rgba render");
    assert_eq!(rgba.len(), 64 * 64 * 4);
    eprintln!("rgba first px {:?}", &rgba[..8]);

    // A solid source must survive the compositing chain with its color.
    let center = ((32 * 64 + 32) * 4) as usize;
    assert!(
        rgba[center].abs_diff(120) <= 2,
        "px {:?}",
        &rgba[center..center + 4]
    );

    // JSON round-trip via the timeline helpers.
    let json = session.to_json_string(true).expect("serialize");
    let reloaded = Session::from_json(&json).expect("deserialize");
    assert_eq!(reloaded, session);
}

#[test]
fn render_frames_to_avi_writes_file() {
    let session = demo_session();
    let mut renderer = match TimelineRenderer::headless(session) {
        Ok(Some(renderer)) => renderer,
        Ok(None) => {
            eprintln!("skipping: no GPU adapter available");
            return;
        }
        Err(e) => panic!("{e}"),
    };
    renderer.attach_default_decoders().expect("decoders");

    let out = std::env::temp_dir().join("tpt-av-visual-facade-test.avi");
    let bytes = renderer
        .render_frames_to_avi(&out, 5, 90)
        .expect("avi export");
    assert!(bytes > 1000, "non-trivial AVI: {bytes}");
    assert!(out.exists());
    std::fs::remove_file(&out).ok();
}

#[test]
fn builder_rejects_bad_indices() {
    let result = SessionBuilder::new("bad", FrameRate::film(), Resolution::new(8, 8).unwrap())
        .add_clip(ClipSpec::new(5, 0, 0))
        .build();
    assert!(result.is_err(), "unknown track index must be rejected");
}
