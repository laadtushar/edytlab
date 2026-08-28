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

use std::path::PathBuf;

/// Every workflow carrying the Linux build-deps step.
const WORKFLOWS: [&str; 3] = ["ci.yml", "release-dev.yml", "release-signed.yml"];

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
/// `timeout N` values inside it.
struct AptStep {
    cap_secs: u64,
    command_timeouts: Vec<u64>,
    pause_secs: u64,
}

fn parse_apt_step(yaml: &str) -> AptStep {
    let start = yaml
        .find("Install Linux build deps")
        .unwrap_or_else(|| panic!("no `Install Linux build deps` step"));
    // The step ends at the next step's `- name:` at the same depth.
    let rest = &yaml[start..];
    let end = rest[1..]
        .find("\n      - name:")
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    let step = &rest[..end];

    let cap_minutes: u64 = step
        .lines()
        .find_map(|l| l.trim().strip_prefix("timeout-minutes:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_else(|| panic!("no `timeout-minutes:` on the apt step"));

    let mut command_timeouts = Vec::new();
    // `sudo timeout 150 apt-get …`. Anchored on `sudo ` rather than the
    // bare word: `Acquire::http::Timeout` is a different setting, and
    // the comment above the cap quotes the numbers in prose, which an
    // unanchored match happily counted a second time.
    for (i, _) in step.match_indices("sudo timeout ") {
        let after = &step[i + "sudo timeout ".len()..];
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            if let Ok(secs) = digits.parse::<u64>() {
                command_timeouts.push(secs);
            }
        }
    }

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
        command_timeouts,
        pause_secs,
    }
}

#[test]
fn the_step_cap_fits_two_full_attempts() {
    for name in WORKFLOWS {
        let step = parse_apt_step(&read_workflow(name));

        assert!(
            step.command_timeouts.len() >= 2,
            "{name}: found {} `timeout N` commands in the apt step; the parser \
             is no longer reading the retry it is meant to be checking",
            step.command_timeouts.len()
        );

        let one_attempt: u64 = step.command_timeouts.iter().sum();
        let worst_case = 2 * one_attempt + step.pause_secs;

        assert!(
            step.cap_secs >= worst_case,
            "{name}: the apt step is capped at {} s but two attempts can take \
             {worst_case} s ({:?} per attempt, twice, plus {} s between) — the \
             retry would be killed part-way through the second `apt-get \
             install`. Raise `timeout-minutes` or shorten a `timeout N`.",
            step.cap_secs,
            step.command_timeouts,
            step.pause_secs,
        );
    }
}

/// The three copies are the same step; a fix applied to one is a bug
/// left in the other two.
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
             the step is duplicated across all three and a budget fixed in \
             one is a budget still broken in the others",
            caps[0].0
        );
    }
}
