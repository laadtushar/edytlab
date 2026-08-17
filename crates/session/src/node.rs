//! Node identity and the on-disk node record.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::state::SessionState;
use crate::{Error, Result};

/// Content-addressed identifier for a [`SessionState`].
///
/// The hash covers only the state, not the surrounding metadata
/// (timestamp, label, reasoning), so two nodes describing the same
/// session content will share an id even if authored at different
/// times or with different labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(#[serde(with = "hex_array_32")] pub [u8; 32]);

impl NodeId {
    /// Content-hash a [`SessionState`] into a stable id.
    ///
    /// Canonicalization is explicit, not "whatever serde happens to do":
    /// the state is converted to a [`serde_json::Value`], object keys
    /// are sorted recursively, and the resulting bytes are hashed. This
    /// pins the id against accidental drift from serde version bumps or
    /// future struct-field reorderings.
    ///
    /// Reference digests for the canonical form are pinned by snapshot
    /// tests; do not change `canonicalize_value` without also updating
    /// (and re-reviewing) those snapshots.
    pub fn from_state(state: &SessionState) -> Result<Self> {
        let value = serde_json::to_value(state)?;
        let canonical = canonicalize_value(value);
        // `to_string` on a sorted-keys `Value` is deterministic byte-for-byte.
        let bytes = serde_json::to_vec(&canonical)?;
        let hash = blake3::hash(&bytes);
        Ok(NodeId(*hash.as_bytes()))
    }

    pub fn to_hex(self) -> String {
        let mut s = String::with_capacity(64);
        for b in self.0 {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    pub fn from_hex(s: &str) -> Result<Self> {
        if s.len() != 64 {
            return Err(Error::HexDecode(format!(
                "expected 64 hex chars, got {}",
                s.len()
            )));
        }
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            let chunk = &s[i * 2..i * 2 + 2];
            *byte = u8::from_str_radix(chunk, 16)
                .map_err(|e| Error::HexDecode(format!("byte {i}: {e}")))?;
        }
        Ok(NodeId(out))
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Recursively canonicalize a JSON value: object keys are sorted; array
/// element order is preserved (it is semantically meaningful in
/// `SessionState` — e.g. effects chain order).
fn canonicalize_value(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut sorted = serde_json::Map::with_capacity(entries.len());
            for (k, v) in entries {
                sorted.insert(k, canonicalize_value(v));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize_value).collect()),
        other => other,
    }
}

/// How a node was produced, so the audio it points at can be rebuilt
/// rather than merely kept.
///
/// Every destructive edit writes a content-addressed WAV and none are
/// ever deleted (#98). The obvious fix — evict the cold ones and rebuild
/// on demand — needs something nothing recorded: *which* edit to replay.
/// `label` is a human sentence ("gain track 0 +6 dB") and was never a
/// contract; the transform itself was an anonymous closure.
///
/// This is that record. Together with `parent`, a node carries the whole
/// derivation: parent state, tool, parameters.
///
/// ## Why this is safe to replay
///
/// A derived file is named `blake3(post-edit samples)`. The filename is
/// therefore a checksum of the result a replay must produce — so a
/// rebuild is *self-verifying*, and a replay that drifts (a DSP fix
/// landing between the original edit and the rebuild) is detected rather
/// than silently substituted. That property is why recording the
/// operation is enough, and it is worth stating plainly because
/// content-addressed does not by itself mean reproducible.
///
/// `engine_version` is recorded for diagnosis when a rebuild does drift.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeOp {
    /// Registered tool name, e.g. `"reverb"`.
    pub tool: String,
    /// The arguments it was called with, exactly as validated.
    pub params: serde_json::Value,
    /// Workspace version of the code that ran it.
    pub engine_version: String,
    /// False when the tool reads something the session does not contain
    /// — an in-memory clipboard, a file elsewhere on disk, an ML model —
    /// so replaying it here cannot be expected to reproduce the bytes.
    pub reproducible: bool,
    /// What the op read that its `params` do not name (#163).
    ///
    /// A tool that reads outside the session can still be replayable if
    /// it *closes over* what it read: `paste_region` records the CAS
    /// blob its clipboard was persisted to, `load` records the content
    /// hash of the file it imported. A replay checks these before
    /// running, so a source that has changed underneath is a clear
    /// refusal naming the file rather than silently different output.
    ///
    /// Some inputs cannot be closed over and are recorded for diagnosis
    /// instead: `transcribe` and `separate_stems` name their model and
    /// version, and stay permanently non-replayable — re-running Demucs
    /// to reclaim 50 MB is a bad trade regardless.
    ///
    /// `null` on nodes written before this existed, which reads the same
    /// as "nothing was closed over".
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub inputs: serde_json::Value,
}

