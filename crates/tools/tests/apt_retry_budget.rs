//! The apt step's timeout has to fit the retry it wraps (#237).
//!
//! The step retries `install_deps`, and each attempt is `timeout 150`
//! on `apt-get update` plus `timeout 300` on `apt-get install`, with a
//! 10 s pause between them: `2 * (150 + 300) + 10 = 910 s`. The step
//! cap was `timeout-minutes: 12` — 720 s — so in the tail case where
//! the first attempt fails *slowly* rather than stalling outright, the
//! retry began around 7.7 minutes in with 4.3 left and was killed
//! part-way through `apt-get install`.
//!
//! That is the same class of mistake as the one the step's own comment
//! describes fixing. The first version bounded the step with
//! `timeout-minutes: 6` and retried a shell function, which never fired
//! because a step timeout kills the step and the `||` branch is
//! unreachable. Moving the timeout onto the command created a real
//! retry; the cap was never widened to hold two of them.
//!
//! A comment stating the sum would have been enough for a careful
//! reader and useless against the actual failure mode, which is
//! somebody adjusting one `timeout N` and not redoing the arithmetic.
//! So the arithmetic is checked here.
//!
//! Note the `errexit` subtlety this budget depends on: GitHub runs
//! steps under `bash -e`, and `errexit` is suspended inside a function
//! on the left of `||`. A timed-out `update` therefore does *not* abort
//! the attempt — `install` still runs — so 450 s in a single attempt is
//! reachable rather than theoretical.
//!
//! Before the second attempt the step also runs, once, the dpkg recovery
//! a first attempt killed mid-unpack needs (`dpkg --configure -a`, then
//! `apt-get -f install`; without it the retry stopped at "dpkg was
//! interrupted", exit 100). Those commands are outside `install_deps`, so
//! they count once, not twice: `2 * (150 + 300) + 10 + 60 + 120 = 1090 s`.

use std::path::PathBuf;

/// Every workflow carrying the Linux build-deps step.
const WORKFLOWS: [&str; 4] = [
    "ci.yml",
    "release-dev.yml",
    "release-signed.yml",
    "coverage.yml",
];

/// Seconds of pause between the two attempts, from `sleep 10`.
const RETRY_PAUSE_SECS: u64 = 10;

