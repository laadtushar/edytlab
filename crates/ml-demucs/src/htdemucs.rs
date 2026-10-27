// Derived from Demucs, https://github.com/facebookresearch/demucs (demucs/htdemucs.py, `HTDemucs.forward`),
// commit e976d93ecc3865e5757426930257e200846a520a. MIT License,
// Copyright (c) Meta Platforms, Inc. and affiliates.
// Changed by edytlab: re-implemented in Rust, without PyTorch; the parts of
// `forward` outside the ONNX graph are done here, as this module's
// documentation says.
// The upstream licence text is in
// apps/desktop/src-tauri/licenses/THIRD_PARTY_NOTICES.txt (CREDITS, section F).

//! The htdemucs network on ONNX Runtime: one 7.8 s segment of stereo audio
//! in, four stereo sources out ([`SegmentModel`]).
//!
//! ## What the pinned export is, and what is left outside it
//!
//! `models.json` pins Intel's export of htdemucs v4
//! (`Intel/demucs-openvino`, `htdemucs_v4/htdemucs_fwd.onnx` and its
//! `htdemucs_fwd.onnx.data` of weights). Its graph, read from the file on
//! 2026-10-27, has two inputs and two outputs, all `float32` with fixed
//! shapes (batch 1, opset 20):
//!
//! | | name | shape |
//! |---|---|---|
//! | in | `x` | `[1, 4, 2048, 336]`: the spectrogram, a re and an im plane per channel |
//! | in | `xt` | `[1, 2, 343980]`: the waveform |
//! | out | `x_out` | `[1, 16, 2048, 336]`: 4 sources x 4 planes |
//! | out | `xt_out` | `[1, 8, 343980]`: 4 sources x 2 channels |
//!
//! It is `HTDemucs.forward` with the parts that PyTorch does around the
//! network cut away, and **those parts are this module's job**. There are
//! three, and the first is the one that is easy to miss, because a graph that
//! is fed an un-normalised spectrogram runs and returns numbers:
//!
//! 1. **Normalise both inputs, per segment.** `forward` standardises the
//!    spectrogram over all of its elements (`x.mean(dim=(1, 2, 3))`,
//!    `x.std(dim=(1, 2, 3))`, one pair of scalars for the whole segment) and
//!    the waveform over all of its (`xt.mean(dim=(1, 2))`, `xt.std(...)`),
//!    each as `(v - mean) / (1e-5 + std)`. The deviation is torch's, with
//!    Bessel's correction. This is on top of the track-level normalisation
//!    [`separate_streaming`](crate::separate::separate_streaming) does.
//! 2. **Undo it on the way out**: `x_out * std + mean` and
//!    `xt_out * stdt + meant`, with the plain deviation (no `1e-5`).
//! 3. **The STFT and the iSTFT** ([`crate::spec`]), and the sum of the two
//!    branches: a source is `xt_out + istft(x_out)`.
//!
//! The reference for all three is `htdemucs.py` at the commit above, and
//! Intel's own pipeline for this export, which does exactly these steps
//! around the same graph:
//! <https://github.com/intel/openvino-plugins-ai-audacity/blob/main/mod-openvino/htdemucs.cpp>
//! (`HTDemucs::_actually_run_model`). The source order is `drums, bass,
//! other, vocals` ([`MODEL_SOURCES`]).
//!
//! ## Layers
//!
//! [`Network`] does the three steps above around a [`Backend`], which runs
//! the graph on `x` and `xt` and hands back `x_out` and `xt_out`.
//! [`OrtBackend`] is the real one, on a session from
//! `ml_pipeline::onnx_session::build` (never `Session::builder`: the crate
//! `ml-pipeline` checks that). Tests give it a Rust function in place of the
//! graph, so everything but ONNX Runtime is checked in CI.

use std::path::{Path, PathBuf};

use ml_pipeline::onnx_session::{self, SessionOptions};
use ort::session::Session;
use ort::value::{Outlet, Tensor, TensorElementType};

