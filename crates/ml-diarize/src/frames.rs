// Derived from pyannote.audio 3.3.2, https://github.com/pyannote/pyannote-audio
// (core/inference.py, pipelines/utils/diarization.py). MIT License, Copyright (c) 2020 CNRS.
// Changed by edytlab: re-implemented in Rust, without PyTorch, NumPy, SciPy
// or numba; this module's documentation says what differs.
// The upstream licence text is in
// apps/desktop/src-tauri/licenses/THIRD_PARTY_NOTICES.txt (CREDITS, section F).

//! The frame grid of the segmentation network, and what the pipeline does on
//! it after the clustering: counting the speakers in each frame, giving each
//! cluster its frames, and cutting the frames into turns.
//!
//! The network answers every 10 s window with one row per frame, about every
//! 17 ms. Windows start every second and so overlap 90 %, and every frame of
//! the recording is seen by up to ten of them. This is the arithmetic that
//! puts their answers on one time axis, a port of the numpy parts of
//! pyannote.audio 3.3.2: `Inference.aggregate` and `Inference.trim`
//! (`core/inference.py`), `SpeakerDiarizationMixin.speaker_count` and
//! `.to_diarization` (`pipelines/utils/diarization.py`), `SpeakerDiarization.
//! reconstruct` (`pipelines/speaker_diarization.py`) and `Binarize`
//! (`utils/signal.py`). The fixtures are what those functions made on a
//! real recording (`tests/fixtures/make_fixtures.py`).
//!
//! ## The grid
//!
//! Frame `i` of a window that starts at `s` covers `[s + i * STEP, s + i *
//! STEP + DURATION]` and its middle is `s + i * STEP + DURATION / 2`: the
//! network's receptive field is 991 samples wide and moves 270 samples a
//! frame. A window's first frame is `round(s / STEP)` frames into the
//! recording, rounding halves to even as numpy's `rint` does. Not every
//! window lands on a whole frame, which is why the answers are summed on this
//! grid and not stacked.

/// Samples in one window the network is run on: 10 s at 16 kHz.
pub const CHUNK_SAMPLES: usize = 160_000;
/// Frames the network answers a window with: the 991-sample receptive field
/// moved 270 samples at a time over [`CHUNK_SAMPLES`], `(160000 - 991) / 270 + 1`
/// rounded down. [`Segmentation::total_frames`] and every index built on it
/// assume it.
pub const FRAMES_PER_WINDOW: usize = 589;
/// Seconds in one window.
pub const CHUNK_SECONDS: f64 = 10.0;
/// Seconds a frame of the network's output spans: 991 samples.
pub const FRAME_DURATION: f64 = 991.0 / 16_000.0;
/// Seconds between two frames: 270 samples.
pub const FRAME_STEP: f64 = 270.0 / 16_000.0;

/// numpy's `rint`: to the nearest integer, halves to the even one.
fn rint(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// `SlidingWindow.closest_frame` for the output grid.
pub fn closest_frame(t: f64) -> i64 {
    rint((t - 0.5 * FRAME_DURATION) / FRAME_STEP)
}

/// The time at the middle of frame `i` of the recording.
pub fn frame_middle(i: usize) -> f64 {
    i as f64 * FRAME_STEP + 0.5 * FRAME_DURATION
}

/// The network's answers for every window of a recording, as the pipeline
/// uses them: which of the 3 local speakers is speaking in each frame, 0 or 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Segmentation {
    /// Windows.
    pub chunks: usize,
    /// Frames in a window (589).
    pub frames: usize,
    /// Local speakers the network tells apart (3).
    pub speakers: usize,
    /// Seconds between the starts of two windows.
    pub step_s: f64,
    /// `chunks * frames * speakers` values, window first, then frame, then speaker.
    pub data: Vec<u8>,
}

