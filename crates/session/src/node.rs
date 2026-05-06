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
    pub fn from_state(state: &SessionState) -> Result<Self> {
        // Canonical-ish: serde_json's struct field ordering is the field
        // declaration order, which is deterministic for a given Rust build.
        // Snapshot tests pin the exact byte layout. Phase 2 may switch to a
        // sort-keys serializer if/when we accept untrusted input that may
        // reorder map keys.
        let bytes = serde_json::to_vec(state)?;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub created_at: DateTime<Utc>,
    pub label: Option<String>,
    pub reasoning: Option<String>,
    pub state: SessionState,
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
