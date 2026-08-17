//! Closing operations over the inputs their parameters do not name
//! (#163).
//!
//! #151 recorded `NodeOp { tool, params, engine_version, reproducible }`
//! on every node, which is enough to describe an edit but not always
//! enough to *redo* one. Four tools read something the session does not
//! contain, and `reproducible` is false for all four — so
//! `storage_report` reported zero rebuildable history for any ordinary
//! session, because every chain begins with `load`.
//!
//! The recording was done. The closing-over was not. This module is the
//! closing-over:
//!
//! * **`load`** reads a file elsewhere on disk that may since have moved
//!   or changed. It now records the content hash of the audio it
//!   imported, so a replay can check the source is still the same audio
//!   and refuse by name when it is not — a clear refusal rather than
//!   silently different output.
//! * **`paste_region`** splices an in-memory clipboard that was never
//!   persisted, so after a paste that audio existed *only* inside the
//!   derived file. `copy_region` now writes it to a CAS blob under
//!   `<project>/.audiograph/clipboard/`, and the paste references it.
//! * **`transcribe` / `separate_stems`** read ML model weights, which
//!   are not part of the session and should not be. They record model
//!   identity and stay permanently non-replayable. That is correct
//!   rather than unfortunate: re-running Demucs to reclaim 50 MB is a
//!   bad trade anyway.
//!
//! ## Hashing convention
//!
//! One convention, used everywhere audio is content-addressed here:
//! sample rate, then channel count, then every sample as little-endian
//! bytes. Explicit rather than inherited from a struct layout, so the
//! name a file lands on is stable across platforms and rustc versions.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// Directory under `.audiograph/` where clipboard blobs are kept.
pub const CLIPBOARD_DIR: &str = "clipboard";

/// Content hash of decoded audio: rate, channels, then samples as
/// little-endian bytes.
///
/// Two files whose samples, rate and channel count all match *are* the
/// same audio for every purpose this store has, so they hash the same
/// (#160) — and two genuinely different ones cannot collide without
/// colliding blake3 itself.
pub fn audio_hash(samples: &[f32], sample_rate: u32, channels: u16) -> String {
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(&sample_rate.to_le_bytes());
    hasher.update(&(channels as u32).to_le_bytes());
    hasher.update(&bytes);
    hasher.finalize().to_hex().to_string()
}

/// Directory under `.audiograph/` holding derived audio.
pub const DERIVED_DIR: &str = "derived";

/// Where derived audio lives: `<project>/.audiograph/derived/`.
///
/// Inside the project, not beside the source (#156). Derived files used
/// to be written to `<source_dir>/derived/`, which meant a project
/// folder held only `project.json` and `.audiograph/` while every
/// sample sat next to whatever the user happened to open — usually
/// somewhere else entirely. A project was therefore not a thing you
/// could copy, move or back up, because the audio it points at was not
/// in it. The clipboard blobs and the preview cache already live under
/// `.audiograph/`; this puts the derived audio with them.
pub fn derived_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(session::STORE_DIR).join(DERIVED_DIR)
}

/// Where clipboard blobs live for a project.
pub fn clipboard_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(session::STORE_DIR).join(CLIPBOARD_DIR)
}

/// Persist a clipboard buffer as a CAS blob and return its hash.
///
/// Named by content, so copying the same region twice writes once. The
/// blob is what makes a paste replayable: without it the pasted audio
/// exists only inside the derived file that a sweep might evict, and
/// nothing on disk could recreate it.
pub fn store_clipboard_blob(
    project_dir: &Path,
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
) -> Result<String, String> {
    let hash = audio_hash(samples, sample_rate, channels);
    let dir = clipboard_dir(project_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create clipboard dir {}: {e}", dir.display()))?;
    let path = dir.join(format!("{hash}.wav"));
    if !path.exists() {
        audio_engine::write_wav(samples, sample_rate, channels.max(1), &path)
            .map_err(|e| format!("failed to write clipboard blob {}: {e}", path.display()))?;
    }
    Ok(hash)
}

/// Whether a clipboard blob for `hash` is on disk.
pub fn clipboard_blob_path(project_dir: &Path, hash: &str) -> PathBuf {
    clipboard_dir(project_dir).join(format!("{hash}.wav"))
}

/// The `inputs` an ML tool records: what produced the output, so a
/// mismatch is diagnosable even though a replay is not offered.
pub fn model_inputs(model: &str, version: &str) -> Value {
    json!({
        "model": { "id": model, "version": version },
        // Stated rather than implied. Model weights are not part of the
        // session and should not be, so this can never become
        // replayable by recording more.
        "replayable": false,
    })
}

/// Something a recorded op says it needs that is no longer true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Node the problem is attached to, hex.
    pub node: String,
    /// Tool that recorded the op.
    pub tool: String,
    /// What is wrong, in a sentence that names the thing.
    pub reason: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({}): {}",
            &self.node[..8.min(self.node.len())],
            self.tool,
            self.reason
        )
    }
}

