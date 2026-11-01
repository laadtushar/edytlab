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

/// The most characters a label's name may hold when the agent sets it. The
/// window's box has no limit; a name goes into the session block and every
/// listing, so a tool refuses one that would fill them.
pub const MAX_NAME_CHARS: usize = 200;

/// A marker's time must be a place on the timeline: finite and not before
/// its start. A label at a negative time cannot be seen, sought to or
/// deleted. There is deliberately no upper bound: the session's length is
/// not known to every caller, and a label past the end stays addressable.
///
/// The one rule the window's `add_marker` and `update_marker` and the
/// agent's marker tools share, so they cannot disagree about what a label
/// may be. The text is what the person or the model reads.
pub fn check_marker_time(time_sec: f64) -> Result<(), String> {
    if !time_sec.is_finite() || time_sec < 0.0 {
        return Err(format!("time must be finite and >= 0; got {time_sec}"));
    }
    Ok(())
}

/// A region must be finite with its end after its start, which is not
/// before the timeline's start.
pub fn check_region(start_sec: f64, end_sec: f64) -> Result<(), String> {
    if !start_sec.is_finite() || !end_sec.is_finite() || start_sec < 0.0 || end_sec <= start_sec {
        return Err(format!(
            "region must be finite with end > start >= 0; got {start_sec}..{end_sec}"
        ));
    }
    Ok(())
}

/// A change to one label, as the window's `update_marker` and the agent's
/// `edit_markers` both ask for it. Every field is optional and a field left
/// out is left alone.
///
/// `time` moves the label to that time: a marker sits there, a region
/// starts there and keeps its length ([`AnnotationKind::starting_at`]).
/// `start` and `end` set a region's edges. The two are not mixed in one
/// edit.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnnotationEdit {
    pub name: Option<String>,
    pub time: Option<f64>,
    pub start: Option<f64>,
    pub end: Option<f64>,
}

impl AnnotationEdit {
    /// The checks that need no stored label: a marker and a region are
    /// different shapes, and accepting both in one edit means silently
    /// picking one. That hides a caller bug (a UI sending `time` beside a
    /// stale `start`), so a mixed edit is refused. A time must be a place
    /// on the timeline, and edges given together must make a region.
    pub fn validate(&self) -> Result<(), String> {
        if self.time.is_some() && (self.start.is_some() || self.end.is_some()) {
            return Err(
                "pass either `time` (a marker) or `start` and `end` (a region), not both".into(),
            );
        }
        if let Some(t) = self.time {
            check_marker_time(t)?;
        }
        if let (Some(s), Some(e)) = (self.start, self.end) {
            check_region(s, e)?;
        }
        Ok(())
    }

    /// The kind `existing` becomes, or `None` when the edit does not touch
    /// its shape or place (a rename alone).
    ///
    /// Both edges make the label a region, whatever it was. One edge
    /// resizes a region and keeps the other; a point marker has no other
    /// edge to keep, so it is refused rather than guessed at.
    pub fn resolve(&self, existing: &AnnotationKind) -> Result<Option<AnnotationKind>, String> {
        self.validate()?;
        if let Some(t) = self.time {
            return Ok(Some(existing.starting_at(t)));
        }
        match (self.start, self.end) {
            (None, None) => Ok(None),
            (Some(start_sec), Some(end_sec)) => {
                Ok(Some(AnnotationKind::Region { start_sec, end_sec }))
            }
            (start, end) => match *existing {
                AnnotationKind::Region {
                    start_sec: s,
                    end_sec: e,
                } => {
                    let (start_sec, end_sec) = (start.unwrap_or(s), end.unwrap_or(e));
                    check_region(start_sec, end_sec)?;
                    Ok(Some(AnnotationKind::Region { start_sec, end_sec }))
                }
                AnnotationKind::Marker { .. } => Err(
                    "a point marker has no start or end to change by itself; give `start` and \
                     `end` to make it a region, or `time` to move it"
                        .into(),
                ),
            },
        }
    }

    /// Whether the edit names nothing to change.
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.time.is_none() && self.start.is_none() && self.end.is_none()
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
    fn an_edit_is_read_the_same_for_every_caller() {
        let marker = AnnotationKind::Marker { time_sec: 4.0 };
        let region = AnnotationKind::Region {
            start_sec: 10.0,
            end_sec: 14.0,
        };
        let edit = |time, start, end| AnnotationEdit {
            name: None,
            time,
            start,
            end,
        };
        // A rename alone leaves the shape.
        assert_eq!(edit(None, None, None).resolve(&marker), Ok(None));
        // `time` moves, keeping a region's length.
        assert_eq!(
            edit(Some(1.0), None, None).resolve(&region),
            Ok(Some(AnnotationKind::Region {
                start_sec: 1.0,
                end_sec: 5.0
            }))
        );
        // One edge resizes a region; a point marker has none to keep.
        assert_eq!(
            edit(None, None, Some(20.0)).resolve(&region),
            Ok(Some(AnnotationKind::Region {
                start_sec: 10.0,
                end_sec: 20.0
            }))
        );
        assert!(edit(None, None, Some(20.0)).resolve(&marker).is_err());
        // Both edges make a region of anything.
        assert_eq!(
            edit(None, Some(1.0), Some(2.0)).resolve(&marker),
            Ok(Some(AnnotationKind::Region {
                start_sec: 1.0,
                end_sec: 2.0
            }))
        );
        // Mixed, backwards and negative are refused.
        assert!(edit(Some(1.0), Some(1.0), Some(2.0)).validate().is_err());
        assert!(edit(None, Some(2.0), Some(2.0)).validate().is_err());
        assert!(edit(Some(-0.5), None, None).validate().is_err());
        assert!(edit(None, Some(12.0), None)
            .resolve(&AnnotationKind::Region {
                start_sec: 10.0,
                end_sec: 11.0
            })
            .is_err());
        assert!(check_marker_time(f64::NAN).is_err());
        assert!(check_marker_time(0.0).is_ok());
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
