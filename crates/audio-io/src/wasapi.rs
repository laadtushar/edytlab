//! Windows WASAPI specifics.
//!
//! `cpal` opens WASAPI in **shared mode** by default, which is what Phase 1
//! targets (see `lib.rs` doc comment for the latency-floor rationale).
//! Exclusive mode and `IAudioClient3` low-latency paths are deliberately out
//! of scope here — this module is a placeholder for future tweaks.
