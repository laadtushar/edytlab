//! A track's effects see what the track produces (#313).
//!
//! Follow-up to #243. Before that fix each clip streamer ran its own
//! copy of the track's chain and the results were summed into master;
//! now the clips are summed first and one chain runs on the sum.
//!
//! For *adjacent* clips through a linear filter the two are
//! indistinguishable — superposition — which is why #243's own tests
//! barely move when the grouping is removed. For **overlapping** clips
//! through a **nonlinear** effect they are not:
//!
//! * before: `limit(a) + limit(b)`
//! * after:  `limit(a + b)`
//!
//! Only the second is a limiter. A limiter exists to guarantee a
//! ceiling, and limiting two overlapping clips independently lets their
//! sum sail straight past it — which is the one thing the effect is
//! for.
//!
//! #243 scoped this out as "related but separate", so the corrected
//! behaviour shipped resting on nothing. This is that test.

use std::path::{Path, PathBuf};

use session::{EffectInstance, SessionState};
use tempfile::TempDir;

mod support;
use support::{
    peak_lsb as peak, plain_track, render_i16, session_of, unity_clip, write_sine_trunc,
};

const SAMPLE_RATE: u32 = 44_100;
const FRAMES: u64 = SAMPLE_RATE as u64; // one second

/// Each clip's amplitude. Two of them overlapping sum to 0.6 FS, over
/// the ceiling below; one alone is 0.3 FS, well under it.
///
/// The gap is deliberately wide. At 0.5 each, a single clip peaks at
/// 16383 LSB against a 16422 LSB ceiling — 39 apart — so a render that
/// had silently dropped one clip would sit just under the ceiling and
/// pass every check here. Raised in review on #318. At 0.3 the two
/// outcomes are 9830 and 16422, which nothing can confuse.
const CLIP_AMP: f32 = 0.3;

/// −6 dB. Well above `CLIP_AMP` so a single clip is untouched, well
/// below their sum so the overlap must be caught.
const CEILING_DB: f32 = -6.0;

fn ceiling_linear() -> f32 {
    10.0f32.powf(CEILING_DB / 20.0)
}

/// The ceiling in 16-bit LSB, with a little slack for quantisation.
fn ceiling_lsb() -> i32 {
    (ceiling_linear() * 32_767.0).ceil() as i32 + 2
}

/// How close to the ceiling a limited signal is expected to land.
/// Quantisation only, so a handful of LSB.
const CEILING_TOLERANCE_LSB: i32 = 4;

fn write_tone(dir: &Path) -> PathBuf {
    write_sine_trunc(
        dir,
        "tone.wav",
        SAMPLE_RATE,
        FRAMES as usize,
        220.0,
        CLIP_AMP,
    )
}

fn limiter() -> Vec<EffectInstance> {
    vec![EffectInstance {
        kind: "limiter".to_string(),
        params: serde_json::json!({ "ceiling_db": CEILING_DB }),
        bypassed: false,
    }]
}

/// One track holding `n` copies of the same clip, all starting at zero
/// — so they overlap completely and their contributions add.
fn state(source: &Path, n: usize, effects: Vec<EffectInstance>) -> SessionState {
    let mut track = plain_track(
        "t1",
        (0..n).map(|_| unity_clip(source, 0, 0, FRAMES)).collect(),
    );
    track.effects = effects;
    session_of(vec![track], SAMPLE_RATE, FRAMES)
}

fn render(n: usize, effects: Vec<EffectInstance>) -> Vec<i16> {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    render_i16(&state(&src, n, effects))
}

/// The premise, in two halves.
///
/// Without it the real test below could pass for either of two wrong
/// reasons: because only one clip is actually reaching the mix, or
/// because a single clip was already over the ceiling and the overlap
/// is not what is being measured.
#[test]
fn the_two_clips_really_do_overlap_and_one_alone_is_under_the_ceiling() {
    let one = peak(&render(1, Vec::new()));
    let two = peak(&render(2, Vec::new()));

    assert!(
        one < ceiling_lsb(),
        "a single clip peaks at {one} LSB, already at or over the {} LSB ceiling — the test \
         below would pass without the overlap mattering",
        ceiling_lsb()
    );
    assert!(
        two > one + one / 2,
        "two fully overlapping clips peaked at {two} LSB against {one} for one clip; they are \
         not both reaching the mix, so there is no overlap to test"
    );
    assert!(
        two > ceiling_lsb(),
        "the unlimited sum ({two} LSB) does not exceed the ceiling ({} LSB), so a limiter has \
         nothing to do and the test below is vacuous",
        ceiling_lsb()
    );
}

/// The point. A limiter guarantees a ceiling, and it can only do that
/// if it sees the track's whole output.
///
/// Per-clip chains gave `limit(a) + limit(b)`: each clip passes through
/// untouched at 0.5 FS, and their sum lands at ~1.0 FS — twice the
/// ceiling the user set, from an effect whose entire job was to prevent
/// exactly that.
#[test]
fn a_limiter_holds_its_ceiling_across_overlapping_clips() {
    let limited = render(2, limiter());
    let p = peak(&limited);
    assert!(
        p <= ceiling_lsb(),
        "the mix peaks at {p} LSB against a {CEILING_DB} dB ceiling ({} LSB). The track's \
         effect chain is running per clip, so each clip was limited on its own and the sum went \
         straight past the ceiling — which is the one thing a limiter is for.",
        ceiling_lsb()
    );
}

/// And it reached the ceiling, rather than getting under it by losing
/// something.
///
/// Two ways to satisfy the test above without the fix working:
/// silencing the track, and dropping one of the two clips. Both leave
/// a peak below the ceiling, so "under the ceiling" alone proves very
/// little.
///
/// The limiter's attack is instant, so the sample that first crosses
/// the ceiling is brought to it exactly and the correct output peaks
/// *at* it (the gain then eases back, so the rest of the cycle sits a
/// little under it; a hard clip, which this used to be, held the whole
/// crest there). One clip alone cannot — it peaks
/// at `CLIP_AMP`, roughly 9830 LSB against a 16422 LSB ceiling — and
/// silence obviously cannot. So pinning the peak to the ceiling is a
/// single assertion covering both.
///
/// My first version used `peak > ceiling / 2`, which one clip alone
/// clears comfortably. Raised in review on #318.
#[test]
fn the_limited_mix_sits_at_the_ceiling() {
    let limited = render(2, limiter());
    let p = peak(&limited);
    let exact = (ceiling_linear() * 32_767.0) as i32;

    assert!(
        (p - exact).abs() <= CEILING_TOLERANCE_LSB,
        "the limited mix peaks at {p} LSB, not the {exact} LSB ceiling. Below it means a \
         contribution went missing — one clip alone would peak near {}, and silence at 0 — \
         rather than that the limiter did its job.",
        (CLIP_AMP * 32_767.0) as i32
    );

    // And the single-clip peak really is far enough away for the
    // assertion above to tell them apart.
    let one = peak(&render(1, limiter()));
    assert!(
        exact - one > CEILING_TOLERANCE_LSB * 10,
        "one clip peaks at {one} LSB against a {exact} LSB ceiling — too close for the check \
         above to distinguish a dropped clip from a limited sum"
    );
}
