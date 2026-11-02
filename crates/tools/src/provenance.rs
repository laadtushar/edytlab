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
//!
//! Every file the project writes under a name made from its samples gets
//! that name from [`audio_hash`]: a destructive edit's output, a `load`
//! copy, a generated tone or noise, a clipboard blob. Until #620 an edit's
//! output and a generated tone or noise were named by the samples alone,
//! which two buffers of a different rate or channel count can share (a file
//! under a name is reused, not written, so the second was handed the
//! first's). [`legacy_sample_hash_name`] is how those older names are
//! recognised; nothing writes one. (A mixed-down track is named by the
//! blake3 of its WAV file, header included, which holds the layout too.)

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use session::audio_store::{AudioFolder, AudioName, AudioStore};

// The names and the folders of the project's audio live in
// `session::audio_store` (#719); they stay reachable from here, where every
// caller has always found them.
pub use session::audio_store::{
    audio_hash, clipboard_dir, derived_dir, legacy_sample_hash, legacy_sample_hash_name,
    legacy_stem, CLIPBOARD_DIR, DERIVED_DIR,
};

/// A clipboard buffer kept in the project: its hash (what a paste records)
/// and where the store put it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardBlob {
    pub hash: String,
    pub path: PathBuf,
}

/// Persist a clipboard buffer as a CAS blob and return its hash and path.
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
) -> Result<ClipboardBlob, String> {
    let name = AudioName::of_samples(samples, sample_rate, channels);
    let store = AudioStore::new(project_dir);
    let path = store.path_of(AudioFolder::Clipboard, &name);
    // Written as float so it reads back as exactly these samples: a paste
    // replayed from a lossy blob would splice in different audio, and its
    // output would never match the file it is meant to rebuild. A blob
    // already there is kept only if it does read back exactly; one written
    // as 16-bit before this was float is replaced, whole (#719: the one
    // folder whose files can be checked against their name).
    store
        .put_verified(
            AudioFolder::Clipboard,
            &name,
            |_| load_clipboard_blob(project_dir, name.as_str()).is_ok(),
            |tmp| {
                audio_engine::write_wav_f32(samples, sample_rate, channels.max(1), tmp)
                    .map_err(|e| e.to_string())
            },
        )
        .map_err(|e| format!("failed to write clipboard blob {}: {e}", path.display()))?;
    Ok(ClipboardBlob {
        hash: name.as_str().to_string(),
        path,
    })
}

/// Read back the clipboard a paste used, checked against its name.
///
/// A replayed paste starts with an empty clipboard; this is what it
/// splices instead (#377). The name is the hash of the samples, so a blob
/// whose samples hash to anything else — missing, damaged, or written
/// lossily before blobs were float — is refused rather than pasted: the
/// replay would produce audio that is not what the node describes.
pub fn load_clipboard_blob(project_dir: &Path, hash: &str) -> Result<crate::Clipboard, String> {
    let path = clipboard_blob_path(project_dir, hash);
    let mut reader = hound::WavReader::open(&path)
        .map_err(|e| format!("cannot read clipboard blob {}: {e}", path.display()))?;
    let spec = reader.spec();
    let samples: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, 32) => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| format!("cannot read clipboard blob {}: {e}", path.display()))?,
        (hound::SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|s| s.map(|v| f32::from(v) / 32_768.0))
            .collect::<Result<_, _>>()
            .map_err(|e| format!("cannot read clipboard blob {}: {e}", path.display()))?,
        (format, bits) => {
            return Err(format!(
                "clipboard blob {} is {bits}-bit {format:?}, which no clipboard is written as",
                path.display()
            ))
        }
    };
    if audio_hash(&samples, spec.sample_rate, spec.channels) != hash {
        return Err(format!(
            "clipboard blob {} does not hold the audio it is named for",
            path.display()
        ));
    }
    Ok(crate::Clipboard {
        samples,
        sample_rate: spec.sample_rate,
        channels: spec.channels,
    })
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

    /// The names on disk already depend on these bytes: hashing a slice at
    /// a time must give what hashing the whole buffer did, at every length
    /// around a slice's edge.
    #[test]
    fn hashing_in_slices_gives_the_hash_of_the_whole_buffer() {
        for n in [0, 1, 4095, 4096, 4097, 10_000] {
            let samples: Vec<f32> = (0..n).map(|i| (i as f32 * 0.37).sin()).collect();
            let mut bytes = Vec::new();
            for s in &samples {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
            let mut whole = blake3::Hasher::new();
            whole.update(&48_000u32.to_le_bytes());
            whole.update(&2u32.to_le_bytes());
            whole.update(&bytes);
            assert_eq!(
                audio_hash(&samples, 48_000, 2),
                whole.finalize().to_hex().to_string(),
                "{n} samples"
            );
            assert_eq!(
                legacy_sample_hash(&samples),
                *blake3::hash(&bytes).as_bytes(),
                "the older hash is the samples alone, {n} of them"
            );
        }
    }

    /// The bug, in one place: the older name cannot tell two layouts apart,
    /// and the new one can.
    #[test]
    fn the_older_name_ignores_the_layout_and_the_new_one_does_not() {
        let samples = vec![0.1f32, -0.2, 0.3, 0.4];
        assert_eq!(
            legacy_sample_hash_name(&samples),
            legacy_stem(&legacy_sample_hash(&samples))
        );
        assert_ne!(
            audio_hash(&samples, 48_000, 1),
            audio_hash(&samples, 24_000, 2)
        );
        assert_ne!(
            audio_hash(&samples, 48_000, 1),
            legacy_sample_hash_name(&samples)
        );
    }

    #[test]
    fn a_clipboard_blob_is_written_once_and_named_by_content() {
        let tmp = tempfile::tempdir().unwrap();
        let samples = vec![0.25f32; 512];
        let first = store_clipboard_blob(tmp.path(), &samples, 48_000, 2)
            .unwrap()
            .hash;
        let path = clipboard_blob_path(tmp.path(), &first);
        assert!(path.exists());
        let written = std::fs::metadata(&path).unwrap().len();

        let second = store_clipboard_blob(tmp.path(), &samples, 48_000, 2)
            .unwrap()
            .hash;
        assert_eq!(first, second, "same audio must land on the same name");
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            written,
            "the blob was rewritten rather than reused"
        );
    }
}
