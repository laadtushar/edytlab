//! `generate_tone` and `generate_noise` on a project with no head (#743):
//! they need no input audio, so they start an empty session and their track
//! is the first node.

use serde_json::{json, Value};

#[path = "support/harness.rs"]
mod harness;
use harness::Session;

impl Session {}

fn tone() -> (&'static str, Value) {
    (
        "generate_tone",
        json!({ "frequency_hz": 440.0, "duration_sec": 1.0 }),
    )
}

fn noise() -> (&'static str, Value) {
    ("generate_noise", json!({ "duration_sec": 1.0 }))
}

#[test]
fn each_generator_on_an_empty_project_makes_the_first_node() {
    for (name, args) in [tone(), noise()] {
        let mut p = Session::empty();
        assert!(p.store.head().is_none());
        let out = p.ok(name, args);
        assert_eq!(out["track_index"], 0, "{name}");
        let head = p.store.head().expect("a head now");
        let node = p.store.get(head).unwrap();
        assert!(
            node.parent.is_none(),
            "{name}: the first node has no parent"
        );
        let s = node.state;
        assert_eq!(s.tracks.len(), 1, "{name}");
        assert_eq!(s.sample_rate, 44_100, "{name}");
        assert_eq!(s.length_samples, 44_100, "{name}");
        let clip = &s.tracks[0].clips[0];
        assert_eq!(clip.length, 44_100, "{name}");
        assert!(clip.source_path.starts_with(p.dir.path()), "{name}");
        assert!(clip.source_path.exists(), "{name}");
    }
}

#[test]
fn both_generators_still_append_to_a_project_with_a_head() {
    let mut p = Session::empty();
    let (n, a) = tone();
    p.ok(n, a);
    let before = p.store.head().unwrap();
    let (n, a) = noise();
    let out = p.ok(n, a);
    assert_eq!(out["track_index"], 1);
    let (n, a) = tone();
    let out = p.ok(n, a);
    assert_eq!(out["track_index"], 2);
    let s = p.state();
    assert_eq!(s.tracks.len(), 3);
    assert_eq!(s.sample_rate, 44_100);
    assert_ne!(p.store.head().unwrap(), before);
}