impl Segmentation {
    pub fn new(chunks: usize, frames: usize, speakers: usize, step_s: f64, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), chunks * frames * speakers, "a 0/1 per cell");
        Self {
            chunks,
            frames,
            speakers,
            step_s,
            data,
        }
    }

    /// 1 if local speaker `s` speaks in frame `f` of window `c`.
    pub fn at(&self, c: usize, f: usize, s: usize) -> u8 {
        self.data[(c * self.frames + f) * self.speakers + s]
    }

    /// The row of window `c`, frame `f`: one value per local speaker.
    pub fn row(&self, c: usize, f: usize) -> &[u8] {
        let at = (c * self.frames + f) * self.speakers;
        &self.data[at..at + self.speakers]
    }

    /// Whether local speaker `s` speaks at all in window `c`.
    pub fn active(&self, c: usize, s: usize) -> bool {
        (0..self.frames).any(|f| self.at(c, f, s) != 0)
    }

    /// How many frames of the recording there are: where the last window's
    /// last frame is (`Inference.aggregate`'s `num_frames`).
    pub fn total_frames(&self) -> usize {
        if self.chunks == 0 {
            return 0;
        }
        let end = CHUNK_SECONDS + (self.chunks - 1) as f64 * self.step_s + 0.5 * FRAME_DURATION;
        (closest_frame(end) + 1).max(0) as usize
    }

    /// The recording's frame that frame 0 of window `c` is.
    pub fn first_frame(&self, c: usize) -> usize {
        closest_frame(c as f64 * self.step_s + 0.5 * FRAME_DURATION).max(0) as usize
    }

    /// The number of speakers talking in each frame of the recording:
    /// `speaker_count` with no warm-up trimmed. Every window that sees a frame
    /// says how many are speaking in it, and the answers are averaged and
    /// rounded (halves to even).
    pub fn speaker_count(&self) -> Vec<u8> {
        let total = self.total_frames();
        let mut sum = vec![0.0f32; total];
        let mut seen = vec![0.0f32; total];
        for c in 0..self.chunks {
            let first = self.first_frame(c);
            for f in 0..self.frames {
                let n: u8 = self.row(c, f).iter().sum();
                sum[first + f] += f32::from(n);
                seen[first + f] += 1.0;
            }
        }
        sum.iter()
            .zip(&seen)
            .map(|(s, n)| (s / n.max(1e-12)).round_ties_even() as u8)
            .collect()
    }
}

/// Which cluster each local speaker of each window was given, or `None` for a
/// speaker that does not speak in that window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub speakers: usize,
    /// `chunks * speakers` clusters, window first.
    pub clusters: Vec<Option<usize>>,
}

impl Assignment {
    pub fn get(&self, c: usize, s: usize) -> Option<usize> {
        self.clusters[c * self.speakers + s]
    }

    /// How many clusters there are: the highest id plus one.
    pub fn cluster_count(&self) -> usize {
        self.clusters
            .iter()
            .flatten()
            .max()
            .map_or(0, |highest| highest + 1)
    }
}

/// Which clusters are speaking in each frame: `frames * clusters` values of 0 or 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub frames: usize,
    pub clusters: usize,
    pub data: Vec<u8>,
}

impl Activity {
    pub fn at(&self, frame: usize, cluster: usize) -> u8 {
        self.data[frame * self.clusters + cluster]
    }
}

/// Give each cluster its frames: `reconstruct` and `to_diarization`.
///
/// In each window the frames of all the local speakers that were given a
/// cluster are combined (a cluster speaks where any of them does). The
/// windows' answers are then added up on the recording's frames, and in every
/// frame the `count` clusters with the most votes are the ones speaking, ties
/// going to the lower cluster number. The count caps how many speak at once,
/// however the clusters disagree.
pub fn reconstruct(seg: &Segmentation, assignment: &Assignment, count: &[u8]) -> Activity {
    let total = count.len();
    let widest = usize::from(count.iter().copied().max().unwrap_or(0));
    let clusters = assignment.cluster_count().max(widest);
    let mut votes = vec![0.0f32; total * clusters];
    for c in 0..seg.chunks {
        let first = seg.first_frame(c);
        for f in 0..seg.frames {
            for k in 0..assignment.cluster_count() {
                let speaking = (0..seg.speakers)
                    .any(|s| assignment.get(c, s) == Some(k) && seg.at(c, f, s) != 0);
                if speaking {
                    votes[(first + f) * clusters + k] += 1.0;
                }
            }
        }
    }
    let mut data = vec![0u8; total * clusters];
    let mut order: Vec<usize> = Vec::with_capacity(clusters);
    for (frame, &n) in count.iter().enumerate() {
        let row = &votes[frame * clusters..(frame + 1) * clusters];
        order.clear();
        order.extend(0..clusters);
        // Stable: equal votes keep the lower cluster first.
        order.sort_by(|a, b| row[*b].total_cmp(&row[*a]));
        for &k in order.iter().take(usize::from(n)) {
            data[frame * clusters + k] = 1;
        }
    }
    Activity {
        frames: total,
        clusters,
        data,
    }
}

