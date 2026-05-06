//! Individual tools that the M07 dispatcher exposes to the model.
//!
//! Each submodule is a standalone tool: a unit struct implementing
//! [`crate::Tool`] alongside a private `Args` struct. The
//! [`crate::dispatcher::default_dispatcher`] constructor wires them up
//! together.

pub mod cut_range;
pub mod gain;
pub mod load;
pub mod normalize;
pub mod render_final;
pub mod render_preview;
pub mod trim;
mod util;

pub use cut_range::CutRangeTool;
pub use gain::GainTool;
pub use load::LoadTool;
pub use normalize::NormalizeTool;
pub use render_final::RenderFinalTool;
pub use render_preview::RenderPreviewTool;
pub use trim::TrimTool;
