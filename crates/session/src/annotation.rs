//! Marker / region annotations.
//!
//! This module defines the `Annotation` types only (Task A1 of the
//! audacity-surface plan). Task A2 adds an `annotations: Vec<Annotation>`
//! field to `SessionState`, at which point annotations gain the
//! content-addressed lifetime of the graph — forks see only their own
//! annotation set, and reverting moves the user to a different one
//! automatically. Until A2 lands, these types exist but are not yet
//! attached to any session-graph node.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AnnotationId(pub Uuid);

impl AnnotationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for AnnotationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnnotationKind {
    Marker { time_sec: f64 },
    Region { start_sec: f64, end_sec: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: AnnotationId,
    pub name: String,
    #[serde(flatten)]
    pub kind: AnnotationKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_serializes_to_expected_shape() {
        let a = Annotation {
            id: AnnotationId(Uuid::nil()),
            name: "chorus".into(),
            kind: AnnotationKind::Marker { time_sec: 42.0 },
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["id"], "00000000-0000-0000-0000-000000000000");
        assert_eq!(json["name"], "chorus");
        assert_eq!(json["kind"], "marker");
        assert_eq!(json["time_sec"], 42.0);
        // Round-trip — guards against accidental removal of
        // `#[serde(transparent)]` / `#[serde(flatten)]` attributes.
        let back: Annotation = serde_json::from_value(json).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn region_serializes_to_expected_shape() {
        let a = Annotation {
            id: AnnotationId(Uuid::nil()),
            name: "verse".into(),
            kind: AnnotationKind::Region {
                start_sec: 1.0,
                end_sec: 3.5,
            },
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["id"], "00000000-0000-0000-0000-000000000000");
        assert_eq!(json["kind"], "region");
        assert_eq!(json["start_sec"], 1.0);
        assert_eq!(json["end_sec"], 3.5);
        let back: Annotation = serde_json::from_value(json).unwrap();
        assert_eq!(back, a);
    }
}
