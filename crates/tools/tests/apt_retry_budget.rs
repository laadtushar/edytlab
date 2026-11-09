//! The Linux build-deps step's timeout has to hold the install it wraps (#237).
//!
//! Every workflow that builds on Linux runs
//! `scripts/ci/install-linux-build-deps.sh`. Its `fetch` function is one
//! attempt: `timeout 150` on `apt-get update` and `timeout 300` on
//! `apt-get install --download-only`. A failed attempt is retried once after
//! a 10 s pause, and the packages are then installed from the downloaded
//! archives with `--no-download` under `timeout 120`. So the step's worst
//! case is `2 * (150 + 300) + 10 + 120`, and its `timeout-minutes` must hold
//! all of it, or the second attempt or the install is killed part-way.
//!
//! The cap has been wrong twice. The first version bounded the step with
//! `timeout-minutes: 6` and retried a shell function, which never fired: a
//! step timeout kills the step, so the `||` branch is unreachable. Moving
//! the timeout onto the commands made the retry real, but the cap stayed at
//! 12 minutes, less than two attempts. A comment stating the sum would not
//! stop somebody changing one `timeout N` without redoing the arithmetic,
//! so the arithmetic is checked here.
//!
//! A failed `update` must not end an attempt (the download still runs), so
//! one attempt can take 150 + 300 s. The script says so with `|| echo`
//! rather than leaning on `errexit` being suspended left of `||`, which holds
//! for the first attempt but not for the retry.
//!
//! Only downloads and a network-free install run under a timeout. When the
//! install itself was under `timeout 300`, a slow download used the time up
//! and the kill landed in dpkg mid-unpack; dpkg's journal was left pending
//! and the retry stopped at once with "dpkg was interrupted", exit 100
//! (#957's Linux leg at 332c6a5). The tests below hold that split in place.

use std::path::PathBuf;

/// Every workflow carrying the Linux build-deps step.
const WORKFLOWS: [&str; 4] = [
    "ci.yml",
    "release-dev.yml",
    "release-signed.yml",
    "coverage.yml",
];

/// The release builds, which also bundle an AppImage.
const RELEASE_WORKFLOWS: [&str; 2] = ["release-dev.yml", "release-signed.yml"];

/// The shared script every one of them runs.
const SCRIPT: &str = "scripts/ci/install-linux-build-deps.sh";

/// The longest local install measured on GitHub's ubuntu-22.04 runners:
/// unpack and configure took 8-21 s across 12 successful ci.yml and
/// release-dev.yml runs on 2026-11-09. The install's timeout must stay well
/// clear of it (at least four times), so it only ever stops a hang.
const MEASURED_LOCAL_INSTALL_MAX_SECS: u64 = 21;

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

/// The step's `run:` line, trimmed.
fn run_line(step: &str) -> &str {
    step.lines()
        .map(str::trim)
        .find(|l| l.starts_with("run:"))
        .unwrap_or_else(|| panic!("the apt step has no `run:`"))
}

/// The `N` of a `timeout N` anywhere in a command (`sudo timeout N ...`,
/// `sudo VAR=x timeout N ...` or `timeout N sudo ...`). Token-based, so
/// `Acquire::http::Timeout=15` is not one.
fn timeout_of(command: &str) -> Option<u64> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    tokens
        .windows(2)
        .find(|w| w[0] == "timeout")
        .and_then(|w| w[1].parse().ok())
}

/// The script as logical commands (backslash continuations joined, comments
/// and blank lines dropped), split into its `fetch` function and the rest.
/// Line by line and trimmed, so a Windows checkout's CRLF endings read the
/// same.
struct Script {
    fetch: Vec<String>,
    rest: Vec<String>,
}

