//! A stored effect the renderer cannot run never fails a render (#520).
//!
//! `add_effect` and `set_effect_bypassed` refuse such a kind now, but a
//! project saved by an older build can already hold one, on a track, on a
//! bus or on the master bus, written straight into the session. Until
//! this, every render of that session failed, preview, playback and
//! export included, until someone found the entry and removed it.
//!
//! The rule these tests pin: an un-bypassed entry the renderer cannot run
//! (a kind that is not streamable yet, or one that does not exist) is
//! left out, the rest of the chain renders, and the report says which
//! entry was skipped. "Left out" means the output is the output of the
//! same session with that entry removed, byte for byte, and the warning
//! names where it was (track index and name, bus, or master), which entry
//! (the position in the stored chain, the one `remove_effect` takes) and
//! what kind.

use std::path::{Path, PathBuf};

use audio_engine::effect_chain::{skipped_effects, ChainScope, KindSupport};
use audio_engine::{render_state_to_wav, render_warnings, RenderReport, TimeRange};
use hound::{SampleFormat, WavSpec, WavWriter};
use session::{Bus, BusGraph, Clip, EffectInstance, Send, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;
use uuid::Uuid;

const SAMPLE_RATE: u32 = 44_100;
/// Two seconds, so the render crosses a chunk boundary (the master chunk
/// is one second) with the skipped entry in the chain.
const FRAMES: usize = SAMPLE_RATE as usize * 2;

/// Effects that exist as destructive tools but cannot run in a chain yet.
const NOT_YET_STREAMABLE: [&str; 11] = [
    "tremolo",
    "phaser",
    "echo",
    "reverb",
    "noise_gate",
    "leveler",
    "de_esser",
    "distortion",
    "stereo_widener",
    "eq",
    "compressor",
];

/// Every kind a stored entry can have that the renderer cannot run: the
/// eleven above, and one that exists nowhere.
fn unrunnable_kinds() -> Vec<&'static str> {
    let mut kinds = NOT_YET_STREAMABLE.to_vec();
    kinds.push("wobbulator");
    kinds
}

