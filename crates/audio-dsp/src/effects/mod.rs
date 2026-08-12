//! Effect algorithms, moved out of `crates/tools` so the render path
//! can reach them. See the crate docs for why that was impossible.
//!
//! Each module holds the algorithm verbatim as it was in the tool that
//! owned it — same arithmetic, same order — so output is bit-identical
//! across the move. The tools now re-export from here, which is why no
//! call site changed.

pub mod de_esser;
pub mod distortion;
pub mod echo;
pub mod high_pass_filter;
pub mod leveler;
pub mod limiter;
pub mod low_pass_filter;
pub mod noise_gate;
pub mod notch_filter;
pub mod phaser;
pub mod reverb;
pub mod stereo_widener;
pub mod tremolo;