use crate::separate::{SegmentModel, Stereo, MODEL_SOURCES, SEGMENT_SAMPLES};
use crate::spec::{frames_for, Stft, FREQS};
use crate::{DemucsError, Result};

/// The graph's file in the model folder.
pub const MODEL_FILE: &str = "htdemucs_fwd.onnx";
/// The weights, which the graph refers to by this name: the two must sit in
/// one folder under these names.
pub const WEIGHTS_FILE: &str = "htdemucs_fwd.onnx.data";

/// Sources the network emits.
const SOURCES: usize = MODEL_SOURCES.len();
/// Audio channels in, per source out.
const CHANNELS: usize = 2;
/// Planes of the spectrogram per audio channel: real and imaginary.
const PLANES: usize = 2;

/// Frames of the spectrogram of one segment: the `336` of `[1, 4, 2048, 336]`.
fn frames() -> usize {
    frames_for(SEGMENT_SAMPLES)
}

// ---------------------------------------------------------------------
// The normalisation `forward` does around the network
// ---------------------------------------------------------------------

/// The two scalars one normalisation took off, to be put back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Scale {
    mean: f32,
    /// The deviation itself: `1e-5` is added only when dividing.
    std: f32,
}

/// `(v - mean) / (1e-5 + std)` over all of `values`, with torch's mean and
/// deviation (the deviation divides by `n - 1`). Sums in `f64`.
pub(crate) fn normalise(values: &mut [f32]) -> Scale {
    let n = values.len();
    if n == 0 {
        return Scale {
            mean: 0.0,
            std: 0.0,
        };
    }
    let mean = values.iter().map(|&v| f64::from(v)).sum::<f64>() / n as f64;
    let variance = values
        .iter()
        .map(|&v| (f64::from(v) - mean).powi(2))
        .sum::<f64>()
        / (n.max(2) - 1) as f64;
    let scale = Scale {
        mean: mean as f32,
        std: variance.sqrt() as f32,
    };
    let divisor = 1e-5 + scale.std;
    for v in values {
        *v = (*v - scale.mean) / divisor;
    }
    scale
}

impl Scale {
    /// `v * std + mean`.
    fn restore(self, v: f32) -> f32 {
        v * self.std + self.mean
    }
}

// ---------------------------------------------------------------------
// Ports
// ---------------------------------------------------------------------

/// One input or output of a session: name, whether it is a `float32` tensor,
/// and dimensions (`-1`: not fixed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    pub name: String,
    pub float32: bool,
    pub dims: Vec<i64>,
}

fn ports(outlets: &[Outlet]) -> Vec<Port> {
    outlets
        .iter()
        .map(|o| Port {
            name: o.name().to_string(),
            float32: matches!(o.dtype().tensor_type(), Some(TensorElementType::Float32)),
            dims: o
                .dtype()
                .tensor_shape()
                .map(|s| s.to_vec())
                .unwrap_or_default(),
        })
        .collect()
}

/// A float32 tensor of this name and exactly these dimensions.
fn check_port(ports: &[Port], name: &str, want: &[i64]) -> Result<()> {
    let bad = |what: String| DemucsError::UnexpectedExport(format!("{MODEL_FILE}: {what}"));
    let got = ports.iter().find(|p| p.name == name).ok_or_else(|| {
        bad(format!(
            "no `{name}`; it has {:?}",
            ports.iter().map(|p| p.name.as_str()).collect::<Vec<_>>()
        ))
    })?;
    if !got.float32 {
        return Err(bad(format!("`{name}` is not a float32 tensor")));
    }
    if got.dims != want {
        return Err(bad(format!(
            "`{name}` is {:?}, expected {want:?}",
            got.dims
        )));
    }
    Ok(())
}