fn write_tone(dir: &Path) -> PathBuf {
    let path = dir.join("tone.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..FRAMES {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.4;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn effect(kind: &str, params: serde_json::Value) -> EffectInstance {
    EffectInstance {
        kind: kind.to_string(),
        params,
        bypassed: false,
    }
}

fn gain(db: f32) -> EffectInstance {
    effect("gain", serde_json::json!({ "db": db }))
}

fn low_pass(hz: f32) -> EffectInstance {
    effect("low_pass_filter", serde_json::json!({ "cutoff_hz": hz }))
}

/// The entry an old project holds: of `kind`, switched on.
fn stored(kind: &str) -> EffectInstance {
    effect(kind, serde_json::json!({}))
}

/// Where the chains under test live. Each field is a whole chain.
#[derive(Clone, Default)]
struct Chains {
    track: Vec<EffectInstance>,
    master: Vec<EffectInstance>,
    bus: Vec<EffectInstance>,
}

fn state(source: &Path, chains: Chains) -> SessionState {
    let decoded = audio_decoder::decode_file(source).expect("decode");
    let frames = (decoded.samples.len() / decoded.channels as usize) as u64;
    let bus = Bus {
        id: Uuid::from_u128(7),
        name: "Room".into(),
        effects: chains.bus,
    };
    SessionState {
        tracks: vec![Track {
            id: TrackId::new(),
            name: "Vocals".into(),
            clips: vec![Clip {
                source_path: source.to_path_buf(),
                start_in_track: 0,
                source_offset: 0,
                length: frames,
                content_hash: None,
                time_stretch_factor: None,
                pitch_shift_semitones: None,
                beat_grid: None,
                volume_envelope: Vec::new(),
            }],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: chains.track,
            sends: vec![Send {
                bus_id: bus.id,
                level_db: -6.0,
            }],
        }],
        bus_routing: BusGraph { buses: vec![bus] },
        master_chain: chains.master,
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: decoded.sample_rate,
        length_samples: frames,
        annotations: Vec::new(),
        sync_lock: false,
    }
}

/// Like [`state`] but with no send, so the session can be the plain
/// single-track case the byte-copy shortcut serves.
fn state_without_send(source: &Path, chains: Chains) -> SessionState {
    let mut st = state(source, chains);
    st.tracks[0].sends.clear();
    st.bus_routing.buses.clear();
    st
}

struct Rendered {
    bytes: Vec<u8>,
    report: RenderReport,
}

fn render(st: &SessionState, range: Option<TimeRange>) -> Rendered {
    let tmp = TempDir::new().expect("tempdir");
    let out = tmp.path().join("out.wav");
    let report = render_state_to_wav(st, &out, range)
        .unwrap_or_else(|e| panic!("a stored effect must not fail the render: {e}"));
    Rendered {
        bytes: std::fs::read(&out).expect("read render"),
        report,
    }
}

/// `chain` with the entry at `at` taken out: the session the skipped
/// render has to be indistinguishable from.
fn without(chain: &[EffectInstance], at: usize) -> Vec<EffectInstance> {
    let mut c = chain.to_vec();
    c.remove(at);
    c
}

/// A chain with real, audible effects either side of the entry under
/// test, so a skip that also swallowed its neighbours, or shifted them,
/// would show in the bytes. The entry sits at index 1.
fn around(entry: EffectInstance) -> Vec<EffectInstance> {
    vec![gain(-4.0), entry, low_pass(900.0)]
}

fn assert_one_warning(report: &RenderReport, scope_text: &[&str], index: usize, kind: &str) {
    assert_eq!(
        report.warnings.len(),
        1,
        "one skipped entry, one warning: {:?}",
        report.warnings
    );
    let w = &report.warnings[0];
    for part in scope_text {
        assert!(
            w.contains(part),
            "{kind}: the warning should name `{part}`: {w}"
        );
    }
    assert!(
        w.contains(&format!("effect {index}")),
        "{kind}: the warning should give the chain index {index}: {w}"
    );
    assert!(
        w.contains(kind),
        "{kind}: the warning should name the kind: {w}"
    );
}

// --- the ticket: a stored kind never fails the render ----------------------

/// **Acceptance item 2.** Each kind, written straight into a track's chain
/// the way an old project holds it: the render succeeds, says so in one
/// warning that names the track, the chain index and the kind, and is
/// byte-identical to the render with that entry removed.
#[test]
fn each_unrunnable_kind_in_a_track_chain_is_skipped_with_a_warning() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path());

    for kind in unrunnable_kinds() {
        let chain = around(stored(kind));
        let got = render(
            &state(
                &src,
                Chains {
                    track: chain.clone(),
                    ..Chains::default()
                },
            ),
            None,
        );
        let removed = render(
            &state(
                &src,
                Chains {
                    track: without(&chain, 1),
                    ..Chains::default()
                },
            ),
            None,
        );

        assert_one_warning(&got.report, &["track 0", "Vocals"], 1, kind);
        assert_eq!(
            got.bytes, removed.bytes,
            "{kind}: skipping the entry must render exactly as if it were not there"
        );
        assert!(
            removed.report.warnings.is_empty(),
            "{kind}: the session without the entry has nothing to warn about"
        );
        assert_eq!(got.report.frames_written, removed.report.frames_written);
        assert_eq!(got.report.peak_dbfs, removed.report.peak_dbfs);
    }
}

/// **Acceptance item 2, on the master bus.** The same, written into
/// `master_chain`, where one such entry used to fail the whole mix.
#[test]
fn each_unrunnable_kind_in_the_master_chain_is_skipped_with_a_warning() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path());

    for kind in unrunnable_kinds() {
        let chain = around(stored(kind));
        let got = render(
            &state(
                &src,
                Chains {
                    master: chain.clone(),
                    ..Chains::default()
                },
            ),
            None,
        );
        let removed = render(
            &state(
                &src,
                Chains {
                    master: without(&chain, 1),
                    ..Chains::default()
                },
            ),
            None,
        );

        assert_one_warning(&got.report, &["master"], 1, kind);
        assert_eq!(
            got.bytes, removed.bytes,
            "{kind}: skipping the entry must render exactly as if it were not there"
        );
        assert!(removed.report.warnings.is_empty(), "{kind}");
    }
}

