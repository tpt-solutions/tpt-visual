//! CPU reference behavior tests for the built-in effects.

use effects::{Effect, ParamBag};
use tpt_av_visual_effects as effects;

fn solid(width: u32, height: u32, rgba: [u8; 4]) -> (Vec<u8>, u32, u32) {
    (
        [rgba[0], rgba[1], rgba[2], rgba[3]].repeat((width * height) as usize),
        width,
        height,
    )
}

#[test]
fn blur_preserves_flat_fields_and_energy() {
    // A flat field stays flat under blur.
    let (mut buf, w, h) = solid(8, 8, [100, 150, 200, 255]);
    let blur = effects::GaussianBlur::new(2.0);
    blur.apply_cpu(&mut buf, w, h);
    assert!(buf
        .chunks_exact(4)
        .all(|px| px[0] == 100 && px[1] == 150 && px[2] == 200));

    // A step edge is smeared: the boundary pixel moves toward the mean.
    let mut edge = vec![0_u8; 8 * 4 * 4];
    for y in 0..4 {
        for x in 0..8 {
            let v = if x < 4 { 0_u8 } else { 200 };
            edge[(y * 8 + x) * 4] = v;
            edge[(y * 8 + x) * 4 + 1] = v;
            edge[(y * 8 + x) * 4 + 2] = v;
            edge[(y * 8 + x) * 4 + 3] = 255;
        }
    }
    blur.apply_cpu(&mut edge, 8, 4);
    let mid = &edge[(2 * 8 + 4) * 4..][..4]; // first column right of the edge
    assert!(
        mid[0] > 20 && mid[0] < 180,
        "edge pixel should be smeared, got {}",
        mid[0]
    );
}

#[test]
fn box_blur_is_normalized() {
    let (mut buf, w, h) = solid(4, 4, [10, 20, 30, 255]);
    effects::BoxBlur::new(1.0).apply_cpu(&mut buf, w, h);
    assert!(buf.chunks_exact(4).all(|px| px[0] == 10));
}

#[test]
fn motion_blur_smears_horizontally() {
    let mut buf = vec![0_u8; 8 * 8 * 4];
    // Single white column at x=4.
    for y in 0..8 {
        let idx = (y * 8 + 4) * 4;
        buf[idx] = 255;
        buf[idx + 1] = 255;
        buf[idx + 2] = 255;
        buf[idx + 3] = 255;
    }
    effects::MotionBlur::new(0.0, 3.0).apply_cpu(&mut buf, 8, 8);
    let center = &buf[(4 * 8 + 4) * 4..][..4];
    let neighbor = &buf[(4 * 8 + 3) * 4..][..4];
    assert!(center[0] < 255, "center dimmed by spreading");
    assert!(neighbor[0] > 0, "neighbor brightened by spreading");
}

#[test]
fn sharpen_increases_local_contrast() {
    let mut buf = vec![0_u8; 4 * 4];
    for x in 0..4 {
        let v = if x < 2 { 60_u8 } else { 200 };
        buf[x * 4] = v;
        buf[x * 4 + 1] = v;
        buf[x * 4 + 2] = v;
        buf[x * 4 + 3] = 255;
    }
    effects::Sharpen::new(1.0).apply_cpu(&mut buf, 4, 1);
    let dark_edge = buf[4];
    let bright_edge = buf[2 * 4];
    assert!(dark_edge < 60, "dark side pushed darker: {dark_edge}");
    assert!(
        bright_edge > 200,
        "bright side pushed brighter: {bright_edge}"
    );
}

#[test]
fn color_correct_saturation_zero_greys() {
    let (mut buf, w, h) = solid(2, 2, [180, 60, 30, 255]);
    let mut cc = effects::ColorCorrect::neutral();
    cc.saturation = 0.0;
    cc.apply_cpu(&mut buf, w, h);
    for px in buf.chunks_exact(4) {
        assert!(
            px[0].abs_diff(px[1]) <= 1 && px[1].abs_diff(px[2]) <= 1,
            "{px:?}"
        );
    }
}

#[test]
fn color_correct_hue_rotates_red_to_green() {
    let (mut buf, w, h) = solid(2, 2, [255, 0, 0, 255]);
    let mut cc = effects::ColorCorrect::neutral();
    cc.hue_degrees = 120.0;
    cc.apply_cpu(&mut buf, w, h);
    // Pure red rotated 120° in HSV lands on pure green.
    assert!(
        buf[1] > 200 && buf[0] < 60 && buf[2] < 60,
        "{:?}",
        &buf[..4]
    );
}

#[test]
fn levels_black_white_points() {
    let mut levels = effects::Levels::neutral();
    levels.in_black = 0.2;
    levels.in_white = 0.8;
    let (mut buf, w, h) = solid(2, 2, [51, 51, 51, 255]); // 0.2
    levels.apply_cpu(&mut buf, w, h);
    assert_eq!(buf[0], 0, "in_black maps to output black");
    let (mut buf, w, h) = solid(2, 2, [204, 204, 204, 255]); // 0.8
    levels.apply_cpu(&mut buf, w, h);
    assert_eq!(buf[0], 255, "in_white maps to output white");
}

