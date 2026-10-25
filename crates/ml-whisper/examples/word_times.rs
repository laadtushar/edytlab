//! Transcribe a WAV file with the real model and print the words with times.
//!
//! The measuring half of the word-timing comparison (#659): this prints, and
//! `scripts/word-timing-eval/score.py` scores against a reference.
//!
//! ```text
//! export ORT_DYLIB_PATH=/path/to/libonnxruntime.so
//! export WHISPER_MODEL_PATH=/path/to/whisper-base
//! cargo run --release -p ml-whisper --example word_times -- audio.wav [language] > hyp.tsv
//! ```
//!
//! Standard output is `start_s<TAB>end_s<TAB>word` for each word, preceded by
//! one `# windows N` line and one `# seconds S` line; the same two lines are
//! what the scorer reads for the window count and the run time.

use std::path::PathBuf;
use std::time::Instant;

use ml_whisper::{transcribe, WhisperModel};

fn main() {
    let mut args = std::env::args().skip(1);
    let wav = PathBuf::from(
        args.next()
            .expect("usage: word_times <audio.wav> [language]"),
    );
    let language = args.next();
    let model_dir = PathBuf::from(
        std::env::var_os("WHISPER_MODEL_PATH").expect("WHISPER_MODEL_PATH names the model folder"),
    );

    let mut reader = hound::WavReader::open(&wav).expect("a WAV file");
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.expect("a sample") as f32 / scale)
                .collect()
        }
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.expect("a sample"))
            .collect(),
    };

    let model = WhisperModel::load(&model_dir).expect("the model loads");
    let started = Instant::now();
    let transcript = transcribe(
        &model,
        &samples,
        spec.sample_rate,
        spec.channels,
        language.as_deref(),
        || false,
    )
    .expect("transcription");
    println!("# windows {}", transcript.windows);
    println!("# seconds {:.1}", started.elapsed().as_secs_f32());
    for w in &transcript.words {
        println!("{:.3}\t{:.3}\t{}", w.start_s, w.end_s, w.text);
    }
}
