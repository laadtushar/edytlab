//! The Linux build-deps step's timeout has to hold the install it wraps (#237).
//!
//! Every workflow that builds on Linux runs
//! `scripts/ci/install-linux-build-deps.sh`. Its `fetch` function is one
//! attempt: `timeout 150` on `apt-get update` and `timeout 300` on
//! `apt-get install --download-only`. A failed attempt is retried once after
//! a 10 s pause, and the packages are then installed from the downloaded
//! archives with `--no-download` and no timeout. So the step's worst case is
//! `2 * (150 + 300) + 10` plus that local install, and its `timeout-minutes`
//! must hold all of it, or the second attempt is killed part-way through.
//!
//! The cap has been wrong twice. The first version bounded the step with
//! `timeout-minutes: 6` and retried a shell function, which never fired: a
//! step timeout kills the step, so the `||` branch is unreachable. Moving
//! the timeout onto the commands made the retry real, but the cap stayed at
//! 12 minutes, less than two attempts. A comment stating the sum would not
//! stop somebody changing one `timeout N` without redoing the arithmetic,
//! so the arithmetic is checked here.
//!
//! The budget depends on an `errexit` subtlety: GitHub runs steps under
//! `bash -e`, and `errexit` is suspended inside a function on the left of
//! `||`. A timed-out `update` therefore does *not* end the attempt (the
//! download still runs), so 450 s in one attempt is reachable.
//!
//! Only the download runs under a timeout. When the install itself was under
//! `timeout 300`, a slow download used the time up and the kill landed in
//! dpkg mid-unpack; dpkg's journal was left pending and the retry stopped at
//! once with "dpkg was interrupted", exit 100 (#957's Linux leg at 332c6a5).
//! The tests below hold that split in place.

use std::path::PathBuf;

/// Every workflow carrying the Linux build-deps step.
const WORKFLOWS: [&str; 4] = [
    "ci.yml",
    "release-dev.yml",
    "release-signed.yml",
    "coverage.yml",
];

/// The shared script every one of them runs.
const SCRIPT: &str = "scripts/ci/install-linux-build-deps.sh";

/// Seconds the budget allows for the local install, which has no timeout
/// of its own. Measured on GitHub's ubuntu-22.04 runners: unpack and
/// configure took 8-21 s across 12 successful ci.yml and release-dev.yml
/// runs on 2026-11-09 (the release builds install a few more packages).
const LOCAL_INSTALL_SECS: u64 = 120;