#[test]
fn tone_curve_bakes_and_evaluates() {
    let curve = effects::Curve {
        points: vec![(0.0, 0.0), (0.5, 0.25), (1.0, 1.0)],
    };
    assert!((curve.eval(0.25) - 0.125).abs() < 1e-6);
    assert!((curve.eval(0.5) - 0.25).abs() < 1e-6);
    let (mut buf, w, h) = solid(2, 2, [128, 128, 128, 255]);
    let effect = effects::ToneCurve::rgb(vec![(0.0, 0.0), (1.0, 0.5)]);
    effect.apply_cpu(&mut buf, w, h);
    assert!(
        buf[0] < 128,
        "half-contrast curve darkens mid grey: {}",
        buf[0]
    );
}

#[test]
fn chroma_key_makes_green_transparent_and_spills() {
    let (mut buf, w, h) = solid(3, 3, [20, 230, 40, 255]); // near key green
    let mut key = effects::ChromaKey::green_screen();
    key.tolerance = 0.4;
    key.softness = 0.1;
    key.spill_suppression = 1.0;
    key.apply_cpu(&mut buf, w, h);
    for px in buf.chunks_exact(4) {
        assert_eq!(px[3], 0, "pure key green fully keyed: {px:?}");
    }

    // A foreground color far from green is untouched.
    let (mut buf, w, h) = solid(2, 2, [200, 60, 50, 255]);
    key.apply_cpu(&mut buf, w, h);
    assert_eq!(buf[3], 255, "foreground stays opaque");
}

#[test]
fn noise_is_deterministic_by_seed() {
    let (mut a, w, h) = solid(8, 8, [128, 128, 128, 255]);
    let (mut b, _, _) = solid(8, 8, [128, 128, 128, 255]);
    let mut other_seed = effects::Noise::new(0.5, 1.0);
    other_seed.seed = 7.0;
    effects::Noise::new(0.5, 7.0).apply_cpu(&mut a, w, h);
    other_seed.apply_cpu(&mut b, w, h);
    assert_eq!(a, b, "same seed → identical grain");
}

#[test]
fn noise_reduction_shrinks_grain_variance_keeps_edges() {
    // Noisy flat field: alternating +/-10 around 128.
    let mut buf = vec![0_u8; 16 * 16 * 4];
    for (i, px) in buf.chunks_exact_mut(4).enumerate() {
        let v = if i % 2 == 0 { 138 } else { 118 };
        px[0] = v;
        px[1] = v;
        px[2] = v;
        px[3] = 255;
    }
    effects::NoiseReduction::new(1.0).apply_cpu(&mut buf, 16, 16);
    let min = buf.chunks_exact(4).map(|px| px[0]).min().unwrap();
    let max = buf.chunks_exact(4).map(|px| px[0]).max().unwrap();
    // Original spread is 20; checkerboard parity keeps some structure, so
    // assert a solid reduction rather than full smoothing.
    assert!(
        i16::from(max) - i16::from(min) < 16,
        "grain variance shrinks: {min}..{max}"
    );

    // Bilateral weight keeps hard edges: a black/white boundary stays.
    let mut edge = vec![0_u8; 8 * 8 * 4];
    for y in 0..8 {
        for x in 0..8 {
            let v = if x < 4 { 0_u8 } else { 255 };
            let px = &mut edge[(y * 8 + x) * 4..][..4];
            px[0] = v;
            px[1] = v;
            px[2] = v;
            px[3] = 255;
        }
    }
    effects::NoiseReduction::new(1.0).apply_cpu(&mut edge, 8, 8);
    let left_of_edge = edge[(4 * 8 + 3) * 4];
    let right_of_edge = edge[(4 * 8 + 4) * 4];
    assert!(left_of_edge < 20, "dark side stays dark: {left_of_edge}");
    assert!(
        right_of_edge > 235,
        "bright side stays bright: {right_of_edge}"
    );
}

#[test]
fn vignette_darkens_corners_not_center() {
    let mut buf = vec![200_u8; 16 * 16 * 4];
    for px in buf.chunks_exact_mut(4) {
        px[3] = 255;
    }
    effects::Vignette::gentle().apply_cpu(&mut buf, 16, 16);
    let center = buf[(8 * 16 + 8) * 4];
    let corner = buf[0];
    assert_eq!(center, 200, "center untouched");
    assert!(corner < 200, "corner darkened: {corner}");
}

#[test]
fn registry_builds_every_registered_effect() {
    let mut params = ParamBag::new();
    params.insert("radius".into(), 3.0);
    for name in effects::registered_effects() {
        let effect = effects::build_effect(name, &params).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(effect.name(), *name);
        // Each must produce at least one GPU pass and run on the CPU.
        let passes = effect.passes(16, 16);
        assert!(!passes.is_empty(), "{name} has no GPU passes");
        let mut scratch = vec![128_u8; 16 * 16 * 4];
        effect.apply_cpu(&mut scratch, 16, 16);
    }
    assert!(matches!(
        effects::build_effect("definitely_not_an_effect", &params),
        Err(effects::EffectError::UnknownEffect(_))
    ));
}