impl NodeOp {
    /// An op with no closed-over inputs — the shape every tool that
    /// reads only the session produces.
    pub fn new(tool: String, params: serde_json::Value, engine_version: String) -> Self {
        Self {
            tool,
            params,
            engine_version,
            reproducible: true,
            inputs: serde_json::Value::Null,
        }
    }

    /// Mark this op as reading outside the session without closing over
    /// what it read.
    pub fn not_reproducible(mut self) -> Self {
        self.reproducible = false;
        self
    }

    /// Record what the op closed over. Whether that makes it replayable
    /// is the caller's judgement: a persisted clipboard blob does,
    /// a model version does not.
    pub fn with_inputs(mut self, inputs: serde_json::Value) -> Self {
        self.inputs = inputs;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub created_at: DateTime<Utc>,
    pub label: Option<String>,
    pub reasoning: Option<String>,
    pub state: SessionState,
    /// Absent on nodes written before provenance was recorded, and on
    /// any node produced outside the tool dispatcher. Absent means "not
    /// known to be rebuildable", which is the safe reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<NodeOp>,
}

mod hex_array_32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        let mut hex = String::with_capacity(64);
        for b in bytes {
            hex.push_str(&format!("{b:02x}"));
        }
        s.serialize_str(&hex)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        if s.len() != 64 {
            return Err(serde::de::Error::custom(format!(
                "expected 64 hex chars, got {}",
                s.len()
            )));
        }
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            let chunk = &s[i * 2..i * 2 + 2];
            *byte = u8::from_str_radix(chunk, 16).map_err(serde::de::Error::custom)?;
        }
        Ok(out)
    }
}

/// Same hex encoding as [`hex_array_32`] but for `Option<[u8; 32]>`. Used
/// by `Clip::content_hash`, which is `None` until provenance is locked at
/// render time. Serializes `None` as JSON `null`, `Some(_)` as a 64-char
/// hex string — much tidier than the default `Vec<u8>`-as-array form.
pub mod hex_array_32_opt {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Option<[u8; 32]>, s: S) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(arr) => {
                let mut hex = String::with_capacity(64);
                for b in arr {
                    hex.push_str(&format!("{b:02x}"));
                }
                s.serialize_some(&hex)
            }
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[u8; 32]>, D::Error> {
        // Accept either `null` or a 64-char hex string.
        let opt: Option<String> = Option::deserialize(d)?;
        let Some(s) = opt else { return Ok(None) };
        if s.len() != 64 {
            return Err(serde::de::Error::custom(format!(
                "expected 64 hex chars, got {}",
                s.len()
            )));
        }
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            let chunk = &s[i * 2..i * 2 + 2];
            *byte = u8::from_str_radix(chunk, 16).map_err(serde::de::Error::custom)?;
        }
        Ok(Some(out))
    }
}

#[cfg(test)]
mod canonical_hash_tests {
    //! Pinned reference digests for [`NodeId::from_state`].
    //!
    //! Each fixture exercises a different shape (empty, single track,
    //! nested effects with object-typed params, transcript-only,
    //! tempo-map-only). The expected hex is checked into the source so
    //! any accidental change to canonicalization (key sort order, field
    //! reorder, serde version bump) trips this test and forces explicit
    //! review of id drift.
    use super::*;
    use crate::state::{
        BusGraph, Clip, EffectInstance, KeyMap, KeySegment, SessionState, TempoMap, TempoSegment,
        Track, TrackId, Transcript, TranscriptWord,
    };
    use std::path::PathBuf;
    use uuid::Uuid;

    fn empty_state() -> SessionState {
        SessionState {
            tracks: Vec::new(),
            bus_routing: BusGraph::default(),
            master_chain: Vec::new(),
            tempo_map: TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            length_samples: 0,
            annotations: Vec::new(),
        }
    }

