//! The one way to render a node (ADR 0003, #714).
//!
//! Every path that renders a node has to do three things, and each used to
//! be a rule a caller remembered: put back audio a history sweep removed
//! (`rederive::materialize`, #98), use the bounded preview cache and hold
//! the entry when something will read the file again (#575, #607), and
//! report the stored effects the renderer leaves out (#520, #531, #542).
//! [`node`] does all three and hands back a [`Rendered`], so a caller cannot
//! get a file without the list of what it left out.
//!
//! Callers: the `render_preview` and `render_final` tools, and the window's
//! `render_preview`, `prepare_compare` and `render_range` commands. Only this
//! module and the paths that move the head (`fork_node`, `revert_to`,
//! `apply_diff`, the window's undo and accept) call `materialize`.
//!
//! ## Locks
//!
//! The window may not hold the store's lock while a mix renders (it would
//! stop every other command for as long as the render takes), and takes the
//! engine's lock alone. [`node`] therefore reaches the store and the engine
//! through [`Access`]: a tool hands it the `Store` and `Engine` it already
//! holds, the window hands it their `Mutex`es, and the module locks each for
//! the one step that needs it and no longer. The store's lock is never held
//! while the engine's is taken.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use audio_engine::{Engine, TimeRange};
use session::{NodeId, SessionState, Store};

use crate::preview_cache::{excerpt_key, Held, PreviewCache};

/// A stored effect a render leaves out (#520, #532): where it sits, which
/// entry, its kind and why. Its `Display` is the sentence the tools report.
pub use audio_engine::effect_chain::SkippedEffect as LeftOutEffect;

/// What a render of `state` leaves out, from the stored chains alone: no
/// render and no audio, so it answers for a render served from the cache as
/// well as for one just made.
pub fn left_out(state: &SessionState) -> Vec<LeftOutEffect> {
    audio_engine::effect_chain::skipped_effects(state)
}

/// The sentences the tools' `warnings` array carries for `left_out`.
pub fn sentences(left_out: &[LeftOutEffect]) -> Vec<String> {
    left_out.iter().map(ToString::to_string).collect()
}

/// How a caller reaches a `T` (the store, the engine): by holding it, or by
/// locking the `Mutex` that does. [`node`] reaches each only inside
/// [`Access::with`].
pub trait Access<T> {
    /// Run `f` on the `T`. `label` names it if its lock is poisoned.
    fn with<R>(&self, label: &'static str, f: impl FnOnce(&T) -> R) -> Result<R, Error>;
}

impl<T> Access<T> for T {
    fn with<R>(&self, _label: &'static str, f: impl FnOnce(&T) -> R) -> Result<R, Error> {
        Ok(f(self))
    }
}

impl<T> Access<T> for Mutex<T> {
    fn with<R>(&self, label: &'static str, f: impl FnOnce(&T) -> R) -> Result<R, Error> {
        let guard = self.lock().map_err(|_| Error::Poisoned(label))?;
        Ok(f(&guard))
    }
}

/// What to render of the node.
#[derive(Debug, Clone, Copy)]
pub enum Span {
    /// The whole mix.
    Whole,
    /// `[start_frame, end_frame)` of the rendered output.
    Frames(TimeRange),
}

/// Where the render goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    /// The project's preview cache: the whole mix under the node's id, an
    /// excerpt under a key that names its range.
    Cache {
        /// Hold the entry from before it is looked up (see
        /// [`PreviewCache::hold`]), for a caller that reads the file again.
        hold: bool,
        /// A cached file that does not read as a WAV is not a hit: it is
        /// removed and rendered again. Without it the file is served as it
        /// is, and [`Rendered::audio`] is `None` if it cannot be read.
        verify: bool,
    },
    /// A file. `destination` is the file the caller named, and is refused
    /// first if it is audio the project reads (#653); `path` is where the
    /// render is written, which is `destination` itself unless the caller
    /// encodes the render into it afterwards, as `render_final` does for a
    /// compressed format.
    File {
        path: &'a Path,
        destination: &'a Path,
    },
}

