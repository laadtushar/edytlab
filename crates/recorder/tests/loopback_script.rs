//! `scripts/audio-loopback.sh` stands aside for a recording session (#203).
//!
//! The machine's sound is shared with whoever records the demo videos, who
//! marks a session with a `recording.lock`. The script has to refuse before
//! it starts anything, so this needs neither PulseAudio nor a device.

#![cfg(unix)]

use std::process::Command;

fn script() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/audio-loopback.sh")
}

#[test]
fn a_recording_lock_stops_the_loopback_before_anything_starts() {
    let dir = tempfile::tempdir().unwrap();
    let lock = dir.path().join("recording.lock");
    std::fs::write(&lock, "").unwrap();

    let empty_bin = dir.path().join("bin");
    std::fs::create_dir(&empty_bin).unwrap();
    let out = Command::new("/bin/bash")
        .arg(script())
        .env("EDYTLAB_RECORDING_LOCK", &lock)
        // A path with no pulseaudio, so a script that got past the lock
        // would stop at 2, not 4, and could never start a daemon.
        .env("PATH", &empty_bin)
        .output()
        .expect("bash runs the script");

    assert_eq!(out.status.code(), Some(4), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("recording.lock"), "{err}");
    assert!(err.contains("not running the loopback"), "{err}");
}

#[test]
fn without_a_lock_the_script_gets_past_the_check() {
    let dir = tempfile::tempdir().unwrap();
    let empty_bin = dir.path().join("bin");
    std::fs::create_dir(&empty_bin).unwrap();
    // `TMPDIR` is a fresh folder with no lock, and the PATH has no
    // pulseaudio: the next thing the script does is say so (exit 2).
    let out = Command::new("/bin/bash")
        .arg(script())
        .env("TMPDIR", dir.path())
        .env_remove("EDYTLAB_RECORDING_LOCK")
        .env("PATH", &empty_bin)
        .output()
        .expect("bash runs the script");
    // Skipped, not failed, where a real lock happens to sit at one of the
    // fixed places on this machine.
    if out.status.code() == Some(4) {
        return;
    }
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("pulseaudio is not installed"));
}
