//! End-to-end timeline workflows: build a session, edit it through the
//! history, serialize it, and reload it.

use tpt_av_visual_timeline as timeline;
use timeline::{
    AssetId, BlendMode, Clip, EffectInstance, History, InterpolationMethod, Keyframe, Session,
};
use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution};

fn demo_session() -> Session {
    let mut session = Session::new("Demo", FrameRate::film(), Resolution::full_hd());
    let asset = session.register_asset(timeline::VideoAsset::new(
        AssetId(0),
        "media/interview.mp4",
        480,
        FrameRate::film(),
        Resolution::full_hd(),
        PixelFormat::Yuv420p,
        "Rec709",
    ));

    // Track 1: a trimmed clip with an animated position and a blur.
    let mut clip = Clip::new(
        session.allocate_clip_id(),
        asset.id,
        24,
        12,
        96,
    );
    clip.blend_mode = BlendMode::Normal;
    clip.effects
        .push(EffectInstance::new("gaussian_blur").with_param("radius", 2.5));
    let mut pos_y = timeline::KeyframeTrack::new(
        "transform.position.y",
        InterpolationMethod::Bezier,
    );
    pos_y.upsert_keyframe(Keyframe::bezier(24, -200.0, 0.25, 0.1, 0.25, 1.0));
    pos_y.upsert_keyframe(Keyframe::at(72, 0.0));
    clip.keyframes.push(pos_y);
    session.tracks[0].insert_clip(clip).unwrap();

    // Track 2: an overlay with screen blending.
    let overlay_track = session.add_track("Overlay");
    let overlay = Clip::new(session.allocate_clip_id(), asset.id, 48, 0, 48);
    session
        .track_checked_mut(overlay_track)
        .unwrap()
        .insert_clip(overlay)
        .unwrap();

    session
}

#[test]
fn split_undo_roundtrip() {
    let mut session = demo_session();
    assert_eq!(session.duration_frames(), 120);

    // Split the base clip in the middle, then undo it.
    let base_clip_id = session.tracks[0].clips[0].id;
    let mut history = History::default();
    let right_id = history.split_clip(&mut session, base_clip_id, 72).unwrap();
    assert_eq!(session.tracks[0].clips.len(), 2);
    let right = session.tracks[0].clip(right_id).unwrap();
    assert_eq!(right.source_offset, 12 + 48);

    assert!(history.undo(&mut session).unwrap());
    assert_eq!(session.tracks[0].clips.len(), 1);
    assert_eq!(session.tracks[0].clips[0].duration_frames, 96);
    assert_eq!(session.tracks[0].clips[0].source_offset, 12);
}

#[test]
fn split_move_undo_redo_roundtrip() {
    let mut session = demo_session();
    let base_clip_id = session.tracks[0].clips[0].id;
    let track0 = session.tracks[0].id;
    let mut history = History::default();

    // Split at frame 72 (48 frames into the 96-frame clip).
    let right_id = history.split_clip(&mut session, base_clip_id, 72).unwrap();
    let right = session.tracks[0].clip(right_id).unwrap().clone();
    assert_eq!(right.start_frame, 72);
    assert_eq!(right.duration_frames, 48);
    assert_eq!(right.source_offset, 12 + 48);

    // Move the right part to the overlay track, after the existing overlay
    // clip (which spans 48..96).
    let overlay_track = session.tracks[1].id;
    history
        .commit(
            &mut session,
            timeline::edit::MoveClip::new(right_id, track0, 72, overlay_track, 96),
        )
        .unwrap();
    assert!(session.tracks[0].clip(right_id).is_none());
    assert!(session.tracks[1].clip(right_id).is_some());

    // Undo both operations: the move, then the split.
    assert!(history.undo(&mut session).unwrap());
    assert!(session.tracks[0].clip(right_id).is_some());
    assert!(history.undo(&mut session).unwrap());
    assert_eq!(session.tracks[0].clips.len(), 1);
    assert_eq!(session.tracks[0].clips[0], {
        demo_session().tracks[0].clips[0].clone()
    });

    // Redo both.
    assert!(history.redo(&mut session).unwrap());
    assert!(history.redo(&mut session).unwrap());
    assert_eq!(session.tracks[0].clips.len(), 1);
    assert_eq!(session.tracks[1].clips.len(), 2);

    // Serialize and reload; the document must be identical.
    let json = serde_json::to_string(&session).unwrap();
    let mut reloaded: Session = serde_json::from_str(&json).unwrap();
    assert_eq!(reloaded, session);

    // Id allocation continues past everything created before the save.
    let fresh = reloaded.allocate_clip_id();
    assert!(session.tracks.iter().all(|t| t.clip(fresh).is_none()));
}

#[test]
fn animated_property_values_flow_through_clip_api() {
    let session = demo_session();
    let clip = &session.tracks[0].clips[0];

    assert_eq!(clip.property_value("transform.position.y", 24), -200.0);
    assert_eq!(clip.property_value("transform.position.y", 72), 0.0);
    // Holds the last keyframe value after the end.
    assert_eq!(clip.property_value("transform.position.y", 119), 0.0);
    // Static fallback for unanimated properties.
    assert_eq!(clip.property_value("opacity", 50), 1.0);
}
