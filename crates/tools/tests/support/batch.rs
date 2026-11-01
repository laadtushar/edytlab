//! What the batch tests need beside the session: a recipe to run and a
//! folder of takes to run it over. Include `harness.rs` first.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use serde_json::json;

use super::harness::{ok, write_take, Session};

/// A recipe that loads a file and quietens it, exported from a throwaway
/// session — the same way a user would make one.
pub fn make_recipe(s: &mut Session) -> PathBuf {
    let src = write_take(&s.dir.path().join("template.wav"), 440.0);
    ok(s.call("load", json!({ "path": src.to_string_lossy() })));
    ok(s.call("gain", json!({ "track": 0, "db": -6.0 })));
    let recipe = s.dir.path().join("chain.json");
    ok(s.call(
        "export_recipe",
        json!({ "out_path": recipe.to_string_lossy(), "name": "quieten" }),
    ));
    recipe
}

/// A folder of three takes.
pub fn make_folder(root: &Path) -> PathBuf {
    let dir = root.join("takes");
    std::fs::create_dir_all(&dir).unwrap();
    for (i, freq) in [220.0, 440.0, 880.0].into_iter().enumerate() {
        write_take(&dir.join(format!("take-{i}.wav")), freq);
    }
    // Something that is not audio, which must be ignored rather than
    // reported as a failure.
    std::fs::write(dir.join("notes.txt"), "not audio").unwrap();
    dir
}