/// A render to make.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub node: NodeId,
    pub span: Span,
    pub target: Target<'a>,
}

/// What a rendered file holds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Audio {
    pub frames: u64,
    pub sample_rate: u32,
    pub channels: u16,
    /// Known for a render made now, not for a file served from the cache.
    pub peak_dbfs: Option<f32>,
}

/// A rendered node: the file, the hold on it if one was asked for, and what
/// the render left out.
#[derive(Debug)]
#[must_use = "a hold is released when this is dropped"]
pub struct Rendered {
    pub path: PathBuf,
    pub hold: Option<Held>,
    pub left_out: Vec<LeftOutEffect>,
    /// What the file holds: `None` only for a cached file served unverified
    /// that cannot be read as a WAV.
    pub audio: Option<Audio>,
    /// Whether the file was already in the cache (or another render of the
    /// same key put it there first) rather than rendered by this call.
    pub cached: bool,
}

/// Why a node was not rendered.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A lock the render needs was poisoned.
    #[error("internal: a state mutex was poisoned ({0})")]
    Poisoned(&'static str),
    /// The destination is audio the project reads (#653); the text says which.
    #[error("{0}")]
    Refused(String),
    /// Audio the node names is gone and could not be rebuilt; the text says
    /// which file and why.
    #[error("{0}")]
    MissingAudio(String),
    /// The node could not be read.
    #[error("{0}")]
    Node(session::Error),
    /// The renderer failed.
    #[error("{0}")]
    Render(audio_engine::Error),
    /// The render could not be stored in the cache at `dir`.
    #[error("could not store the preview in the project's preview cache ({}): {source}", dir.display())]
    Cache {
        dir: PathBuf,
        source: std::io::Error,
    },
    /// The cached file at the path cannot be read as a WAV.
    #[error("the cached preview at {} cannot be read", .0.display())]
    Unreadable(PathBuf),
}

/// Render `request.node`.
///
/// Puts the node's audio back first, then renders to the request's target:
/// for the cache, the existing entry if there is one (holding it first when
/// asked) and a render into it if not; for a file, the render itself.
/// Always answers with what the render left out.
pub fn node<S, E>(store: &S, engine: &E, request: Request<'_>) -> Result<Rendered, Error>
where
    S: Access<Store>,
    E: Access<Engine>,
{
    let Request { node, span, target } = request;
    let range = match span {
        Span::Whole => None,
        Span::Frames(r) => Some(r),
    };

    // One lock of the store for what needs it, released before any render.
    let (state, project_dir) = store.with("store", |store| -> Result<_, Error> {
        if let Target::File { destination, .. } = target {
            crate::destination::refuse_project_audio(store, [destination])
                .map_err(Error::Refused)?;
        }
        // Its audio may have been swept from history; put it back first.
        crate::rederive::materialize(store, node).map_err(Error::MissingAudio)?;
        let n = store.get(node).map_err(Error::Node)?;
        Ok((n.state, store.project_dir().to_path_buf()))
    })??;
    let left_out = left_out(&state);

    let (hold, verify) = match target {
        Target::File { path, .. } => {
            let report = engine
                .with("engine", |e| e.render_to_wav(&state, path, range))?
                .map_err(Error::Render)?;
            return Ok(Rendered {
                path: path.to_path_buf(),
                hold: None,
                left_out,
                audio: Some(fresh(&report)),
                cached: false,
            });
        }
        Target::Cache { hold, verify } => (hold, verify),
    };

    // The whole mix is cached by node id (#164): the id is a hash of the
    // state, so undo and redo land on renders that already exist. An excerpt
    // is cached too, under a key that names its range, so it is never served
    // in place of the mix (#535).
    let key = match range {
        None => node,
        Some(r) => excerpt_key(node, r.start_frame, r.end_frame),
    };
    let cache = PreviewCache::new(&project_dir);
    // Taken before the entry is looked up, so no eviction can take it.
    let hold = hold.then(|| cache.hold(key));

    if let Some(cached) = cache.lookup(key) {
        let audio = wav_header(&cached);
        if audio.is_some() || !verify {
            return Ok(Rendered {
                path: cached,
                hold,
                left_out,
                audio,
                cached: true,
            });
        }
        // A file that cannot be read is a miss: render over it below.
        let _ = std::fs::remove_file(&cached);
    }

    let mut report = None;
    let stored = cache.get_or_render::<_, Failure>(key, |staged| {
        let rendered = engine
            .with("engine", |e| e.render_to_wav(&state, staged, range))
            .map_err(Failure::Module)?
            .map_err(|e| Failure::Module(Error::Render(e)))?;
        report = Some(rendered);
        Ok(())
    });
    let (path, _) = stored.map_err(|failure| match failure {
        Failure::Module(e) => e,
        Failure::Io(source) => Error::Cache {
            dir: cache.dir().to_path_buf(),
            source,
        },
    })?;

    finish(path, hold, left_out, report, verify)
}

/// The answer for a cache entry at `path`: rendered by this call when
/// `report` is there, otherwise another render of the same key finished
/// first, its file is the one in the cache, and ours was never made.
fn finish(
    path: PathBuf,
    hold: Option<Held>,
    left_out: Vec<LeftOutEffect>,
    report: Option<audio_engine::RenderReport>,
    verify: bool,
) -> Result<Rendered, Error> {
    if let Some(report) = report {
        return Ok(Rendered {
            path,
            hold,
            left_out,
            audio: Some(fresh(&report)),
            cached: false,
        });
    }
    let audio = wav_header(&path);
    if audio.is_none() && verify {
        return Err(Error::Unreadable(path));
    }
    Ok(Rendered {
        path,
        hold,
        left_out,
        audio,
        cached: true,
    })
}

/// Why `get_or_render` did not give a file: the render, or storing it.
enum Failure {
    Module(Error),
    Io(std::io::Error),
}

impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Failure::Io(e)
    }
}

