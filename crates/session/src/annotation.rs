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

impl AnnotationKind {
    /// This label moved so that it starts at `start_sec`, keeping its
    /// shape: a marker sits at the new time, a region keeps its length
    /// and its end moves with its start.
    ///
    /// What dragging a label means. A drag names one time, and turning a
    /// region into a point marker at that time would drop its end
    /// silently, so a label that was `3..9` and was moved to 20 is
    /// `20..26`, not `20`.
    pub fn starting_at(&self, start_sec: f64) -> AnnotationKind {
        match *self {
            AnnotationKind::Marker { .. } => AnnotationKind::Marker {
                time_sec: start_sec,
            },
            // Put back where it is, the region is the one it was: adding
            // the length back on to the start would not always give the
            // same end in floating point, and that would read as an edit.
            AnnotationKind::Region {
                start_sec: s,
                end_sec: e,
            } if s == start_sec => AnnotationKind::Region {
                start_sec: s,
                end_sec: e,
            },
            AnnotationKind::Region {
                start_sec: s,
                end_sec: e,
            } => AnnotationKind::Region {
                start_sec,
                end_sec: start_sec + (e - s),
            },
        }
    }
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
    fn a_marker_moves_to_the_new_time() {
        let k = AnnotationKind::Marker { time_sec: 4.0 };
        assert_eq!(k.starting_at(7.5), AnnotationKind::Marker { time_sec: 7.5 });
    }

    #[test]
    fn a_region_moves_with_its_length_and_stays_a_region() {
        let k = AnnotationKind::Region {
            start_sec: 3.0,
            end_sec: 9.0,
        };
        assert_eq!(
            k.starting_at(20.0),
            AnnotationKind::Region {
                start_sec: 20.0,
                end_sec: 26.0
            },
            "six seconds before, six seconds after"
        );
        assert_eq!(
            k.starting_at(0.0),
            AnnotationKind::Region {
                start_sec: 0.0,
                end_sec: 6.0
            },
            "and it can be moved to the start"
        );
    }

    #[test]
    fn a_region_put_back_where_it_is_is_the_region_it_was() {
        // 0.3 - 0.1 + 0.1 is not 0.3 in floating point.
        let k = AnnotationKind::Region {
            start_sec: 0.1,
            end_sec: 0.3,
        };
        assert_eq!(k.starting_at(0.1), k);
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
