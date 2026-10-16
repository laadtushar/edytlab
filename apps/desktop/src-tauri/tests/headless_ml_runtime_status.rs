//! `edytlab-desktop --ml-runtime-status` prints the answer Settings'
//! runtime line shows, as one line of JSON, and exits without opening a
//! window (#383).
//!
//! The release workflows run it against every unpacked installer
//! (`scripts/installer-ml-runtime.py check`): the app binary, in the
//! installer's own layout, says which ONNX Runtime library it found and
//! whether that is the bundled one. That is the check that the file is not
//! merely in the package but where this build of the app looks for it.
//!
//! Here there is no bundled library, so the answer is `missing`, and the
//! first place searched is the one `ORT_DYLIB_PATH` names: the flag runs the
//! same search as the command, environment variable first.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Long enough for a cold start on a slow CI runner; a build that ignores
/// the flag starts the window instead and never prints, so it fails here
/// rather than hanging the suite.
const DEADLINE: Duration = Duration::from_secs(60);

#[test]
fn the_status_flag_prints_the_loaders_answer_and_exits_without_a_window() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let pointed = dir.path().join("no-such-onnxruntime-library");

    let mut child = Command::new(env!("CARGO_BIN_EXE_edytlab-desktop"))
        .arg("--ml-runtime-status")
        .env("ORT_DYLIB_PATH", &pointed)
        // No display to open a window on, so a build that ignores the flag
        // fails to start its window instead of showing one.
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the app binary starts");

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("waiting on the app") {
            break status;
        }
        if started.elapsed() > DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!("`--ml-runtime-status` did not exit within {DEADLINE:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let mut stdout = String::new();
    let mut stderr = String::new();
    child
        .stdout
        .take()
        .expect("piped")
        .read_to_string(&mut stdout)
        .expect("stdout is text");
    child
        .stderr
        .take()
        .expect("piped")
        .read_to_string(&mut stderr)
        .expect("stderr is text");

    assert!(
        status.success(),
        "`--ml-runtime-status` exited with {status}\nstdout: {stdout}\nstderr: {stderr}"
    );
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "stdout must be the one JSON line and nothing else, got: {stdout:?}"
    );
    let answer: serde_json::Value =
        serde_json::from_str(lines[0]).unwrap_or_else(|e| panic!("not JSON ({e}): {stdout:?}"));
    assert_eq!(answer["state"], "missing", "{answer}");
    assert_eq!(
        answer["searched"][0],
        pointed.display().to_string(),
        "the environment variable is searched first: {answer}"
    );
}
