//! Microphone capture → WAV file writer.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RecorderError {
    #[error("no input device available")]
    NoDevice,
    #[error("stream error: {0}")]
    Stream(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("wav write error: {0}")]
    Wav(#[from] hound::Error),
}

pub struct Recorder {
    samples: Arc<Mutex<Vec<f32>>>,
    stream: Option<cpal::Stream>,
    sample_rate: u32,
    channels: u16,
}

// cpal::Stream is not Send by default on some platforms; we hold it
// inside a Mutex<Option<Recorder>> so only one thread touches it.
// SAFETY: we never share the stream across threads without a Mutex guard.
unsafe impl Send for Recorder {}

impl Recorder {
    pub fn start() -> Result<Self, RecorderError> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or(RecorderError::NoDevice)?;
        let config = device
            .default_input_config()
            .map_err(|e| RecorderError::Stream(e.to_string()))?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let samples_clone = Arc::clone(&samples);
        let stream = device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut s) = samples_clone.lock() {
                        s.extend_from_slice(data);
                    }
                },
                |e| tracing::error!("stream error: {e}"),
                None,
            )
            .map_err(|e| RecorderError::Stream(e.to_string()))?;
        stream
            .play()
            .map_err(|e| RecorderError::Stream(e.to_string()))?;
        Ok(Self {
            samples,
            stream: Some(stream),
            sample_rate,
            channels,
        })
    }

    pub fn stop_and_save(mut self, path: &PathBuf) -> Result<(PathBuf, u32, u16), RecorderError> {
        drop(self.stream.take());
        let samples = self.samples.lock().unwrap().clone();
        let spec = hound::WavSpec {
            channels: self.channels,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec)?;
        for s in &samples {
            writer.write_sample(*s)?;
        }
        writer.finalize()?;
        Ok((path.clone(), self.sample_rate, self.channels))
    }

    pub fn duration_sec(&self) -> f64 {
        let n = self.samples.lock().unwrap().len();
        n as f64 / (self.sample_rate as f64 * self.channels as f64)
    }
}

pub fn format_seconds(secs: f64) -> String {
    let m = (secs / 60.0) as u64;
    let s = secs as u64 % 60;
    format!("{m}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::format_seconds;

    #[test]
    fn formats_duration() {
        assert_eq!(format_seconds(65.3), "1:05");
        assert_eq!(format_seconds(3.0), "0:03");
    }
}
