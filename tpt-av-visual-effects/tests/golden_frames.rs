//! Bit-exact golden-master tests for the deterministic CPU effect paths,
//! using `tpt-av-test-reference::assert_frame_exact`.
//!
//! Regenerate goldens after an intentional change with
//! `BLESS_GOLDEN=1 cargo test -p tpt-av-visual-effects --test golden_frames`.

use std::path::PathBuf;

use image::{DynamicImage, RgbaImage};
use tpt_av_test_reference::image::assert_frame_exact;
use tpt_av_visual_effects::{BoxBlur, Effect, Sharpen};

const SIZE: u32 = 32;

/// Deterministic RGBA test pattern: diagonal gradient with a hard-edged square.
fn pattern() -> Vec<u8> {
    let mut buf = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let inside = (8..24).contains(&x) && (8..24).contains(&y);
            let (r, g, b) = if inside {
                (240, 64, 32)
            } else {
                ((x * 8) as u8, (y * 8) as u8, ((x + y) * 4) as u8)
            };
            buf.extend_from_slice(&[r, g, b, 255]);
        }
    }
    buf
}

fn check(name: &str, buf: Vec<u8>) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"));
    let img = DynamicImage::ImageRgba8(RgbaImage::from_raw(SIZE, SIZE, buf).unwrap());
    if std::env::var_os("BLESS_GOLDEN").is_some() {
        img.save(&path).unwrap();
    }
    assert_frame_exact(&img, &path).unwrap_or_else(|e| panic!("{name}: {e}"));
}

#[test]
fn box_blur_matches_golden() {
    let mut buf = pattern();
    BoxBlur::new(2.0).apply_cpu(&mut buf, SIZE, SIZE);
    check("box_blur", buf);
}

#[test]
fn sharpen_matches_golden() {
    let mut buf = pattern();
    Sharpen::new(1.0).apply_cpu(&mut buf, SIZE, SIZE);
    check("sharpen", buf);
}