fn fresh(report: &audio_engine::RenderReport) -> Audio {
    Audio {
        frames: report.frames_written,
        sample_rate: report.sample_rate,
        channels: report.channels,
        peak_dbfs: Some(report.peak_dbfs),
    }
}

/// Read a WAV's `fmt ` and `data` chunk headers.
///
/// A cache hit still has to report frames, rate and channels, and those are
/// properties of the file rather than of the session: reading the header
/// keeps a hit honest without decoding a sample, and a file too damaged to
/// parse reads as `None`, not as a preview with made-up numbers.
fn wav_header(path: &Path) -> Option<Audio> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut riff = [0u8; 12];
    file.read_exact(&mut riff).ok()?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return None;
    }

    let (mut rate, mut channels, mut bits, mut data_bytes) = (0u32, 0u16, 0u16, 0u64);
    let mut header = [0u8; 8];
    while file.read_exact(&mut header).is_ok() {
        let id = [header[0], header[1], header[2], header[3]];
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as u64;
        match &id {
            b"fmt " => {
                let mut fmt = vec![0u8; size as usize];
                file.read_exact(&mut fmt).ok()?;
                if fmt.len() < 16 {
                    return None;
                }
                channels = u16::from_le_bytes([fmt[2], fmt[3]]);
                rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                bits = u16::from_le_bytes([fmt[14], fmt[15]]);
            }
            b"data" => {
                data_bytes = size;
                break;
            }
            _ => {
                // Chunks are word-aligned, so an odd size carries a pad
                // byte that is not counted in the size field.
                let skip = size + (size & 1);
                std::io::copy(&mut file.by_ref().take(skip), &mut std::io::sink()).ok()?;
            }
        }
    }

    let bytes_per_frame = (bits as u64 / 8) * channels as u64;
    if bytes_per_frame == 0 || rate == 0 {
        return None;
    }
    Some(Audio {
        frames: data_bytes / bytes_per_frame,
        sample_rate: rate,
        channels,
        peak_dbfs: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(path: &Path, frames: u32) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for _ in 0..frames * 2 {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn a_file_another_render_made_first_is_a_hit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mix.wav");
        wav(&path, 10);
        let got = finish(path.clone(), None, Vec::new(), None, true).unwrap();
        assert!(got.cached);
        assert_eq!(got.path, path);
        let audio = got.audio.unwrap();
        assert_eq!(
            (audio.frames, audio.sample_rate, audio.channels),
            (10, 8_000, 2)
        );
        assert_eq!(audio.peak_dbfs, None);
    }

    #[test]
    fn an_unreadable_file_is_served_as_it_is_or_refused_when_verified() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mix.wav");
        std::fs::write(&path, b"not audio").unwrap();

        let served = finish(path.clone(), None, Vec::new(), None, false).unwrap();
        assert!(served.cached && served.audio.is_none());

        let refused = finish(path.clone(), None, Vec::new(), None, true).unwrap_err();
        assert!(matches!(&refused, Error::Unreadable(p) if *p == path));
        assert!(refused.to_string().contains("cannot be read"), "{refused}");
    }

    #[test]
    fn a_render_made_now_reports_its_peak() {
        let report = audio_engine::RenderReport {
            frames_written: 5,
            sample_rate: 8_000,
            channels: 1,
            peak_dbfs: -6.0,
            warnings: Vec::new(),
        };
        let got = finish(PathBuf::from("x.wav"), None, Vec::new(), Some(report), true).unwrap();
        assert!(!got.cached);
        assert_eq!(got.audio.unwrap().peak_dbfs, Some(-6.0));
    }

    /// Every way a WAV header can fail to read.
    #[test]
    fn a_damaged_header_reads_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let read = |name: &str, bytes: &[u8]| {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            wav_header(&path)
        };
        assert!(wav_header(&dir.path().join("missing.wav")).is_none());
        assert!(read("short.wav", b"RIFF").is_none());
        assert!(read("not-riff.wav", b"RIFX\0\0\0\0WAVE").is_none());

        let chunk = |id: &[u8; 4], body: &[u8]| {
            let mut c = id.to_vec();
            c.extend_from_slice(&(body.len() as u32).to_le_bytes());
            c.extend_from_slice(body);
            if body.len() % 2 == 1 {
                c.push(0);
            }
            c
        };
        let file = |chunks: &[Vec<u8>]| {
            let mut f = b"RIFF\0\0\0\0WAVE".to_vec();
            for c in chunks {
                f.extend_from_slice(c);
            }
            f
        };
        let mut fmt = vec![0u8; 16];
        fmt[0] = 1;
        fmt[2] = 1; // mono
        fmt[4..8].copy_from_slice(&8_000u32.to_le_bytes());
        fmt[14] = 16;
        let data = chunk(b"data", &[0u8; 8]);

        // A fmt chunk shorter than 16 bytes.
        let bytes = file(&[chunk(b"fmt ", &[0u8; 4]), data.clone()]);
        assert!(read("short-fmt.wav", &bytes).is_none());
        // A fmt chunk cut off by the end of the file.
        let mut cut = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        cut.extend_from_slice(&16u32.to_le_bytes());
        assert!(read("cut-fmt.wav", &cut).is_none());
        // A chunk that says it is longer than the file, before any fmt.
        let mut long = b"RIFF\0\0\0\0WAVELIST".to_vec();
        long.extend_from_slice(&1_000u32.to_le_bytes());
        long.extend_from_slice(&[0u8; 4]);
        assert!(read("long-chunk.wav", &long).is_none());
        // No data chunk is no frames; no fmt chunk is nothing to report.
        assert_eq!(
            read("no-data.wav", &file(&[chunk(b"fmt ", &fmt)])).map(|a| a.frames),
            Some(0)
        );
        assert!(read("no-fmt.wav", &file(std::slice::from_ref(&data))).is_none());

        // An odd-sized chunk before the data is skipped with its pad byte.
        let good = file(&[chunk(b"LIST", &[1u8; 3]), chunk(b"fmt ", &fmt), data]);
        let audio = read("odd.wav", &good).expect("reads");
        assert_eq!(
            (audio.frames, audio.sample_rate, audio.channels),
            (4, 8_000, 1)
        );
    }
}