/// Check that a graph is the one this module reads: the two inputs and two
/// outputs of the table in the module docs, by name, type and shape. A
/// different export (another name, a dynamic batch, a six-source model) is an
/// error that says what differs, not a model that runs and returns something
/// else.
pub fn check_ports(inputs: &[Port], outputs: &[Port]) -> Result<()> {
    let spec = (frames() as i64, FREQS as i64);
    check_port(
        inputs,
        "x",
        &[1, (CHANNELS * PLANES) as i64, spec.1, spec.0],
    )?;
    check_port(inputs, "xt", &[1, CHANNELS as i64, SEGMENT_SAMPLES as i64])?;
    check_port(
        outputs,
        "x_out",
        &[1, (SOURCES * CHANNELS * PLANES) as i64, spec.1, spec.0],
    )?;
    check_port(
        outputs,
        "xt_out",
        &[1, (SOURCES * CHANNELS) as i64, SEGMENT_SAMPLES as i64],
    )
}

// ---------------------------------------------------------------------
// The network
// ---------------------------------------------------------------------

/// Runs the graph: `x` (`[4, 2048, 336]`, normalised) and `xt`
/// (`[2, 343980]`, normalised) in, `x_out` (`[16, 2048, 336]`) and `xt_out`
/// (`[8, 343980]`) out, flat and row-major. The outputs are lent to
/// `use_outputs` rather than returned, because the runtime owns them.
pub trait Backend {
    fn run<R>(
        &mut self,
        x: Vec<f32>,
        xt: Vec<f32>,
        use_outputs: impl FnOnce(&[f32], &[f32]) -> Result<R>,
    ) -> Result<R>;
}

/// The htdemucs network around a [`Backend`]: the STFT, the per-segment
/// normalisation, the graph, and the way back (see the module docs).
pub struct Network<B: Backend> {
    backend: B,
    stft: Stft,
}

impl<B: Backend> Network<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            stft: Stft::new(),
        }
    }
}

impl<B: Backend> SegmentModel for Network<B> {
    fn segment_samples(&self) -> usize {
        SEGMENT_SAMPLES
    }

    fn sources(&self) -> &[&'static str] {
        &MODEL_SOURCES
    }

    fn separate_segment(&mut self, segment: &Stereo) -> Result<Vec<Stereo>> {
        let n = SEGMENT_SAMPLES;
        if segment[0].len() != n || segment[1].len() != n {
            return Err(DemucsError::Pipeline(format!(
                "a segment is {n} samples per channel, got {} and {}",
                segment[0].len(),
                segment[1].len()
            )));
        }
        let Self { backend, stft } = self;

        let mut x = stft.spec_cac(&[&segment[0], &segment[1]]);
        let x_scale = normalise(&mut x);
        let mut xt = Vec::with_capacity(CHANNELS * n);
        xt.extend_from_slice(&segment[0]);
        xt.extend_from_slice(&segment[1]);
        let xt_scale = normalise(&mut xt);

        backend.run(x, xt, |x_out, xt_out| {
            let spec_len = SOURCES * CHANNELS * PLANES * FREQS * frames();
            if x_out.len() != spec_len || xt_out.len() != SOURCES * CHANNELS * n {
                return Err(DemucsError::Pipeline(format!(
                    "the network returned {} and {} values, expected {spec_len} and {}",
                    x_out.len(),
                    xt_out.len(),
                    SOURCES * CHANNELS * n
                )));
            }
            // Step 2, then 3: the frequency branch back to a waveform, plus
            // the time branch.
            let planes: Vec<f32> = x_out.iter().map(|&v| x_scale.restore(v)).collect();
            let spectral = stft.ispec_cac(&planes, n);
            drop(planes);
            let mut out = Vec::with_capacity(SOURCES);
            for s in 0..SOURCES {
                let mut source: Stereo = [Vec::new(), Vec::new()];
                for (c, channel) in source.iter_mut().enumerate() {
                    let row = (s * CHANNELS + c) * n..(s * CHANNELS + c + 1) * n;
                    *channel = xt_out[row.clone()]
                        .iter()
                        .zip(&spectral[row])
                        .map(|(&t, &f)| xt_scale.restore(t) + f)
                        .collect();
                }
                out.push(source);
            }
            Ok(out)
        })
    }
}

