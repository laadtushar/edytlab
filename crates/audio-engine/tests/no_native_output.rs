//! `audio-engine` renders offline; the app plays audio in the webview
//! (WaveSurfer / `<audio>` over the asset protocol). So this crate must
//! not link an audio-output backend.
//!
//! Before #388 it reached cpal through `audio-io`, for a `play_state`
//! that nothing called.
//!
//! The test walks `Cargo.lock` instead of running `cargo tree`, so it
//! runs offline, takes no cargo lock and covers every target platform at
//! once. `Cargo.lock` does not separate normal, dev and build edges, so
//! the walk over-approximates, which is the safe direction for a
//! must-not-reach check.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

fn lockfile() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

/// Package name -> names of the packages it depends on. A package that
/// appears in several versions is merged by name.
fn dependency_map(lock: &str) -> HashMap<String, HashSet<String>> {
    let mut map: HashMap<String, HashSet<String>> = HashMap::new();
    let mut current: Option<String> = None;
    let mut in_deps = false;

    // `trim` also drops the `\r` of a CRLF checkout.
    for line in lock.lines().map(str::trim) {
        if line == "[[package]]" {
            current = None;
            in_deps = false;
        } else if in_deps {
            if line.starts_with(']') {
                in_deps = false;
            } else if let Some(pkg) = &current {
                // `"thiserror 2.0.18",` -> `thiserror`
                let name = line
                    .trim_end_matches(',')
                    .trim_matches('"')
                    .split(' ')
                    .next()
                    .unwrap_or_default();
                map.entry(pkg.clone()).or_default().insert(name.to_string());
            }
        } else if let Some(name) = line
            .strip_prefix("name = \"")
            .and_then(|rest| rest.strip_suffix('"'))
        {
            map.entry(name.to_string()).or_default();
            current = Some(name.to_string());
        } else if line.starts_with("dependencies = [") {
            in_deps = !line.ends_with(']');
        }
        // Everything else (`version`, `source`, `checksum`, the file
        // header) is irrelevant here.
    }
    map
}

/// Shortest dependency chain from `from` to `to`, if there is one.
fn path_to(map: &HashMap<String, HashSet<String>>, from: &str, to: &str) -> Option<Vec<String>> {
    let mut parent: HashMap<&str, &str> = HashMap::new();
    let mut seen: HashSet<&str> = HashSet::from([from]);
    let mut queue: VecDeque<&str> = VecDeque::from([from]);

    while let Some(node) = queue.pop_front() {
        if node == to {
            let mut chain = vec![node.to_string()];
            let mut at = node;
            while let Some(&prev) = parent.get(at) {
                chain.push(prev.to_string());
                at = prev;
            }
            chain.reverse();
            return Some(chain);
        }
        let Some(deps) = map.get(node) else { continue };
        // Sorted, so the reported chain is the same on every run.
        let mut next: Vec<&str> = deps.iter().map(String::as_str).collect();
        next.sort_unstable();
        for dep in next {
            if seen.insert(dep) {
                parent.insert(dep, node);
                queue.push_back(dep);
            }
        }
    }
    None
}

#[test]
fn audio_engine_links_no_audio_output_backend() {
    let map = dependency_map(&lockfile());

    // Guard the guard: a rename of the crate, or a parser that stopped
    // seeing dependency lists, must not make this pass vacuously.
    assert!(
        map.contains_key("audio-engine"),
        "audio-engine is not in Cargo.lock; was the crate renamed?"
    );
    assert!(
        path_to(&map, "audio-engine", "symphonia").is_some(),
        "the lockfile walk found no audio-engine -> audio-decoder -> symphonia \
         chain, so it is not following transitive dependencies"
    );
    // cpal stays in the lock through crates/recorder (microphone capture).
    // It proves the banned names below are spelled the way the lock spells them.
    assert!(
        map.contains_key("cpal"),
        "cpal is not in Cargo.lock; if recorder no longer uses cpal, drop this line"
    );

    // The backends come before the `audio-io` wrapper so a failure names
    // the backend: `audio-engine -> audio-io -> cpal`, not just `-> audio-io`.
    for banned in ["cpal", "alsa", "coreaudio-rs", "oboe", "audio-io"] {
        if let Some(chain) = path_to(&map, "audio-engine", banned) {
            panic!(
                "audio-engine reaches {banned} via {}; it renders offline and \
                 must not link an audio-output backend (#388)",
                chain.join(" -> ")
            );
        }
    }
}
