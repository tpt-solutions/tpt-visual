//! Session presets bundled with the CLI.
//!
//! Each preset is a complete [`tpt_av_visual::timeline::Session`] JSON
//! document — valid input to `Session::from_json_path` and to
//! `tpt-visual render`. Assets use the `procedural://` pseudo-protocol so
//! every preset renders without media files.

/// The bundled presets, as `(name, description, json)`.
pub const PRESETS: &[(&str, &str, &str)] = &[
    (
        "single-clip",
        "One full-frame clip with a vignette.",
        include_str!("presets/single-clip.json"),
    ),
    (
        "two-track-overlay",
        "A base clip plus a smaller Screen-blended overlay that fades in.",
        include_str!("presets/two-track-overlay.json"),
    ),
    (
        "color-graded",
        "HDR-style grade: PQ input, ACES filmic tone mapping baked into clip color, screen tint.",
        include_str!("presets/color-graded.json"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_is_valid_session_json() {
        for (name, _, json) in PRESETS {
            let session = tpt_av_visual::timeline::Session::from_json(json)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!session.tracks.is_empty(), "{name}: has tracks");
            assert_eq!(session.resolution.width % 2, 0, "{name}: even width");
        }
    }

    #[test]
    fn preset_names_are_unique() {
        let mut names: Vec<&str> = PRESETS.iter().map(|(n, _, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), PRESETS.len());
    }
}
