//! Fuzz the session JSON deserializer: malformed documents must produce
//! errors, never panics.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(session) = tpt_av_visual_timeline::Session::from_json(text) {
            // A successfully parsed session must re-serialize losslessly.
            let round_trip = session.to_json_string(false).expect("re-serialize");
            let reparsed =
                tpt_av_visual_timeline::Session::from_json(&round_trip).expect("re-parse");
            assert_eq!(session, reparsed);
        }
    }
});
