//! Crossfades remain editable source-frame envelopes, including off-rate clips.
use serde_json::json;
use session::EnvelopeFrames;
#[path = "support/harness.rs"]
mod harness;
use harness::Session;

fn args() -> serde_json::Value {
    json!({"from_track": 0, "from_clip": 0, "to_track": 1, "to_clip": 0})
}

fn pair() -> Session {
    let mut s = Session::with_tracks(&[
        (44_100, vec![0.25; 44_100 * 4]),
        (48_000, vec![0.25; 48_000 * 4]),
    ]);
    let mut state = s.state();
    // Trimmed sources: the envelope is relative to the clip, not the file.
    for (t, rate) in state.tracks.iter_mut().zip([44_100, 48_000]) {
        t.clips[0].source_offset = rate;
        t.clips[0].length = rate * 3;
    }
    state.tracks[1].clips[0].start_in_track = 48_000 * 2;
    s.append_state("overlap", state);
    s
}

#[test]
fn mixed_rates_share_session_seconds_and_remain_editable() {
    let mut s = pair();
    let before = s.state();
    let head = s.head();
    let count = s.node_count();
    let result = s.ok("crossfade", args());
    assert_eq!(result["start_sec"], 2.0);
    assert_eq!(result["end_sec"], 3.0);
    s.assert_one_node_appended_after(head, count);
    let state = s.state();
    for (i, rate) in [44_100, 48_000].into_iter().enumerate() {
        let clip = &state.tracks[i].clips[0];
        assert_eq!(clip.volume_envelope_frames, EnvelopeFrames::Source);
        let mid = clip
            .volume_envelope
            .iter()
            .find(|p| p.time_samples == if i == 0 { rate * 5 / 2 } else { rate / 2 })
            .expect("midpoint in the clip's own frames");
        assert!((mid.gain_db + 3.0103).abs() < 0.001);
        assert_eq!(clip.source_path, before.tracks[i].clips[0].source_path);
        assert_eq!(clip.source_offset, before.tracks[i].clips[0].source_offset);
        assert_eq!(clip.length, before.tracks[i].clips[0].length);
        // Render each lane alone: both must reach the same gain at 2.5 s.
        let mut solo = state.clone();
        solo.tracks = vec![solo.tracks[i].clone()];
        let out = s.dir.path().join(format!("solo-{i}.wav"));
        audio_engine::render_state_to_wav(&solo, &out, None).unwrap();
        let samples = harness::read_pcm(&out);
        let channels = hound::WavReader::open(&out).unwrap().spec().channels as usize;
        for t in [0.01, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99] {
            let sample = samples[((2.0 + t) * state.sample_rate as f64) as usize * channels];
            let angle = t * std::f64::consts::FRAC_PI_2;
            let expected = if i == 0 { angle.cos() } else { angle.sin() };
            assert!(
                (f64::from(sample) / 8191.0 - expected).abs() < 0.003,
                "track {i}, at {t} of the fade"
            );
        }
    }
    s.ok(
        "set_clip_envelope",
        json!({"track_index":1,"clip_index":0,
        "points":[{"time_sec":0.5,"gain_db":-9.0}]}),
    );
    assert_eq!(
        s.state().tracks[1].clips[0].volume_envelope[0].gain_db,
        -9.0
    );
    assert_eq!(
        s.store.get(head).unwrap().state,
        before,
        "history stays intact"
    );
}

#[test]
fn same_track_clips_support_equal_gain() {
    let mut s = pair();
    let mut state = s.state();
    let mut incoming = state.tracks[0].clips[0].clone();
    incoming.start_in_track = 44_100 * 2;
    state.tracks[0].clips.push(incoming);
    s.append_state("same-track overlap", state);
    s.ok(
        "crossfade",
        json!({"from_track":0,"from_clip":0,"to_track":0,"to_clip":1,"curve":"equal_gain"}),
    );
    for clip in &s.state().tracks[0].clips {
        let mid = &clip.volume_envelope[64];
        assert!((mid.gain_db + 6.0206).abs() < 0.001);
    }
}

#[test]
fn invalid_targets_and_nonoverlap_leave_history_untouched() {
    let mut s = pair();
    for bad in [
        json!({"from_track":0,"from_clip":0,"to_track":0,"to_clip":0}),
        json!({"from_track":0,"from_clip":5,"to_track":1,"to_clip":0}),
        json!({"from_track":8,"from_clip":0,"to_track":1,"to_clip":0}),
    ] {
        let head = s.head();
        let count = s.node_count();
        let state = s.state();
        s.err("crossfade", bad);
        s.assert_history_unchanged(head, count, &state);
    }
    let mut state = s.state();
    state.tracks[1].clips[0].start_in_track = 48_000 * 3;
    s.append_state("no overlap", state);
    let head = s.head();
    let count = s.node_count();
    let state = s.state();
    assert!(s.err("crossfade", args()).contains("overlap"));
    s.assert_history_unchanged(head, count, &state);
}

#[test]
fn a_one_frame_overlap_has_distinct_finite_endpoints() {
    let mut s = pair();
    let mut state = s.state();
    let mut incoming = state.tracks[0].clips[0].clone();
    incoming.start_in_track = 44_100 * 3 - 1;
    state.tracks[0].clips.push(incoming);
    s.append_state("one-frame overlap", state);
    s.ok(
        "crossfade",
        json!({"from_track":0,"from_clip":0,"to_track":0,"to_clip":1}),
    );
    for clip in &s.state().tracks[0].clips {
        let points = &clip.volume_envelope;
        assert_eq!(points.len(), 2);
        assert_eq!(points[1].time_samples - points[0].time_samples, 1);
        assert!(points.iter().all(|p| p.gain_db.is_finite()));
    }
}

#[test]
fn a_track_with_inconsistent_source_rates_is_refused_atomically() {
    let mut s = pair();
    let mut state = s.state();
    let off_rate = state.tracks[1].clips[0].clone();
    state.tracks[0].clips.push(off_rate);
    s.append_state("inconsistent rates", state);
    let head = s.head();
    let count = s.node_count();
    let state = s.state();
    assert!(s
        .err("crossfade", args())
        .contains("different sample rates"));
    s.assert_history_unchanged(head, count, &state);
}
