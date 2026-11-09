//! Both release workflows fetch the Tauri bundler's tools themselves, with
//! retries, before `tauri build` runs.
//!
//! `tauri build` downloads WiX and NSIS (Windows) and AppRun and linuxdeploy
//! (Linux) from GitHub with no retry, and skips a download when the file is
//! already in `<cache dir>/tauri`. A 504 from GitHub therefore ended a whole
//! release leg (dev.677 on Windows, dev.688 on Linux and Windows).
//! `scripts/fetch-bundler-tools.py` downloads the same files with retries,
//! checks them against pinned hashes and puts them where the bundler looks;
//! a CI cache keeps the verified downloads between runs.
//!
//! The pins and the cache key belong to one bundler release, so what a test
//! can do is hold them to the dependency that decides that release, and hold
//! each workflow's two steps in place (cache, then fetch, then build).
//!
//! ## Why a text test
//!
//! Same reason as `release_ml_runtime.rs`: no GitHub Actions step runs here.

use std::path::PathBuf;

fn repo_file(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

/// The workflow without its comment-only lines, so an explanation does
/// not count as the thing it explains.
fn effective(yaml: &str) -> String {
    yaml.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text of the step named `name`: from its `- name:` line to the next
/// step's, or the end of the job.
fn step<'a>(yaml: &'a str, name: &str) -> &'a str {
    let marker = format!("- name: {name}\n");
    let start = yaml
        .find(&marker)
        .unwrap_or_else(|| panic!("no step named {name:?}"));
    let rest = &yaml[start + marker.len()..];
    let end = rest
        .find("\n      - name: ")
        .or_else(|| rest.find("\n  publish:"))
        .unwrap_or(rest.len());
    &yaml[start..start + marker.len() + end]
}

fn position(yaml: &str, name: &str) -> usize {
    yaml.find(&format!("- name: {name}\n"))
        .unwrap_or_else(|| panic!("no step named {name:?}"))
}

const WORKFLOWS: [&str; 2] = [
    ".github/workflows/release-dev.yml",
    ".github/workflows/release-signed.yml",
];

const SCRIPT: &str = "scripts/fetch-bundler-tools.py";
const CACHE_STEP: &str = "Cache the bundler's tool downloads";
const FETCH_STEP: &str = "Fetch the bundler's tools";

/// The value a `NAME = "value"` line in the script gives.
fn script_constant(script: &str, name: &str) -> String {
    let line = script
        .lines()
        .find(|l| l.starts_with(&format!("{name} = ")))
        .unwrap_or_else(|| panic!("{SCRIPT} has no {name}"));
    line.split('"')
        .nth(1)
        .unwrap_or_else(|| panic!("{name} is not a quoted string: {line}"))
        .to_string()
}

/// `@tauri-apps/cli` as pnpm-lock.yaml resolves it.
fn locked_cli_version() -> String {
    let lock = repo_file("pnpm-lock.yaml");
    let line = lock
        .lines()
        .find(|l| l.trim_start().starts_with("'@tauri-apps/cli@"))
        .expect("pnpm-lock.yaml resolves @tauri-apps/cli");
    line.trim()
        .trim_start_matches("'@tauri-apps/cli@")
        .trim_end_matches(':')
        .trim_end_matches('\'')
        .to_string()
}

/// The pins are the constants of one tauri-bundler release, which the CLI
/// version decides (tauri-cli depends on an exact tauri-bundler). A CLI bump
/// must come back to the script, re-read the bundler's constants and move
/// both versions; until then the pins and the cache key describe a bundler
/// that is no longer the one running.
#[test]
fn the_pins_belong_to_the_locked_tauri_cli() {
    let script = repo_file(SCRIPT);
    let pinned = script_constant(&script, "TAURI_CLI_VERSION");
    assert_eq!(
        pinned,
        locked_cli_version(),
        "@tauri-apps/cli moved: read tauri-cli-<version>/Cargo.toml for its exact \
         `tauri-bundler` requirement, then tauri-bundler-<version>/src/bundle/{{windows/msi/mod.rs, \
         windows/nsis/mod.rs, linux/appimage/linuxdeploy.rs}} for the URLs and hashes, and update \
         {SCRIPT} (both versions and the pins) and the cache key in both release workflows"
    );
}