    fn single_track_state() -> SessionState {
        let track_uuid = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        SessionState {
            tracks: vec![Track {
                id: TrackId(track_uuid),
                name: "Vox".into(),
                clips: vec![Clip {
                    source_path: PathBuf::from("media/v.wav"),
                    start_in_track: 0,
                    source_offset: 0,
                    length: 1000,
                    content_hash: Some([0x11; 32]),
                    // Defaults; M20 fields are skipped when serialised so
                    // the canonical-hash pinned table stays valid.
                    time_stretch_factor: None,
                    pitch_shift_semitones: None,
                    beat_grid: None,
                    volume_envelope: Vec::new(),
                }],
                gain_db: 0.0,
                pan: 0.0,
                muted: false,
                soloed: false,
                effects: vec![],
                sends: vec![],
            }],
            ..empty_state()
        }
    }

    fn effects_state() -> SessionState {
        SessionState {
            master_chain: vec![EffectInstance {
                kind: "compressor".into(),
                params: serde_json::json!({
                    "ratio": 4.0,
                    "attack_ms": 5.0,
                    "release_ms": 100.0
                }),
                bypassed: false,
            }],
            ..empty_state()
        }
    }

    fn transcript_state() -> SessionState {
        SessionState {
            transcript: Some(Transcript {
                words: vec![TranscriptWord {
                    text: "hi".into(),
                    start_s: 0.0,
                    end_s: 0.25,
                    confidence: 1.0,
                }],
            }),
            ..empty_state()
        }
    }

    fn tempo_key_state() -> SessionState {
        SessionState {
            tempo_map: TempoMap {
                default_bpm: 92.5,
                segments: vec![TempoSegment {
                    start_sample: 0,
                    bpm: 92.5,
                }],
            },
            key_map: Some(KeyMap {
                segments: vec![KeySegment {
                    start_sample: 0,
                    key: "F#min".into(),
                }],
            }),
            length_samples: 96_000,
            ..empty_state()
        }
    }

    /// Pinned reference digests. To regenerate after an *intentional*
    /// canonicalization change, run with `UPDATE_CANONICAL_HASHES=1`
    /// to print the new hex values, then update this table.
    fn pinned_digests() -> &'static [(&'static str, &'static str)] {
        &[
            (
                "empty",
                "2f571864eb05fdff81bee7e30ccab9e8c3b340099d6491af8825e182816b08f9",
            ),
            (
                "single_track",
                "f9743db991c2657f0aab84894da9a6f1a96210beb9509bec1b36f5cd38ae8159",
            ),
            (
                "effects",
                "7af197fcaa2a21a6fa87ce342d4e7d32836acb4812a96615cfac35c289be298a",
            ),
            (
                "transcript",
                "3673b4cf5980a335f1be4a303329602843fd8cd82e4aede04dcd4d718f09d49e",
            ),
            (
                "tempo_key",
                "760c33baf5f28f9f53a123ce441fc4126d90d3c3b92fffd2f6476a7a511d2f67",
            ),
        ]
    }

    #[test]
    fn canonical_digests_match_pinned_table() {
        let fixtures: Vec<(&str, SessionState)> = vec![
            ("empty", empty_state()),
            ("single_track", single_track_state()),
            ("effects", effects_state()),
            ("transcript", transcript_state()),
            ("tempo_key", tempo_key_state()),
        ];

        let computed: Vec<(&str, String)> = fixtures
            .iter()
            .map(|(name, s)| (*name, NodeId::from_state(s).unwrap().to_hex()))
            .collect();

        if std::env::var("UPDATE_CANONICAL_HASHES").is_ok() {
            for (name, hex) in &computed {
                println!("(\"{name}\", \"{hex}\"),");
            }
            panic!("UPDATE_CANONICAL_HASHES set; copy the printed table into pinned_digests().");
        }

        let pinned = pinned_digests();
        for ((name, got), (pname, want)) in computed.iter().zip(pinned.iter()) {
            assert_eq!(name, pname, "fixture name order drift");
            assert_eq!(
                got, want,
                "canonical digest drift for fixture `{name}`: got {got}, want {want}"
            );
        }
    }
}
