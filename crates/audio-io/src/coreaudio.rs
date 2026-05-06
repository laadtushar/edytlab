//! macOS Core Audio specifics.
//!
//! `cpal` already targets Core Audio's default `AudioUnit` HAL output on macOS,
//! which is what we want for non-realtime preview playback. This module exists
//! as a placeholder for future Core Audio-specific tweaks (e.g. AudioWorkgroup
//! join for tighter scheduling, kAudioUnitProperty_ScheduledFileIDs for offline
//! render). None are needed for Phase 1.
