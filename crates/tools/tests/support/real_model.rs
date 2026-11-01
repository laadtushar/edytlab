//! What the `#[ignore]`d tests that run a real model share: reading the
//! variables that say where the runtime and the model are, and ending the
//! run if a tool never returns.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

/// Abort, rather than stall for ever, if the run is stuck.
pub fn watchdog(test: &'static str, seconds: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(seconds));
        eprintln!(
            "{test} ran for {} minutes: it is stuck, not slow",
            seconds / 60
        );
        std::process::abort();
    });
}

/// The file or folder `var` names. A test asked for with `--ignored` that
/// finds it unset fails: a skip is not an answer (#263).
pub fn required(var: &str, how: &str, folder: bool) -> PathBuf {
    let value = std::env::var_os(var).unwrap_or_else(|| {
        panic!(
            "{var} is not set, so this test cannot run. It was asked for with --ignored, and a \
             skip is not an answer (#263). {how}"
        )
    });
    let path = PathBuf::from(value);
    assert!(
        if folder {
            path.is_dir()
        } else {
            path.is_file()
        },
        "{var} is {}, which is not an existing {}. {how}",
        path.display(),
        if folder { "folder" } else { "file" }
    );
    path
}
