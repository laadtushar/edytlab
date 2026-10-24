//! Prints what `analyze` makes of real files. Not a pass/fail test: the files
//! are whatever you point it at.
//!
//! ```text
//! EDYTLAB_ANALYZE_FILES="a.wav:120:A minor,b.wav" \
//!   cargo test -p audio-analysis --test real_material -- --ignored --nocapture
//! ```
//!
//! Each entry is `path[:expected_bpm[:expected_key]]`, comma separated. The
//! DJ demo tracks are written by `apps/desktop/e2e-native/fixtures.mjs`
//! (`writeFixtures(dir)`), and are not checked in as audio.

#[test]
#[ignore = "reads the files named in EDYTLAB_ANALYZE_FILES"]
fn analyse_named_files() {
    let Ok(list) = std::env::var("EDYTLAB_ANALYZE_FILES") else {
        println!("set EDYTLAB_ANALYZE_FILES to a comma-separated list of files");
        return;
    };
    for entry in list.split(',').filter(|e| !e.trim().is_empty()) {
        let mut parts = entry.trim().splitn(3, ':');
        let path = parts.next().unwrap_or_default();
        let want_bpm = parts.next().unwrap_or("?");
        let want_key = parts.next().unwrap_or("?");
        let audio = audio_decoder::decode_file(std::path::Path::new(path)).expect("decode");
        let r = audio_analysis::analyze(&audio).expect("analyze");
        // The same file under each profile set and bass weight, to choose
        // them by (the chroma is made once).
        if std::env::var("EDYTLAB_KEY_SWEEP").is_ok() {
            let mono: Vec<f32> = audio
                .samples
                .chunks(audio.channels as usize)
                .map(|f| f.iter().sum::<f32>() / f.len() as f32)
                .collect();
            let c = audio_analysis::key::chroma(&mono, audio.sample_rate).expect("chroma");
            for set in audio_analysis::key::ProfileSet::ALL {
                for bass in [0.0f32, 0.5, 1.0] {
                    let k =
                        audio_analysis::key::estimate_key_from_chroma(&c, set, bass).expect("key");
                    println!(
                        "KEYSWEEP {path}: {:<24} bass {bass}: {} {:.2}, then {} {:.2}",
                        set.name(),
                        k.key,
                        k.score,
                        k.runner_up,
                        k.runner_up_score
                    );
                }
            }
        }
        // Share of the beats within 35 ms of a multiple of the expected
        // period: right for the DJ fixtures, whose kicks start at 0 s.
        let on_grid = want_bpm.parse::<f32>().ok().map(|b| {
            let period = 60.0 / b;
            let near = r
                .beats
                .iter()
                .filter(|t| {
                    let k = (*t / period).round();
                    (*t - k * period).abs() <= 0.035
                })
                .count();
            100.0 * near as f32 / r.beats.len().max(1) as f32
        });
        println!(
            "REAL {path}: bpm {:?} conf {:.2} alts {:?} (expected {want_bpm}), key {} {:.2} then {} {:.2} (expected {want_key}), {} beats, {:?}% on the kick grid, first beats {:?}",
            r.bpm,
            r.bpm_confidence,
            r.bpm_alternatives.iter().map(|a| (a.relation.as_str(), a.bpm, a.confidence)).collect::<Vec<_>>(),
            r.key,
            r.key_score,
            r.key_runner_up,
            r.key_runner_up_score,
            r.beats.len(),
            on_grid.map(|v| v.round()),
            r.beats.iter().take(4).collect::<Vec<_>>()
        );
    }
}
