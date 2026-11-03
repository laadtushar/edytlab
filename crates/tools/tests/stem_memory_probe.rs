//! Opt-in Linux/glibc RSS probe for the real stem-separation tool (#868).
//!
//! Run in a fresh process with `ORT_DYLIB_PATH` and `DEMUCS_MODEL_PATH` set:
//! `cargo test -p tools --release --test stem_memory_probe -- --ignored --nocapture`

#![cfg(all(target_os = "linux", target_env = "gnu"))]

#[path = "support/harness.rs"]
mod harness;

use harness::Session;
use serde_json::json;
use std::time::Instant;

extern "C" {
    fn malloc_trim(pad: usize) -> std::ffi::c_int;
}

fn memory_mib() -> (f64, f64) {
    let status = std::fs::read_to_string("/proc/self/status").expect("Linux process status");
    let value = |key: &str| {
        status
            .lines()
            .find(|line| line.starts_with(key))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|v| v.parse::<f64>().ok())
            .expect("RSS or high-water mark")
            / 1024.0
    };
    (value("VmRSS:"), value("VmHWM:"))
}

#[test]
#[ignore = "requires the pinned ONNX Runtime and htdemucs model; measures real RSS"]
fn real_tool_rss_after_separation_and_second_trim() {
    const STEM_PROBE_SECONDS: &str = "STEM_PROBE_SECONDS";
    for key in ["ORT_DYLIB_PATH", "DEMUCS_MODEL_PATH"] {
        assert!(std::env::var_os(key).is_some(), "{key} is required");
    }
    let seconds: usize = std::env::var(STEM_PROBE_SECONDS)
        .unwrap_or_else(|_| "102".to_string())
        .parse()
        .expect("STEM_PROBE_SECONDS must be a positive integer");
    assert!(seconds > 0, "STEM_PROBE_SECONDS must be positive");
    let mut project = Session::with_sine(44_100, seconds);
    let before = memory_mib();
    let start = Instant::now();
    let result = project.ok("separate_stems", json!({"track": 0}));
    let elapsed = start.elapsed();
    assert_eq!(result["cached"].as_bool(), Some(false));
    // The real tool already trimmed before returning. This is not an
    // untrimmed baseline; the following call is a second, diagnostic trim.
    let after_tool_trim = memory_mib();
    // SAFETY: glibc's malloc_trim accepts any padding value, has no borrowed
    // pointers, and only asks the allocator to return unused pages.
    let trim_started = Instant::now();
    let returned = unsafe { malloc_trim(0) };
    let trim_elapsed = trim_started.elapsed();
    let after_second_trim = memory_mib();
    eprintln!(
        "stem RSS MiB: before={:.1}, peak={:.1}, after_tool_trim={:.1}, after_second_trim={:.1}; audio_s={seconds}; second_trim_return={returned}; elapsed={:.2}s; second_trim_ms={:.2}",
        before.0,
        after_tool_trim.1,
        after_tool_trim.0,
        after_second_trim.0,
        elapsed.as_secs_f64(),
        trim_elapsed.as_secs_f64() * 1000.0
    );
    if let Some(limit) = std::env::var_os("STEM_MAX_POST_MIB") {
        let limit: f64 = limit
            .to_string_lossy()
            .parse()
            .expect("STEM_MAX_POST_MIB must be a number");
        assert!(
            after_tool_trim.0 <= limit,
            "post-run RSS {:.1} MiB exceeded the local measurement limit {limit:.1} MiB",
            after_tool_trim.0
        );
    }
}