fn repo_file(path: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

fn read_workflow(name: &str) -> String {
    repo_file(&format!(".github/workflows/{name}"))
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

/// The step's `timeout-minutes`, in seconds.
fn cap_secs(step: &str) -> u64 {
    let minutes: u64 = step
        .lines()
        .find_map(|l| l.trim().strip_prefix("timeout-minutes:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_else(|| panic!("no `timeout-minutes:` on the apt step"));
    minutes * 60
}

/// Every `sudo timeout N` in `text`, in order.
///
/// Anchored on `sudo ` rather than the bare word: `Acquire::http::Timeout`
/// is a different setting, and comments quote the numbers in prose.
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

/// The script split into its `fetch` function and everything else, both
/// with comment lines dropped. Line by line and trimmed, so a Windows
/// checkout's CRLF endings read the same.
struct Script {
    fetch: Vec<String>,
    rest: Vec<String>,
}

fn script() -> Script {
    let text = repo_file(SCRIPT);
    let mut fetch = Vec::new();
    let mut rest = Vec::new();
    let mut in_fetch = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line == "fetch() {" {
            in_fetch = true;
        } else if in_fetch && line == "}" {
            in_fetch = false;
        } else if in_fetch {
            fetch.push(line.to_string());
        } else {
            rest.push(line.to_string());
        }
    }
    assert!(!fetch.is_empty(), "{SCRIPT} has no `fetch() {{ ... }}`");
    Script { fetch, rest }
}

/// Seconds of pause before the retry, from `sleep N`.
fn pause_secs(rest: &[String]) -> u64 {
    rest.iter()
        .find_map(|l| l.strip_prefix("sleep "))
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or_else(|| panic!("{SCRIPT} has no `sleep N` before the retry"))
}

#[test]
fn every_workflow_runs_the_shared_script() {
    for name in WORKFLOWS {
        let yaml = read_workflow(name);
        let step = apt_step(&yaml);
        assert!(
            step.contains(&format!("run: bash {SCRIPT}")),
            "{name}'s Linux build-deps step no longer runs {SCRIPT}"
        );
        assert!(
            !step.contains("apt-get"),
            "{name}'s Linux build-deps step calls apt-get itself; the retry \
             and its budget live in {SCRIPT}"
        );
    }
}

#[test]
fn the_step_cap_holds_two_downloads_and_the_local_install() {
    let script = script();
    let per_attempt = sudo_timeouts(&script.fetch.join("\n"));
    assert!(
        per_attempt.len() >= 2,
        "found {} `timeout N` commands in `fetch`; the parser is no longer \
         reading the attempt it is meant to be checking",
        per_attempt.len()
    );
    let outside = sudo_timeouts(&script.rest.join("\n"));
    assert!(
        outside.is_empty(),
        "{SCRIPT} has `timeout N` outside `fetch` ({outside:?}); this budget \
         counts only `fetch` twice plus the local install"
    );
    let worst_case =
        2 * per_attempt.iter().sum::<u64>() + pause_secs(&script.rest) + LOCAL_INSTALL_SECS;

    for name in WORKFLOWS {
        let cap = cap_secs(apt_step(&read_workflow(name)));
        assert!(
            cap >= worst_case,
            "{name}: the Linux build-deps step is capped at {cap} s but the \
             script can take {worst_case} s ({per_attempt:?} per attempt, \
             twice, the pause, and {LOCAL_INSTALL_SECS} s for the local \
             install), so the second attempt or the install would be killed \
             part-way. Raise `timeout-minutes` or shorten a `timeout N`."
        );
    }
}

/// The copies call one script, so a cap fixed in one workflow and not the
/// others is the only drift left to catch.
#[test]
fn every_workflow_carrying_this_step_has_the_same_budget() {
    let caps: Vec<(&str, u64)> = WORKFLOWS
        .iter()
        .map(|n| (*n, cap_secs(apt_step(&read_workflow(n)))))
        .collect();
    let first = caps[0].1;
    for (name, cap) in &caps {
        assert_eq!(
            *cap, first,
            "{name} caps the Linux build-deps step at {cap} s while {} uses \
             {first} s",
            caps[0].0
        );
    }
}

/// A timeout may only ever stop a download. The install that unpacks and
/// configures runs from the downloaded archives with nothing that can kill
/// dpkg part-way, and fails rather than quietly skipping a missing archive.
#[test]
fn only_the_download_runs_under_a_timeout() {
    let script = script();
    let fetch = script.fetch.join("\n");
    assert!(
        fetch.contains("--download-only"),
        "`fetch` no longer downloads only: an install under `timeout` can be \
         killed mid-unpack and leave dpkg interrupted"
    );
    assert_eq!(
        fetch.matches("apt-get").count(),
        fetch.matches("sudo timeout ").count(),
        "every apt-get in `fetch` must run under `timeout`"
    );

    let installs: Vec<&String> = script
        .rest
        .iter()
        .filter(|l| l.contains("apt-get") && l.contains(" install"))
        .collect();
    assert_eq!(
        installs.len(),
        1,
        "{SCRIPT} should install once, after `fetch`: {installs:?}"
    );
    let install = installs[0];
    assert!(
        install.contains("--no-download") && !install.contains("timeout"),
        "the local install must use `--no-download` and run under no \
         timeout: {install}"
    );
    assert!(
        !script
            .fetch
            .iter()
            .chain(&script.rest)
            .any(|l| l.contains("--ignore-missing")),
        "`--ignore-missing` would let the install silently leave out a \
         package whose archive is missing"
    );
    assert!(
        script.rest.iter().any(|l| l == "fetch || {")
            && script.rest.iter().filter(|l| *l == "fetch").count() == 1,
        "{SCRIPT} no longer retries `fetch` once after a failure"
    );
}