// ---------------------------------------------------------------------
// ONNX Runtime
// ---------------------------------------------------------------------

/// The graph on ONNX Runtime.
pub struct OrtBackend {
    session: Session,
}

/// The network as the app runs it.
pub type Htdemucs = Network<OrtBackend>;

fn ort_error(what: &str, e: impl std::fmt::Display) -> DemucsError {
    DemucsError::Ort(format!("{what}: {e}"))
}

impl Network<OrtBackend> {
    /// Open the model in `dir`, the folder `models.json`'s `htdemucs` was
    /// downloaded to (or a developer's copy of it): [`MODEL_FILE`] and
    /// [`WEIGHTS_FILE`] side by side.
    ///
    /// The ONNX Runtime library is looked for first
    /// ([`ml_pipeline::runtime::ensure`]), then the files, then the graph is
    /// checked with [`check_ports`]. Nothing is downloaded.
    pub fn load(dir: &Path, options: SessionOptions) -> Result<Self> {
        ml_pipeline::runtime::ensure()
            .map_err(|e| DemucsError::RuntimeUnavailable(e.to_string()))?;
        if !dir.exists() {
            return Err(DemucsError::ModelMissing {
                path: dir.display().to_string(),
            });
        }
        let graph: PathBuf = dir.join(MODEL_FILE);
        for file in [&graph, &dir.join(WEIGHTS_FILE)] {
            if !file.is_file() {
                return Err(DemucsError::ModelMissing {
                    path: file.display().to_string(),
                });
            }
        }
        let session = onnx_session::build(&graph, options).map_err(|e| {
            use ml_pipeline::Error as E;
            match e {
                E::MissingRuntime { .. } | E::RuntimeLoad { .. } | E::RuntimeUnsupported { .. } => {
                    DemucsError::RuntimeUnavailable(e.to_string())
                }
                other => DemucsError::Ort(format!("{}: {other}", graph.display())),
            }
        })?;
        check_ports(&ports(session.inputs()), &ports(session.outputs()))?;
        Ok(Self::new(OrtBackend { session }))
    }
}