fn script() -> Script {
    let text = repo_file(SCRIPT);
    let mut commands: Vec<String> = Vec::new();
    let mut pending = String::new();
    for line in text.lines().map(str::trim) {
        if pending.is_empty() && (line.is_empty() || line.starts_with('#')) {
            continue;
        }
        if let Some(head) = line.strip_suffix('\\') {
            pending.push_str(head);
            pending.push(' ');
        } else {
            pending.push_str(line);
            commands.push(std::mem::take(&mut pending));
        }
    }

    let mut fetch = Vec::new();
    let mut rest = Vec::new();
    let mut in_fetch = false;
    for command in commands {
        if command == "fetch() {" {
            in_fetch = true;
        } else if in_fetch && command == "}" {
            in_fetch = false;
        } else if in_fetch {
            fetch.push(command);
        } else {
            rest.push(command);
        }
    }
    assert!(!fetch.is_empty(), "{SCRIPT} has no `fetch() {{ ... }}`");
    Script { fetch, rest }
}

fn apt_commands(commands: &[String]) -> Vec<&String> {
    commands.iter().filter(|c| c.contains("apt-get")).collect()
}

/// The single install after `fetch`: the one that runs dpkg.
fn local_install(script: &Script) -> &String {
    let installs = apt_commands(&script.rest);
    assert_eq!(
        installs.len(),
        1,
        "{SCRIPT} should run apt-get once after `fetch`, to install: {installs:?}"
    );
    installs[0]
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
        let expected = if RELEASE_WORKFLOWS.contains(&name) {
            format!("run: bash {SCRIPT} --recommends libfuse2 patchelf")
        } else {
            format!("run: bash {SCRIPT}")
        };
        assert_eq!(
            run_line(step),
            expected,
            "{name}'s Linux build-deps step must run the shared script with \
             exactly these arguments: the release builds need `libfuse2` and \
             `patchelf` for the AppImage, and CI never builds one to notice"
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
    let per_attempt: Vec<u64> = apt_commands(&script.fetch)
        .iter()
        .map(|c| timeout_of(c).unwrap_or_else(|| panic!("no `timeout N` on `{c}` in `fetch`")))
        .collect();
    assert!(
        per_attempt.len() >= 2,
        "found {} apt-get commands in `fetch`; the parser is no longer \
         reading the attempt it is meant to be checking",
        per_attempt.len()
    );
    let install = local_install(&script);
    let install_secs = timeout_of(install)
        .unwrap_or_else(|| panic!("the local install has no `timeout N`: {install}"));
    assert!(
        install_secs >= 4 * MEASURED_LOCAL_INSTALL_MAX_SECS,
        "the local install's `timeout {install_secs}` is within four times \
         the longest measured install ({MEASURED_LOCAL_INSTALL_MAX_SECS} s), \
         so it could stop a slow but healthy install mid-unpack"
    );
    let worst_case = 2 * per_attempt.iter().sum::<u64>() + pause_secs(&script.rest) + install_secs;

    for name in WORKFLOWS {
        let cap = cap_secs(apt_step(&read_workflow(name)));
        assert!(
            cap >= worst_case,
            "{name}: the Linux build-deps step is capped at {cap} s but the \
             script can take {worst_case} s ({per_attempt:?} per attempt, \
             twice, the pause, and {install_secs} s for the local install), \
             so the second attempt or the install would be killed part-way. \
             Raise `timeout-minutes` or shorten a `timeout N`."
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

/// The attempts only download, so a timeout there can never stop dpkg; the
/// one install runs from the downloaded archives, without a terminal, and
/// fails rather than quietly skipping a missing archive.
#[test]
fn the_attempts_only_download() {
    let script = script();
    for command in apt_commands(&script.fetch) {
        assert!(
            command.contains("--download-only") || command.contains(" update"),
            "`fetch` runs apt-get for something other than `update` or a \
             download: {command}"
        );
        if command.contains(" update") {
            assert!(
                command.contains("|| echo"),
                "a failed `update` must not end the attempt (the retry runs \
                 under `errexit`): {command}"
            );
        }
    }
    let install = local_install(&script);
    for needed in ["--no-download", "DEBIAN_FRONTEND=noninteractive"] {
        assert!(
            install.contains(needed),
            "the local install lacks `{needed}`: {install}"
        );
    }
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