/// Check the chain from a root down to `node`: is everything it closed
/// over still there, and still the same?
///
/// This is what makes "reproducible" mean something. Recording a source
/// hash is only useful if it is checked before a replay trusts it — and
/// the honest answer to "the file changed" is a refusal naming the file,
/// not output that quietly differs from what was recorded.
///
/// An empty result means every op in the chain is replayable *and* its
/// inputs still match. Nodes with no recorded op are reported too:
/// absent provenance reads as "not known to be rebuildable", which is
/// the safe direction.
pub fn verify_chain(store: &session::Store, node: session::NodeId) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut cursor = Some(node);

    while let Some(id) = cursor {
        let Ok(n) = store.get(id) else {
            problems.push(Problem {
                node: id.to_hex(),
                tool: String::new(),
                reason: "node is missing from the store".to_string(),
            });
            break;
        };

        match &n.op {
            None => {
                // A root written before provenance existed, or one
                // produced outside the dispatcher.
                problems.push(Problem {
                    node: id.to_hex(),
                    tool: String::new(),
                    reason: "no operation was recorded for this node".to_string(),
                });
            }
            Some(op) => {
                if !op.reproducible {
                    problems.push(Problem {
                        node: id.to_hex(),
                        tool: op.tool.clone(),
                        reason: reason_for_unreplayable(op),
                    });
                }
                problems.extend(check_inputs(store, &id.to_hex(), op));
            }
        }

        cursor = n.parent;
    }

    problems
}

fn reason_for_unreplayable(op: &session::NodeOp) -> String {
    if op.inputs.get("model").is_some() {
        let id = op.inputs["model"]["id"].as_str().unwrap_or("an ML model");
        format!("reads {id}, whose weights are not part of the session")
    } else {
        "reads something outside the session that was not recorded".to_string()
    }
}

/// Verify one op's recorded inputs against what is on disk now.
fn check_inputs(store: &session::Store, node: &str, op: &session::NodeOp) -> Vec<Problem> {
    let mut problems = Vec::new();

    if let Some(source) = op.inputs.get("source") {
        let path = source.get("path").and_then(Value::as_str).unwrap_or("");
        let recorded = source.get("audio_hash").and_then(Value::as_str);
        let p = Path::new(path);
        if !p.exists() {
            problems.push(Problem {
                node: node.to_string(),
                tool: op.tool.clone(),
                reason: format!("source file is gone: {path}"),
            });
        } else if let Some(expected) = recorded {
            match audio_decoder::decode_file(p) {
                Ok(d) => {
                    let now = audio_hash(&d.samples, d.sample_rate, d.channels);
                    if now != *expected {
                        problems.push(Problem {
                            node: node.to_string(),
                            tool: op.tool.clone(),
                            reason: format!(
                                "source file has changed since it was imported: {path}"
                            ),
                        });
                    }
                }
                Err(e) => problems.push(Problem {
                    node: node.to_string(),
                    tool: op.tool.clone(),
                    reason: format!("source file can no longer be decoded ({path}): {e}"),
                }),
            }
        }
    }

    if let Some(hash) = op.inputs.get("clipboard").and_then(Value::as_str) {
        let blob = clipboard_blob_path(store.project_dir(), hash);
        if !blob.exists() {
            problems.push(Problem {
                node: node.to_string(),
                tool: op.tool.clone(),
                reason: format!(
                    "the pasted audio's clipboard blob is missing: {}",
                    blob.display()
                ),
            });
        }
    }

    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_audio_hashes_the_same_whatever_buffer_holds_it() {
        let a = vec![0.1f32, -0.2, 0.3];
        let b = a.clone();
        assert_eq!(audio_hash(&a, 48_000, 2), audio_hash(&b, 48_000, 2));
    }

    /// Rate and channel count are part of the identity: the same samples
    /// at a different rate are different audio.
    #[test]
    fn rate_and_channels_change_the_hash() {
        let s = vec![0.1f32, -0.2, 0.3];
        assert_ne!(audio_hash(&s, 48_000, 2), audio_hash(&s, 44_100, 2));
        assert_ne!(audio_hash(&s, 48_000, 2), audio_hash(&s, 48_000, 1));
    }

    #[test]
    fn a_clipboard_blob_is_written_once_and_named_by_content() {
        let tmp = tempfile::tempdir().unwrap();
        let samples = vec![0.25f32; 512];
        let first = store_clipboard_blob(tmp.path(), &samples, 48_000, 2).unwrap();
        let path = clipboard_blob_path(tmp.path(), &first);
        assert!(path.exists());
        let written = std::fs::metadata(&path).unwrap().len();

        let second = store_clipboard_blob(tmp.path(), &samples, 48_000, 2).unwrap();
        assert_eq!(first, second, "same audio must land on the same name");
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            written,
            "the blob was rewritten rather than reused"
        );
    }
}
