//! Speaker diarization (#168 section 2).
pub mod cluster;
pub mod fbank;
pub mod frames;
pub mod model;
pub mod pipeline;

pub use cluster::SpeakerBounds;
pub use model::{DiarizeModel, LoadError, EMBEDDING_FILE, SEGMENTATION_FILE};
pub use pipeline::{
    Diarization, DiarizeError, Options, SpeakerTurn, Stage, EMBEDDING_DIM, SAMPLE_RATE,
};
