//! Fuzz the `.cube` LUT parser: it must never panic on arbitrary input.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = tpt_av_visual_color::luts::parse_cube(text);
    }
});