fn read_workflow(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.github/workflows")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

/// The `install_deps` step: its `timeout-minutes` cap and the
/// `timeout N` values in it.
struct AptStep {
    cap_secs: u64,
    /// `sudo timeout N` inside `install_deps`, which runs once per attempt.
    per_attempt: Vec<u64>,
    /// `sudo timeout N` outside it: the recovery between the attempts, run once.
    once: Vec<u64>,
    pause_secs: u64,
}

/// Every `sudo timeout N` in `text`, in order.
///
/// Anchored on `sudo ` rather than the bare word: `Acquire::http::Timeout`
/// is a different setting, and the comment above the cap quotes the
/// numbers in prose, which an unanchored match happily counted a second
/// time.
fn sudo_timeouts(text: &str) -> Vec<u64> {
    text.match_indices("sudo timeout ")
        .filter_map(|(i, _)| {
            text[i + "sudo timeout ".len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .ok()
        })
        .collect()
}

/// The text of the `Install Linux build deps` step.
fn apt_step(yaml: &str) -> &str {
    let start = yaml
        .find("Install Linux build deps")
        .unwrap_or_else(|| panic!("no `Install Linux build deps` step"));
    // The step ends at the next step's `- name:` at the same depth.
    let rest = &yaml[start..];
    let end = rest[1..]
        .find("\n      - name:")
        .map_or(rest.len(), |i| i + 1);
    &rest[..end]
}

fn parse_apt_step(yaml: &str) -> AptStep {
    let step = apt_step(yaml);

    let cap_minutes: u64 = step
        .lines()
        .find_map(|l| l.trim().strip_prefix("timeout-minutes:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_else(|| panic!("no `timeout-minutes:` on the apt step"));

    // `install_deps() {` up to the line that is only its closing `}`.
    let fn_start = step
        .find("install_deps() {")
        .unwrap_or_else(|| panic!("no `install_deps() {{` in the apt step"));
    let mut fn_len = None;
    let mut offset = 0;
    for line in step[fn_start..].split_inclusive('\n') {
        offset += line.len();
        if line.trim() == "}" {
            fn_len = Some(offset);
            break;
        }
    }
    let fn_end = fn_start + fn_len.unwrap_or_else(|| panic!("`install_deps` has no closing `}}`"));
    let per_attempt = sudo_timeouts(&step[fn_start..fn_end]);
    let once = sudo_timeouts(&format!("{}{}", &step[..fn_start], &step[fn_end..]));

    let pause_secs = step
        .find("sleep ")
        .and_then(|i| {
            step[i + "sleep ".len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .ok()
        })
        .unwrap_or(RETRY_PAUSE_SECS);

    AptStep {
        cap_secs: cap_minutes * 60,
        per_attempt,
        once,
        pause_secs,
    }
}

#[test]
fn the_step_cap_fits_two_full_attempts() {
    for name in WORKFLOWS {
        let step = parse_apt_step(&read_workflow(name));

        assert!(
            step.per_attempt.len() >= 2,
            "{name}: found {} `timeout N` commands in `install_deps`; the parser \
             is no longer reading the retry it is meant to be checking",
            step.per_attempt.len()
        );

        let one_attempt: u64 = step.per_attempt.iter().sum();
        let recovery: u64 = step.once.iter().sum();
        let worst_case = 2 * one_attempt + step.pause_secs + recovery;

        assert!(
            step.cap_secs >= worst_case,
            "{name}: the apt step is capped at {} s but two attempts can take \
             {worst_case} s ({:?} per attempt, twice, plus {} s between and \
             {:?} once for the dpkg recovery) — the retry would be killed \
             part-way through the second `apt-get install`. Raise \
             `timeout-minutes` or shorten a `timeout N`.",
            step.cap_secs,
            step.per_attempt,
            step.pause_secs,
            step.once,
        );
    }
}

/// The copies (one per workflow in `WORKFLOWS`) are the same step; a fix applied to one
/// is a bug left in the others.
#[test]
fn every_workflow_carrying_this_step_has_the_same_budget() {
    let caps: Vec<(&str, u64)> = WORKFLOWS
        .iter()
        .map(|n| (*n, parse_apt_step(&read_workflow(n)).cap_secs))
        .collect();

    let first = caps[0].1;
    for (name, cap) in &caps {
        assert_eq!(
            *cap, first,
            "{name} caps the apt step at {cap} s while {} uses {first} s — \
             the step is duplicated across every workflow in WORKFLOWS and a budget fixed in \
             one is a budget still broken in the others",
            caps[0].0
        );
    }
}

/// A first attempt that `timeout` killed mid-unpack leaves dpkg's journal
/// pending, and the retry's `apt-get install` then stops at once ("dpkg was
/// interrupted", exit 100, #957's Linux leg at 332c6a5). Every copy has to
/// replay the journal and repair what it left before the second attempt.
#[test]
fn the_retry_recovers_dpkg_before_trying_again() {
    for name in WORKFLOWS {
        let yaml = read_workflow(name);
        let step = apt_step(&yaml);
        let retry = &step[step
            .find("install_deps || {")
            .unwrap_or_else(|| panic!("{name}: no `install_deps || {{` retry"))..];
        let at = |needle: &str| {
            retry
                .find(needle)
                .unwrap_or_else(|| panic!("{name}: the retry has no `{needle}`"))
        };
        let configure = at("sudo timeout 60 dpkg --configure -a");
        let fix = at("sudo timeout 120 apt-get -f install -y");
        let second = at("\n            install_deps\n");
        assert!(
            configure < fix && fix < second,
            "{name}: the retry must run `dpkg --configure -a`, then `apt-get -f \
             install`, then the second attempt, in that order"
        );
    }
}
