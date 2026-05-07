//! Section segmentation. Phase-2 stub.
//!
//! Real intro/verse/chorus/bridge segmentation is non-trivial (it
//! typically needs a structure-novelty curve over an MFCC SSM, plus a
//! beat-synchronous post-processing pass). For Phase 2 we hand back a
//! single section labelled `"track"` covering the whole audio so the
//! tool's wire shape is honest without lying about the structure.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Section {
    pub name: String,
    pub start_s: f32,
    pub end_s: f32,
}

/// Phase-2 stub: return a single section spanning `[0, duration_s]`.
pub fn segment_sections(samples_mono: &[f32], sample_rate: u32) -> Vec<Section> {
    if samples_mono.is_empty() || sample_rate == 0 {
        return Vec::new();
    }
    let duration = samples_mono.len() as f32 / sample_rate as f32;
    vec![Section {
        name: "track".to_string(),
        start_s: 0.0,
        end_s: duration,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_section_covers_whole_audio() {
        let sr = 44_100;
        let samples = vec![0.0f32; sr as usize * 5];
        let sections = segment_sections(&samples, sr);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, "track");
        assert!((sections[0].end_s - 5.0).abs() < 1e-3);
    }

    #[test]
    fn empty_audio_no_sections() {
        let s = segment_sections(&[], 44_100);
        assert!(s.is_empty());
    }
}