/// A bus has no effect tool yet, but a stored bus chain goes through the
/// same builder, and "rendering never fails because of a stored effect"
/// has no exception for it.
#[test]
fn each_unrunnable_kind_in_a_bus_chain_is_skipped_with_a_warning() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path());

    for kind in unrunnable_kinds() {
        let chain = around(stored(kind));
        let got = render(
            &state(
                &src,
                Chains {
                    bus: chain.clone(),
                    ..Chains::default()
                },
            ),
            None,
        );
        let removed = render(
            &state(
                &src,
                Chains {
                    bus: without(&chain, 1),
                    ..Chains::default()
                },
            ),
            None,
        );

        assert_one_warning(&got.report, &["bus", "Room"], 1, kind);
        assert_eq!(got.bytes, removed.bytes, "{kind}");
        assert!(removed.report.warnings.is_empty(), "{kind}");
    }
}

/// The simplest session there is: one track, no sends, and nothing in it
/// but the stored entry. With an empty chain the render is a byte copy of
/// the source; with the entry it takes the streaming path, as a bypassed
/// entry does today, and writes the same samples. The output must be the
/// plain session's, not an error and not a different file.
#[test]
fn a_lone_unrunnable_entry_renders_as_the_plain_session_does() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path());
    let plain = render(&state_without_send(&src, Chains::default()), None);

    for kind in unrunnable_kinds() {
        let on_track = render(
            &state_without_send(
                &src,
                Chains {
                    track: vec![stored(kind)],
                    ..Chains::default()
                },
            ),
            None,
        );
        assert_one_warning(&on_track.report, &["track 0", "Vocals"], 0, kind);
        assert_eq!(on_track.bytes, plain.bytes, "{kind}: on the track");

        let on_master = render(
            &state_without_send(
                &src,
                Chains {
                    master: vec![stored(kind)],
                    ..Chains::default()
                },
            ),
            None,
        );
        assert_one_warning(&on_master.report, &["master"], 0, kind);
        assert_eq!(on_master.bytes, plain.bytes, "{kind}: on the master");
    }
}

// --- what is and is not warned about --------------------------------------

/// Every skipped entry gets its own warning, in the order the signal
/// meets them: tracks, then buses, then the master. Streamable neighbours
/// are never mentioned, and the indices are positions in the stored
/// chain, bypassed entries counted.
#[test]
fn every_skipped_entry_gets_its_own_warning_with_its_own_index() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path());

    let mut off = stored("echo");
    off.bypassed = true;
    let st = state(
        &src,
        Chains {
            track: vec![stored("reverb"), gain(-3.0), off, stored("eq")],
            bus: vec![stored("phaser")],
            master: vec![gain(-1.0), stored("wobbulator"), stored("compressor")],
        },
    );
    let r = render(&st, None);

    assert_eq!(r.report.warnings.len(), 5, "{:?}", r.report.warnings);
    let w = &r.report.warnings;
    for (i, (scope, index, kind)) in [
        ("track 0", 0, "reverb"),
        ("track 0", 3, "eq"),
        ("bus", 0, "phaser"),
        ("master", 1, "wobbulator"),
        ("master", 2, "compressor"),
    ]
    .iter()
    .enumerate()
    {
        assert!(
            w[i].contains(scope)
                && w[i].contains(&format!("effect {index}"))
                && w[i].contains(kind),
            "warning {i} should be {scope} effect {index} ({kind}): {}",
            w[i]
        );
    }
    assert!(
        w.iter().all(|m| !m.contains("echo")),
        "a bypassed entry is not skipped by the renderer, so it is not warned about: {w:?}"
    );
    assert!(
        w.iter().all(|m| !m.contains("gain")),
        "an entry that renders is not warned about: {w:?}"
    );

    // And the audio is the audio of the chains with the five taken out.
    let clean = render(
        &state(
            &src,
            Chains {
                track: vec![gain(-3.0)],
                bus: vec![],
                master: vec![gain(-1.0)],
            },
        ),
        None,
    );
    assert_eq!(r.bytes, clean.bytes);
}

/// A bypassed entry of an unrunnable kind is fine: the renderer never
/// looked at it, so there is nothing to warn about.
#[test]
fn a_bypassed_unrunnable_entry_is_silent() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path());

    let mut off = stored("reverb");
    off.bypassed = true;
    let r = render(
        &state(
            &src,
            Chains {
                track: vec![off.clone()],
                master: vec![off],
                ..Chains::default()
            },
        ),
        None,
    );
    assert!(r.report.warnings.is_empty(), "{:?}", r.report.warnings);
}

