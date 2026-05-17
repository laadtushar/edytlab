//! Individual tools that the M07 dispatcher exposes to the model.
//!
//! Each submodule is a standalone tool: a unit struct implementing
//! [`crate::Tool`] alongside a private `Args` struct. The
//! [`crate::dispatcher::default_dispatcher`] constructor wires them up
//! together.

pub mod add_track;
pub mod align_to_beat;
pub mod analyze_track;
pub mod apply_diff;
pub mod compare_nodes;
pub mod copy_region;
pub mod cut_range;
pub mod eq;
pub mod fade;
pub mod fork_node;
pub mod gain;
pub mod insert_silence;
pub mod label;
pub mod load;
pub mod name_node;
pub mod normalize;
pub mod paste_region;
pub mod pitch_shift;
pub mod remove_track;
pub mod render_final;
pub mod render_preview;
pub mod reverse;
pub mod revert_to;
pub mod separate_stems;
pub mod set_track_gain;
pub mod time_stretch;
pub mod transcribe;
pub mod trim;
mod util;

pub use add_track::AddTrackTool;
pub use align_to_beat::AlignToBeatTool;
pub use analyze_track::AnalyzeTrackTool;
pub use apply_diff::ApplyDiffTool;
pub use compare_nodes::CompareNodesTool;
pub use copy_region::CopyRegionTool;
pub use cut_range::CutRangeTool;
pub use eq::EqTool;
pub use fade::FadeTool;
pub use fork_node::ForkNodeTool;
pub use gain::GainTool;
pub use insert_silence::InsertSilenceTool;
pub use label::LabelTool;
pub use load::LoadTool;
pub use name_node::NameNodeTool;
pub use normalize::NormalizeTool;
pub use paste_region::PasteRegionTool;
pub use pitch_shift::PitchShiftTool;
pub use remove_track::RemoveTrackTool;
pub use render_final::RenderFinalTool;
pub use render_preview::RenderPreviewTool;
pub use reverse::ReverseTool;
pub use revert_to::RevertToTool;
pub use separate_stems::SeparateStemsTool;
pub use set_track_gain::SetTrackGainTool;
pub use time_stretch::TimeStretchTool;
pub use transcribe::TranscribeTool;
pub use trim::TrimTool;
