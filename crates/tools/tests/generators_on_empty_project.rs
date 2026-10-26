//! `generate_tone` and `generate_noise` on a project with no head (#743):
//! they need no input audio, so they start an empty session and their track
//! is the first node.

use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

struct Project {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Project {
    fn empty() -> Self {
        let dir = TempDir::new().unwrap();
        let store = session::Store::open(dir.path()).unwrap();
        Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        }
    }

    fn ok(&mut self, tool: &str, args: Value) -> Value {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        match self.dispatcher.invoke(tool, args, &mut ctx).unwrap() {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("{tool}: expected Ok, got Error({m})"),
        }
    }

    fn state(&self) -> session::SessionState {
        self.store
            .get(self.store.head().expect("a head"))
            .unwrap()
            .state
    }
}

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
        let mut p = Project::empty();
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
    let mut p = Project::empty();
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