/// A session with nothing wrong reports no warnings. The field exists on
/// every render, and an empty one has to mean "nothing skipped".
#[test]
fn a_clean_session_reports_no_warnings() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path());
    let r = render(
        &state(
            &src,
            Chains {
                track: vec![gain(-3.0), low_pass(2_000.0)],
                master: vec![effect("limiter", serde_json::json!({ "ceiling_db": -1.0 }))],
                ..Chains::default()
            },
        ),
        None,
    );
    assert!(r.report.warnings.is_empty(), "{:?}", r.report.warnings);
}

/// A range render skips and warns the same way: the chains are built the
/// same, whatever part of the mix is written.
#[test]
fn a_range_render_skips_and_warns_too() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path());
    let range = Some(TimeRange {
        start_frame: SAMPLE_RATE as u64 / 2,
        end_frame: SAMPLE_RATE as u64 * 3 / 2,
    });

    let chain = around(stored("reverb"));
    let got = render(
        &state(
            &src,
            Chains {
                master: chain.clone(),
                ..Chains::default()
            },
        ),
        range,
    );
    let removed = render(
        &state(
            &src,
            Chains {
                master: without(&chain, 1),
                ..Chains::default()
            },
        ),
        range,
    );
    assert_one_warning(&got.report, &["master"], 1, "reverb");
    assert_eq!(got.bytes, removed.bytes);
}

/// The warning is about the stored chain, not about whether this mix
/// happens to include the track, so a muted track's entry is reported
/// too. What matters is that it does not fail the render, which was true
/// before this change as well (#312) and stays true.
#[test]
fn a_muted_tracks_unrunnable_entry_does_not_fail_the_render() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path());

    let mut st = state(&src, Chains::default());
    let mut muted = st.tracks[0].clone();
    muted.id = TrackId::new();
    muted.name = "Pad".into();
    muted.muted = true;
    muted.sends.clear();
    muted.effects = vec![stored("reverb")];
    st.tracks.push(muted);

    let r = render(&st, None);
    assert_one_warning(&r.report, &["track 1", "Pad"], 0, "reverb");

    // The muted track adds nothing to the audio either way.
    let mut clean = st.clone();
    clean.tracks[1].effects.clear();
    assert_eq!(r.bytes, render(&clean, None).bytes);
}

// --- asking without rendering ----------------------------------------------

/// A render that is served from a cache has no report to read, so the
/// same warnings can be had from the state alone, without rendering. They
/// must be the render's own, word for word, or a cache hit would tell a
/// different story from the miss before it.
#[test]
fn the_warnings_can_be_had_from_the_state_and_match_the_render() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path());

    let st = state(
        &src,
        Chains {
            track: around(stored("reverb")),
            bus: vec![stored("tremolo")],
            master: around(stored("wobbulator")),
        },
    );
    let rendered = render(&st, None).report.warnings;
    assert_eq!(rendered.len(), 3);
    assert_eq!(render_warnings(&st), rendered);

    let clean = state(&src, Chains::default());
    assert!(render_warnings(&clean).is_empty());
}

/// The structured form behind the strings, which the tools that render a
/// selection of tracks use to say only what is true of that selection.
#[test]
fn skipped_effects_say_where_what_and_why() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path());
    let st = state(
        &src,
        Chains {
            track: vec![stored("reverb")],
            bus: vec![stored("wobbulator")],
            master: vec![gain(0.0), stored("eq")],
        },
    );

    let skipped = skipped_effects(&st);
    assert_eq!(skipped.len(), 3, "{skipped:?}");

    assert_eq!(
        skipped[0].scope,
        ChainScope::Track {
            index: 0,
            name: "Vocals".into()
        }
    );
    assert_eq!((skipped[0].index, skipped[0].kind.as_str()), (0, "reverb"));
    assert_eq!(skipped[0].support, KindSupport::NotYetStreamable);

    assert_eq!(
        skipped[1].scope,
        ChainScope::Bus {
            name: "Room".into()
        }
    );
    assert_eq!(skipped[1].support, KindSupport::Unknown);

    assert_eq!(skipped[2].scope, ChainScope::Master);
    assert_eq!((skipped[2].index, skipped[2].kind.as_str()), (1, "eq"));
}