/// The three hashes the bundler itself checks (tauri-bundler 2.10.4:
/// `windows/msi/mod.rs` WIX_SHA256, `windows/nsis/mod.rs` NSIS_SHA1 and
/// NSIS_TAURI_UTILS_SHA1), with the URLs they belong to. The bundler writes
/// the SHA-1s in capitals.
#[test]
fn the_script_pins_what_the_bundler_checks_and_downloads_from_its_urls() {
    let script = repo_file(SCRIPT);
    assert_eq!(script_constant(&script, "TAURI_BUNDLER_VERSION"), "2.10.4");
    for needle in [
        "https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip",
        "6ac824e1642d6f7277d0ed7ea09411a508f6116ba6fae0aa5f2c7daa2ff43d31",
        "https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.13/nsis-3.13.zip",
        &"DB14F8AF2EA346B786C6AE19343F61AAF5E09B8E".to_lowercase(),
        "https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll",
        &"75197FEE3C6A814FE035788D1C34EAD39349B860".to_lowercase(),
        "https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64",
        "https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-07333c6/linuxdeploy-x86_64.AppImage",
    ] {
        assert!(script.contains(needle), "{SCRIPT} should pin {needle}");
    }
}

/// The bundler looks in `<cache dir>/tauri` unless `bundle.useLocalToolsDir`
/// moves it into `target/.tauri`, where the script would not put anything.
#[test]
fn the_bundler_keeps_its_tools_where_the_script_puts_them() {
    let conf = repo_file("apps/desktop/src-tauri/tauri.conf.json");
    assert!(
        !conf.contains("useLocalToolsDir"),
        "tauri.conf.json sets useLocalToolsDir, which moves the bundler's tool folder to \
         target/.tauri; {SCRIPT} writes to <cache dir>/tauri"
    );
}

/// A pinned digest that is not checked, or a download that skips TLS
/// verification, would make the pins decoration.
#[test]
fn the_script_never_skips_a_check() {
    let script = repo_file(SCRIPT);
    for weakened in [
        "CERT_NONE",
        "_create_unverified_context",
        "check_hostname = False",
    ] {
        assert!(
            !script.contains(weakened),
            "{SCRIPT} must not weaken TLS verification ({weakened})"
        );
    }
    assert!(
        script.contains("raise IntegrityError") && script.contains("return 1"),
        "a digest mismatch has to fail the step"
    );
}

#[test]
fn both_release_workflows_cache_then_fetch_the_tools_before_building() {
    let script = repo_file(SCRIPT);
    let bundler = script_constant(&script, "TAURI_BUNDLER_VERSION");
    for workflow in WORKFLOWS {
        let yaml = effective(&repo_file(workflow));

        let cache = step(&yaml, CACHE_STEP);
        let uses = cache
            .lines()
            .find(|l| l.trim_start().starts_with("uses: actions/cache@"))
            .unwrap_or_else(|| panic!("{workflow}: the cache step uses actions/cache"));
        let sha = uses
            .trim()
            .trim_start_matches("uses: actions/cache@")
            .split_whitespace()
            .next()
            .unwrap();
        assert!(
            sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()),
            "{workflow}: pin actions/cache to a commit, not {sha}"
        );
        assert!(
            cache.contains(&format!(
                "key: bundler-tools-${{{{ runner.os }}}}-tauri-bundler-{bundler}-\
                 ${{{{ hashFiles('{SCRIPT}') }}}}"
            )),
            "{workflow}: the cache key names the OS, the bundler version ({bundler}) and the \
             script that holds the pins:\n{cache}"
        );
        assert!(
            cache.contains("path: ${{ runner.temp }}/bundler-tool-downloads"),
            "{workflow}: cache the verified downloads:\n{cache}"
        );

        let fetch = step(&yaml, FETCH_STEP);
        assert!(
            fetch.contains(&format!(
                "-I {SCRIPT} \"$RUNNER_TEMP/bundler-tool-downloads\""
            )),
            "{workflow}: the fetch reads the folder the cache step keeps:\n{fetch}"
        );

        // macOS downloads no tools, so it neither restores nor saves a cache.
        for (name, text) in [(CACHE_STEP, cache), (FETCH_STEP, fetch)] {
            assert!(
                text.contains("if: matrix.platform.runner != 'macos-15'"),
                "{workflow}: {name} runs on the Linux and Windows legs only"
            );
            assert!(
                !text.contains("continue-on-error"),
                "{workflow}: {name} must fail the leg when a hash does not match"
            );
        }

        let build = yaml
            .find("uses: tauri-apps/tauri-action@")
            .unwrap_or_else(|| panic!("{workflow}: no tauri-action step"));
        assert!(
            position(&yaml, CACHE_STEP) < position(&yaml, FETCH_STEP)
                && position(&yaml, FETCH_STEP) < build,
            "{workflow}: cache, then fetch, then tauri-action builds"
        );
    }
}
