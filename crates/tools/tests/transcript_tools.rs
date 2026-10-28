//! Transcript exports and markers through the real dispatcher and recognizer seam.
//!
//! One integration binary shares the existing speech fixture instead of linking
//! another executable for each format or tool.

#[path = "support/transcript_export_cases.rs"]
mod export_cases;
#[path = "support/transcript_marker_cases.rs"]
mod marker_cases;
#[path = "support/transcript_safety_cases.rs"]
mod safety_cases;
#[path = "support/speech.rs"]
mod speech;

use std::sync::Arc;

use serde_json::json;
use session::TranscriptWord;
use speech::{Fake, Project};

use speech::word;

fn track_project(words: Vec<TranscriptWord>) -> Project {
    project_with_transcript(words, false)
}

fn file_project(words: Vec<TranscriptWord>) -> Project {
    project_with_transcript(words, true)
}

fn project_with_transcript(words: Vec<TranscriptWord>, from_file: bool) -> Project {
    let mut project = Project::new(Arc::new(Fake::new(words)));
    let source = project.load("speech.wav", &speech::tone(12.0, 220.0));
    if from_file {
        project.ok("transcribe", json!({ "path": source }));
    } else {
        project.ok("transcribe", json!({ "track": 0 }));
    }
    project
}