impl Backend for OrtBackend {
    fn run<R>(
        &mut self,
        x: Vec<f32>,
        xt: Vec<f32>,
        use_outputs: impl FnOnce(&[f32], &[f32]) -> Result<R>,
    ) -> Result<R> {
        let x = Tensor::from_array(([1usize, CHANNELS * PLANES, FREQS, frames()], x))
            .map_err(|e| ort_error("input x", e))?;
        let xt = Tensor::from_array(([1usize, CHANNELS, SEGMENT_SAMPLES], xt))
            .map_err(|e| ort_error("input xt", e))?;
        let outputs = self
            .session
            .run(ort::inputs!["x" => x, "xt" => xt])
            .map_err(|e| ort_error("run", e))?;
        let (_, x_out) = outputs["x_out"]
            .try_extract_tensor::<f32>()
            .map_err(|e| ort_error("output x_out", e))?;
        let (_, xt_out) = outputs["xt_out"]
            .try_extract_tensor::<f32>()
            .map_err(|e| ort_error("output xt_out", e))?;
        use_outputs(x_out, xt_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(name: &str, dims: &[i64]) -> Port {
        Port {
            name: name.to_string(),
            float32: true,
            dims: dims.to_vec(),
        }
    }

    fn pinned_inputs() -> Vec<Port> {
        vec![port("x", &[1, 4, 2048, 336]), port("xt", &[1, 2, 343_980])]
    }

    fn pinned_outputs() -> Vec<Port> {
        vec![
            port("x_out", &[1, 16, 2048, 336]),
            port("xt_out", &[1, 8, 343_980]),
        ]
    }

    // -- the normalisation --------------------------------------------------

    /// Worked by hand: [1, 2, 3, 4] has mean 2.5 and, dividing by n - 1 = 3,
    /// variance 5/3, so a deviation of 1.2909944. torch's `std` agrees.
    #[test]
    fn normalise_takes_off_the_mean_and_divides_by_torchs_deviation() {
        let mut v = vec![1.0f32, 2.0, 3.0, 4.0];
        let scale = normalise(&mut v);
        assert_eq!(scale.mean, 2.5);
        assert!((scale.std - 1.290_994_4).abs() < 1e-6, "{}", scale.std);
        // (1 - 2.5) / (1e-5 + 1.2909944)
        let first = -1.5 / (1e-5 + 1.290_994_4);
        assert!((v[0] - first).abs() < 1e-6, "{}", v[0]);
        assert!((v[3] + v[0]).abs() < 1e-6, "symmetric about the mean");
    }

    #[test]
    fn the_scale_puts_back_what_normalise_took_off() {
        let original: Vec<f32> = (0..1000)
            .map(|i| ((i * 37) % 101) as f32 * 0.01 - 0.3)
            .collect();
        let mut v = original.clone();
        let scale = normalise(&mut v);
        // The 1e-5 in the divisor is not put back: it is why this is not
        // exact, and by how little (std here is 0.29).
        for (a, b) in v.iter().zip(&original) {
            let back = scale.restore(*a);
            assert!((back - b).abs() < 1e-4, "{back} vs {b}");
        }
    }

    #[test]
    fn a_constant_or_empty_or_single_input_does_not_make_nan() {
        for mut v in [vec![0.5f32; 100], vec![], vec![3.0f32]] {
            normalise(&mut v);
            assert!(v.iter().all(|x| x.is_finite()), "{v:?}");
        }
    }

    // -- the ports ----------------------------------------------------------

    #[test]
    fn the_pinned_graphs_ports_pass() {
        check_ports(&pinned_inputs(), &pinned_outputs()).unwrap();
    }

    #[test]
    fn a_different_export_is_named_not_guessed() {
        let mut inputs = pinned_inputs();
        inputs[0].name = "input".into();
        let e = check_ports(&inputs, &pinned_outputs()).unwrap_err();
        assert!(
            matches!(e, DemucsError::UnexpectedExport(_))
                && e.to_string().contains("no `x`")
                && e.to_string().contains("\"input\""),
            "{e}"
        );

        // A dynamic batch (what a re-export with dynamic axes gives).
        let mut inputs = pinned_inputs();
        inputs[1].dims = vec![-1, 2, 343_980];
        let e = check_ports(&inputs, &pinned_outputs()).unwrap_err();
        assert!(e.to_string().contains("`xt` is [-1, 2, 343980]"), "{e}");

        // htdemucs_6s: six sources.
        let mut outputs = pinned_outputs();
        outputs[0].dims = vec![1, 24, 2048, 336];
        let e = check_ports(&pinned_inputs(), &outputs).unwrap_err();
        assert!(
            e.to_string().contains("`x_out` is [1, 24, 2048, 336]"),
            "{e}"
        );

        let mut outputs = pinned_outputs();
        outputs[1].float32 = false;
        let e = check_ports(&pinned_inputs(), &outputs).unwrap_err();
        assert!(e.to_string().contains("not a float32 tensor"), "{e}");
    }

    // -- the network around a backend --------------------------------------

    /// A backend that is a Rust function: every source is `fraction` of what
    /// went in, in both branches, so that the four fractions of a quarter
    /// each give back the input. It keeps what it was given.
    struct Fractions {
        fraction: f32,
        seen: Option<(Vec<f32>, Vec<f32>)>,
        /// What to return instead of the right sizes, if anything.
        wrong_size: bool,
    }

    impl Fractions {
        fn quarters() -> Self {
            Self {
                fraction: 0.125,
                seen: None,
                wrong_size: false,
            }
        }
    }

    impl Backend for Fractions {
        fn run<R>(
            &mut self,
            x: Vec<f32>,
            xt: Vec<f32>,
            use_outputs: impl FnOnce(&[f32], &[f32]) -> Result<R>,
        ) -> Result<R> {
            let scaled = |v: &[f32]| -> Vec<f32> {
                (0..SOURCES)
                    .flat_map(|_| v.iter().map(|&a| a * self.fraction))
                    .collect()
            };
            let (x_out, mut xt_out) = (scaled(&x), scaled(&xt));
            if self.wrong_size {
                xt_out.pop();
            }
            self.seen = Some((x, xt));
            use_outputs(&x_out, &xt_out)
        }
    }

    /// Band-limited, so the iSTFT gives it back (it drops the Nyquist bin).
    /// The tests use no `offset`: a network that returns a fraction of what it
    /// is given gets the mean back whole (`fraction * (v - mean) + mean`), so
    /// with an offset the quarters are not quarters, which says nothing about
    /// the code.
    fn tones(freqs: &[f32], amplitude: f32, offset: f32) -> Vec<f32> {
        (0..SEGMENT_SAMPLES)
            .map(|i| {
                freqs
                    .iter()
                    .map(|f| (2.0 * std::f32::consts::PI * f * i as f32 / 44_100.0).sin())
                    .sum::<f32>()
                    * amplitude
                    + offset
            })
            .collect()
    }

    fn a_segment() -> Stereo {
        [
            tones(&[220.0, 1234.5, 5000.0], 0.2, 0.0),
            tones(&[330.0, 777.0], 0.3, 0.0),
        ]
    }

    /// The network gives the backend normalised inputs: zero mean and unit
    /// deviation over the whole spectrogram and over the whole waveform.
    #[test]
    fn the_backend_is_given_normalised_inputs() {
        let mut net = Network::new(Fractions::quarters());
        net.separate_segment(&a_segment()).unwrap();
        let (x, xt) = net.backend.seen.take().unwrap();
        assert_eq!(x.len(), 4 * FREQS * frames());
        assert_eq!(xt.len(), 2 * SEGMENT_SAMPLES);
        for (name, v) in [("x", &x), ("xt", &xt)] {
            let n = v.len() as f64;
            let mean = v.iter().map(|&a| f64::from(a)).sum::<f64>() / n;
            let std = (v
                .iter()
                .map(|&a| (f64::from(a) - mean).powi(2))
                .sum::<f64>()
                / (n - 1.0))
                .sqrt();
            assert!(mean.abs() < 1e-4, "{name}: mean {mean}");
            assert!((std - 1.0).abs() < 1e-3, "{name}: std {std}");
        }
        // The waveform branch is the two channels one after the other.
        assert!(xt[1000] != xt[SEGMENT_SAMPLES + 1000], "left then right");
    }

    /// The four sources of a network that splits its input in quarters add
    /// up to the input, in the middle of the segment (the iSTFT is exact only
    /// there: see `spec.rs`). That is the order of the planes, the layout of
    /// `x_out` and `xt_out`, and both ways back through the normalisation,
    /// checked at once.
    #[test]
    fn a_network_that_splits_its_input_in_quarters_gives_quarters_back() {
        let mut net = Network::new(Fractions::quarters());
        let segment = a_segment();
        let sources = net.separate_segment(&segment).unwrap();
        assert_eq!(sources.len(), 4);
        // 0.125 (time branch) + 0.125 (frequency branch) = a quarter each.
        let edge = 2048;
        for (s, source) in sources.iter().enumerate() {
            for c in 0..2 {
                assert_eq!(source[c].len(), SEGMENT_SAMPLES);
                let worst = (edge..SEGMENT_SAMPLES - edge)
                    .map(|i| (source[c][i] - 0.25 * segment[c][i]).abs())
                    .fold(0.0f32, f32::max);
                assert!(worst < 2e-4, "source {s}, channel {c}: {worst}");
            }
        }
    }

    /// Correlation of two signals over the part of the segment where the
    /// iSTFT is exact, with the mean taken off each.
    fn correlation(a: &[f32], b: &[f32]) -> f64 {
        let r = 4096..SEGMENT_SAMPLES - 4096;
        let (a, b) = (&a[r.clone()], &b[r]);
        let n = a.len() as f64;
        let (ma, mb) = (
            a.iter().map(|&v| f64::from(v)).sum::<f64>() / n,
            b.iter().map(|&v| f64::from(v)).sum::<f64>() / n,
        );
        let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
        for (&x, &y) in a.iter().zip(b) {
            let (x, y) = (f64::from(x) - ma, f64::from(y) - mb);
            ab += x * y;
            aa += x * x;
            bb += y * y;
        }
        ab / (aa.sqrt() * bb.sqrt()).max(1e-30)
    }

    /// A network that puts the frequency branch into one source (drums) and
    /// the time branch into another (vocals) and nothing into the others
    /// shows which source is which, and which channel is which.
    #[test]
    fn the_two_branches_are_added_per_source_and_per_channel() {
        struct Routed;
        impl Backend for Routed {
            fn run<R>(
                &mut self,
                x: Vec<f32>,
                xt: Vec<f32>,
                use_outputs: impl FnOnce(&[f32], &[f32]) -> Result<R>,
            ) -> Result<R> {
                let mut x_out = vec![0.0f32; SOURCES * x.len()];
                x_out[..x.len()].copy_from_slice(&x);
                let mut xt_out = vec![0.0f32; SOURCES * xt.len()];
                xt_out[3 * xt.len()..].copy_from_slice(&xt);
                use_outputs(&x_out, &xt_out)
            }
        }
        let mut net = Network::new(Routed);
        let segment = a_segment();
        let out = net.separate_segment(&segment).unwrap();
        for c in 0..2 {
            let same = |s: usize| correlation(&out[s][c], &segment[c]);
            assert!(
                same(0) > 0.999,
                "drums carry the frequency branch: {}",
                same(0)
            );
            assert!(same(3) > 0.999, "vocals carry the time branch: {}", same(3));
            assert!(
                same(1).abs() < 0.2 && same(2).abs() < 0.2,
                "bass and other carry none"
            );
            let other = correlation(&out[0][c], &segment[1 - c]);
            assert!(
                other.abs() < 0.1,
                "channel {c} is not the other one: {other}"
            );
        }
    }

    #[test]
    fn a_network_that_returns_the_wrong_sizes_is_an_error_not_a_panic() {
        let mut backend = Fractions::quarters();
        backend.wrong_size = true;
        let err = Network::new(backend)
            .separate_segment(&a_segment())
            .unwrap_err();
        assert!(matches!(err, DemucsError::Pipeline(_)), "{err:?}");
    }

    #[test]
    fn a_segment_of_the_wrong_length_is_refused() {
        let mut net = Network::new(Fractions::quarters());
        let err = net
            .separate_segment(&[vec![0.0; 100], vec![0.0; 100]])
            .unwrap_err();
        assert!(matches!(err, DemucsError::Pipeline(_)), "{err:?}");
        assert!(net.backend.seen.is_none(), "the backend was not run");
    }

    #[test]
    fn the_model_folder_is_looked_for_after_the_runtime() {
        // CI has no runtime: that is the answer, before the folder is looked at.
        let err = Htdemucs::load(Path::new("/definitely/not/here"), SessionOptions::default())
            .err()
            .unwrap();
        if ml_pipeline::runtime::ensure().is_err() {
            assert!(matches!(err, DemucsError::RuntimeUnavailable(_)), "{err:?}");
        } else {
            assert!(matches!(err, DemucsError::ModelMissing { .. }), "{err:?}");
        }
    }

    #[test]
    fn a_folder_without_the_weights_is_model_missing_when_there_is_a_runtime() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MODEL_FILE), b"graph").unwrap();
        match Htdemucs::load(dir.path(), SessionOptions::default())
            .err()
            .unwrap()
        {
            DemucsError::RuntimeUnavailable(_) => {}
            DemucsError::ModelMissing { path } => assert!(path.ends_with(WEIGHTS_FILE), "{path}"),
            other => panic!("{other:?}"),
        }
    }
}