/// A stretch of one cluster speaking, in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Turn {
    pub start_s: f64,
    pub end_s: f64,
    pub cluster: usize,
}

/// Cut the frames into turns: `Binarize` with both thresholds at 0.5 and no
/// minimum durations, which is the pipeline's setting. A turn starts at the
/// middle of the first frame the cluster speaks in and ends at the middle of
/// the first frame after it that it does not; a turn still open at the last
/// frame ends there. Turns are ordered by start, then cluster.
pub fn turns(activity: &Activity) -> Vec<Turn> {
    let mut out = Vec::new();
    if activity.frames == 0 {
        return out;
    }
    for k in 0..activity.clusters {
        let mut start = frame_middle(0);
        let mut speaking = activity.at(0, k) != 0;
        for f in 1..activity.frames {
            let t = frame_middle(f);
            match (speaking, activity.at(f, k) != 0) {
                (true, false) => {
                    out.push(Turn {
                        start_s: start,
                        end_s: t,
                        cluster: k,
                    });
                    speaking = false;
                }
                (false, true) => {
                    start = t;
                    speaking = true;
                }
                _ => {}
            }
        }
        if speaking {
            out.push(Turn {
                start_s: start,
                end_s: frame_middle(activity.frames - 1),
                cluster: k,
            });
        }
    }
    out.sort_by(|a, b| {
        a.start_s
            .total_cmp(&b.start_s)
            .then(a.cluster.cmp(&b.cluster))
    });
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const FIXTURES: &str = include_str!("../tests/fixtures/fixtures.json");

    pub(crate) fn fixture() -> serde_json::Value {
        serde_json::from_str(FIXTURES).unwrap()
    }

    pub(crate) fn fixture_segmentation() -> Segmentation {
        let data = include_bytes!("../tests/fixtures/segmentation.u8").to_vec();
        let f = fixture();
        Segmentation::new(
            f["chunks"].as_u64().unwrap() as usize,
            f["segmentation_frames"].as_u64().unwrap() as usize,
            3,
            1.0,
            data,
        )
    }

    pub(crate) fn case(name: &str) -> serde_json::Value {
        fixture()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("no case {name}"))
            .clone()
    }

    /// The reference's `hard` clusters, with the inactive speakers left out
    /// as the pipeline does (`hard_clusters[inactive_speakers] = -2`).
    pub(crate) fn assignment(seg: &Segmentation, hard: &serde_json::Value) -> Assignment {
        let mut clusters = Vec::new();
        for (c, row) in hard.as_array().unwrap().iter().enumerate() {
            for (s, k) in row.as_array().unwrap().iter().enumerate() {
                clusters.push(seg.active(c, s).then(|| k.as_u64().unwrap() as usize));
            }
        }
        Assignment {
            speakers: seg.speakers,
            clusters,
        }
    }

    #[test]
    fn the_grid_is_the_networks_receptive_field() {
        assert_eq!(CHUNK_SAMPLES, 160_000);
        assert!((FRAME_DURATION - 0.0619375).abs() < 1e-12);
        assert!((FRAME_STEP - 0.016875).abs() < 1e-12);
        // Window c starts c seconds in; its first frame is rint(c / step).
        let seg = Segmentation::new(3, 4, 1, 1.0, vec![0; 12]);
        assert_eq!(
            [0, 1, 2].map(|c| seg.first_frame(c)),
            [0, 59, 119],
            "59.26 and 118.5 round to 59 and 119"
        );
        assert_eq!(rint(2.5), 2, "halves go to the even integer");
        assert_eq!(rint(3.5), 4);
        assert_eq!(closest_frame(frame_middle(1234)), 1234);
    }

    #[test]
    fn the_recordings_frame_count_is_the_last_windows_last_frame() {
        let seg = fixture_segmentation();
        assert_eq!(seg.total_frames(), 2371);
        assert_eq!(Segmentation::new(0, 589, 3, 1.0, vec![]).total_frames(), 0);
    }

    #[test]
    fn counts_the_speakers_in_every_frame_as_pyannote_does() {
        let seg = fixture_segmentation();
        let reference = include_bytes!("../tests/fixtures/count.u8");
        let count = seg.speaker_count();
        assert_eq!(count.len(), reference.len());
        let wrong: Vec<usize> = (0..count.len())
            .filter(|i| count[*i] != reference[*i])
            .collect();
        assert!(
            wrong.is_empty(),
            "{} frames differ, first {:?}",
            wrong.len(),
            &wrong[..wrong.len().min(5)]
        );
        assert!(
            count.contains(&2),
            "the excerpt has two people talking at once"
        );
    }

    #[test]
    fn gives_each_cluster_its_frames_as_pyannote_does() {
        let seg = fixture_segmentation();
        let free = case("free");
        let count = seg.speaker_count();
        let activity = reconstruct(&seg, &assignment(&seg, &free["hard"]), &count);
        let reference = include_bytes!("../tests/fixtures/discrete.u8");
        assert_eq!(activity.frames, free["frames"].as_u64().unwrap() as usize);
        assert_eq!(
            activity.clusters,
            free["clusters"].as_u64().unwrap() as usize
        );
        assert_eq!(activity.data.len(), reference.len());
        let wrong = activity
            .data
            .iter()
            .zip(reference)
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(wrong, 0, "{wrong} of {} cells differ", reference.len());
    }

    #[test]
    fn cuts_turns_as_pyannotes_binarize_does_for_every_setting() {
        let seg = fixture_segmentation();
        for name in ["free", "num2", "num3", "num1", "min3", "max2"] {
            let c = case(name);
            let mut count = seg.speaker_count();
            if let Some(cap) = c["args"]["num_speakers"]
                .as_u64()
                .or(c["args"]["max_speakers"].as_u64())
            {
                for n in &mut count {
                    *n = (*n).min(cap as u8);
                }
            }
            let activity = reconstruct(&seg, &assignment(&seg, &c["hard"]), &count);
            let got = turns(&activity);
            let want = c["turns"].as_array().unwrap();
            assert_eq!(got.len(), want.len(), "{name}: {got:?}");
            for (g, w) in got.iter().zip(want) {
                assert!(
                    (g.start_s - w[0].as_f64().unwrap()).abs() < 1e-9,
                    "{name}: {g:?} vs {w}"
                );
                assert!(
                    (g.end_s - w[1].as_f64().unwrap()).abs() < 1e-9,
                    "{name}: {g:?} vs {w}"
                );
                assert_eq!(
                    g.cluster as u64,
                    w[2].as_u64().unwrap(),
                    "{name}: {g:?} vs {w}"
                );
            }
        }
    }

    #[test]
    fn a_turn_open_at_the_last_frame_ends_there_and_one_frame_is_a_turn_of_no_length() {
        let on = Activity {
            frames: 3,
            clusters: 1,
            data: vec![0, 1, 1],
        };
        let t = turns(&on);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].start_s, frame_middle(1));
        assert_eq!(t[0].end_s, frame_middle(2));
        let one = Activity {
            frames: 1,
            clusters: 1,
            data: vec![1],
        };
        assert_eq!(turns(&one).len(), 1);
        assert!(turns(&Activity {
            frames: 0,
            clusters: 0,
            data: vec![]
        })
        .is_empty());
    }

    #[test]
    fn nobody_speaking_makes_no_turns() {
        let seg = Segmentation::new(2, 5, 3, 1.0, vec![0; 30]);
        assert!(seg.speaker_count().iter().all(|&n| n == 0));
        let a = Assignment {
            speakers: 3,
            clusters: vec![None; 6],
        };
        let act = reconstruct(&seg, &a, &seg.speaker_count());
        assert!(turns(&act).is_empty());
    }
}
