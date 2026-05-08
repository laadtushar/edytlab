//! Individual tools that the M07 dispatcher exposes to the model.
//!
//! Each submodule is a standalone tool: a unit struct implementing
//! [`crate::Tool`] alongside a private `Args` struct. The
//! [`crate::dispatcher::default_dispatcher`] constructor wires them up
//! together.

pub mod add_track;
pub mod align_to_beat;
pub mod analyze_track;
pub mod cut_range;
pub mod gain;
pub mod load;
pub mod normalize;
pub mod pitch_shift;
pub mod remove_track;
pub mod render_final;
pub mod render_preview;
pub mod separate_stems;
pub mod set_track_gain;
pub mod time_stretch;
pub mod transcribe;
pub mod trim;
mod util;

pub use add_track::AddTrackTool;
pub use align_to_beat::AlignToBeatTool;
pub use analyze_track::AnalyzeTrackTool;
pub use cut_range::CutRangeTool;
pub use gain::GainTool;
pub use load::LoadTool;
pub use normalize::NormalizeTool;
pub use pitch_shift::PitchShiftTool;
pub use remove_track::RemoveTrackTool;
pub use render_final::RenderFinalTool;
pub use render_preview::RenderPreviewTool;
pub use separate_stems::SeparateStemsTool;
pub use set_track_gain::SetTrackGainTool;
pub use time_stretch::TimeStretchTool;
pub use transcribe::TranscribeTool;
pub use trim::TrimTool;
