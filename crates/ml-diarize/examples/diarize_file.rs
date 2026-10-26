//! Diarize a 16 kHz mono WAV file and print the turns as RTTM, with the time each
//! stage took on stderr. For measuring (the numbers in the pull request were made
//! with it) and for trying the model on a recording of your own:
//!
//! ```text
//! export ORT_DYLIB_PATH=/path/to/libonnxruntime.so
//! cargo run --release -p ml-diarize --example diarize_file -- \
//!     /path/to/speaker-diarization meeting.wav [num_speakers] > meeting.rttm
//! ```
//!
//! The first argument is the model folder (`segmentation/model.onnx`,
//! `embedding/model.onnx`), the second a 16 kHz mono WAV, the optional third
//! how many people speak. The RTTM can be scored with `pyannote.metrics`.

use std::cell::Cell;
use std::time::Instant;

use ml_diarize::{DiarizeModel, Options, SpeakerBounds, Stage};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: diarize_file <model folder> <16 kHz mono wav> [num_speakers]");
        std::process::exit(2);
    }
    let reader = hound::WavReader::open(&args[2]).expect("a WAV file");
    let spec = reader.spec();
    assert!(
        spec.sample_rate == 16_000 && spec.channels == 1,
        "this example reads 16 kHz mono; {spec:?}"
    );
    let audio: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .into_samples::<i16>()
            .map(|s| f32::from(s.unwrap()) / 32768.0)
            .collect(),
        hound::SampleFormat::Float => reader.into_samples::<f32>().map(|s| s.unwrap()).collect(),
    };
    let num_speakers = args.get(3).map(|n| n.parse::<usize>().expect("a number"));

    let started = Instant::now();
    let model = DiarizeModel::load(std::path::Path::new(&args[1])).expect("the model loads");
    eprintln!("load: {:.2} s", started.elapsed().as_secs_f64());

    let stage_started = Cell::new(Instant::now());
    let last = Cell::new(Stage::Segmentation);
    let progress = |stage: Stage, done: usize, total: usize| {
        if stage != last.get() {
            eprintln!(
                "{}: {:.2} s",
                last.get(),
                stage_started.get().elapsed().as_secs_f64()
            );
            stage_started.set(Instant::now());
            last.set(stage);
        }
        if done == total {
            eprintln!("{stage}: {total} done");
        }
    };
    let started = Instant::now();
    let result = model
        .diarize(
            &audio,
            &Options {
                bounds: SpeakerBounds::new(num_speakers, None, None).unwrap(),
                on_progress: &progress,
                ..Options::default()
            },
        )
        .expect("diarization");
    let took = started.elapsed().as_secs_f64();
    eprintln!(
        "total: {took:.1} s for {:.1} s of audio (real-time factor {:.3}); {} speakers, {} turns",
        result.duration_s,
        took / result.duration_s,
        result.speakers,
        result.turns.len()
    );
    for t in &result.turns {
        println!(
            "SPEAKER {} 1 {:.3} {:.3} <NA> <NA> SPEAKER_{:02} <NA> <NA>",
            args[2]
                .rsplit('/')
                .next()
                .unwrap_or("audio")
                .trim_end_matches(".wav"),
            t.start_s,
            t.end_s - t.start_s,
            t.speaker
        );
    }
}
