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

/// A YAML line up to its comment: a `#` at the start or after whitespace,
/// outside quotes. A gate or a pin copied into a trailing comment is not one.
fn without_comment(line: &str) -> &str {
    let mut quote = None;
    let mut after_space = true;
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c == '#' && after_space => return &line[..i],
            None => {}
        }
        after_space = c.is_whitespace();
    }
    line
}

/// The workflow without its comments, whole-line or trailing, so an
/// explanation does not count as the thing it explains.
fn effective(yaml: &str) -> String {
    yaml.lines()
        .map(|l| without_comment(l).trim_end())
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_pin_in_a_comment_is_not_code() {
    let yaml = "      - name: Fetch\n        if: matrix.platform.runner != 'macos-15' # && x\n\
                \x20       uses: actions/cache@abc # v6\n        # key: nope\n\
                \x20       name: 'a # in quotes'";
    let code = effective(yaml);
    assert!(code.contains("uses: actions/cache@abc\n"), "{code}");
    assert!(!code.contains("&& x") && !code.contains("v6") && !code.contains("nope"));
    assert!(code.contains("'a # in quotes'"));
}

/// The text of the step named `name`: from its `- name:` line to the next
/// step's, or the end of the job.
fn step<'a>(yaml: &'a str, name: &str) -> &'a str {
    let marker = format!("- name: {name}\n");
    let start = yaml
        .find(&marker)
        .unwrap_or_else(|| panic!("no step named {name:?}"));
    let rest = &yaml[start + marker.len()..];
    // The whole rest when no later step or job follows. Bound first: clippy
    // wants a cheap `len()` passed eagerly, DeepSource (RS-W1031) wants no
    // call inside `unwrap_or`, and a plain value satisfies both.
    let to_end = rest.len();
    let end = rest
        .find("\n      - name: ")
        .or_else(|| rest.find("\n  publish:"))
        .unwrap_or(to_end);
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
const ACTION: &str = ".github/actions/bundler-tools/action.yml";
const USES: &str = "uses: ./.github/actions/bundler-tools";
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
/// The string literals of the script's `NAME = (...)` tuple, in order, with
/// implicit concatenation (`"a" "b"`, however it is wrapped) joined into one.
fn tuple_literals(script: &str, name: &str) -> Vec<String> {
    let marker = format!("\n{name} = (\n");
    let start = script
        .find(&marker)
        .unwrap_or_else(|| panic!("{SCRIPT} has no {name} tuple"))
        + marker.len();
    let block = &script[start..start + script[start..].find("\n)\n").unwrap()];
    let mut literals: Vec<String> = Vec::new();
    let mut rest = block;
    let mut joined = false; // only whitespace since the last literal closed
    while let Some(open) = rest.find('"') {
        let between = &rest[..open];
        let close = rest[open + 1..].find('"').expect("unterminated string") + open + 1;
        let text = &rest[open + 1..close];
        match literals.last_mut() {
            Some(last) if joined && between.trim().is_empty() => last.push_str(text),
            _ => literals.push(text.to_string()),
        }
        joined = true;
        rest = &rest[close + 1..];
        // A `,` or `(` between literals ends the concatenation.
        let to_end = rest.len();
        let next = rest.find('"').unwrap_or(to_end);
        if !rest[..next].trim().is_empty() {
            joined = false;
        }
    }
    literals
}

/// Each tool's name, URL, algorithm and digest together, so swapping two
/// digests, or pointing a name at another tool's URL, fails. The URLs and
/// hashes are the constants of tauri-bundler 2.10.4: `windows/msi/mod.rs`
/// (WIX_URL, WIX_SHA256), `windows/nsis/mod.rs` (NSIS_URL, NSIS_SHA1,
/// NSIS_TAURI_UTILS_URL, NSIS_TAURI_UTILS_SHA1; the bundler writes the SHA-1s
/// in capitals) and `linux/appimage/linuxdeploy.rs` (the URLs; AppRun and
/// linuxdeploy are pinned here, the bundler checks neither).
#[test]
fn each_tool_is_pinned_to_its_own_url_and_digest() {
    let script = repo_file(SCRIPT);
    assert_eq!(script_constant(&script, "TAURI_BUNDLER_VERSION"), "2.10.4");
    let gh = "https://github.com/";
    let tauri = format!("{gh}tauri-apps/binary-releases/releases/download/");
    let expected: [(&str, Vec<String>); 6] = [
        (
            "LINUX_APPRUN",
            vec![
                "AppRun-x86_64".into(),
                format!("{tauri}apprun-old/AppRun-x86_64"),
                "sha256".into(),
                "f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f".into(),
            ],
        ),
        (
            "LINUX_LINUXDEPLOY",
            vec![
                "linuxdeploy-07333c6-x86_64.AppImage".into(),
                format!("{tauri}linuxdeploy-07333c6/linuxdeploy-x86_64.AppImage"),
                "sha256".into(),
                "36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62".into(),
            ],
        ),
        (
            "LINUX_APPIMAGE_PLUGIN",
            vec![
                "linuxdeploy-plugin-appimage.AppImage".into(),
                format!(
                    "{gh}linuxdeploy/linuxdeploy-plugin-appimage/releases/download/\
                     continuous/linuxdeploy-plugin-appimage-x86_64.AppImage"
                ),
            ],
        ),
        (
            "WIX",
            vec![
                "wix314-binaries.zip".into(),
                format!("{gh}wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip"),
                "sha256".into(),
                "6ac824e1642d6f7277d0ed7ea09411a508f6116ba6fae0aa5f2c7daa2ff43d31".into(),
            ],
        ),
        (
            "NSIS",
            vec![
                "nsis-3.13.zip".into(),
                format!("{tauri}nsis-3.13/nsis-3.13.zip"),
                "sha1".into(),
                "DB14F8AF2EA346B786C6AE19343F61AAF5E09B8E".to_lowercase(),
            ],
        ),
        (
            "NSIS_TAURI_UTILS",
            vec![
                "nsis_tauri_utils.dll".into(),
                format!(
                    "{gh}tauri-apps/nsis-tauri-utils/releases/download/\
                     nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll"
                ),
                "sha1".into(),
                "75197FEE3C6A814FE035788D1C34EAD39349B860".to_lowercase(),
            ],
        ),
    ];
    for (name, fields) in expected {
        assert_eq!(tuple_literals(&script, name), fields, "{SCRIPT}: {name}");
    }
}

#[test]
fn the_tuple_reader_joins_wrapped_literals_and_keeps_fields_apart() {
    let script = "\nX = (\n    \"a\",\n    (\n        \"http://h/\"\n        \"p\"\n    ),\n    (\"sha1\", \"d\"),\n)\n";
    assert_eq!(
        tuple_literals(script, "X"),
        ["a", "http://h/p", "sha1", "d"]
    );
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
fn the_script_never_weakens_tls() {
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
}

/// The first of `python3` and `python` that runs Python, picked the way the
/// release workflows pick theirs (`"$py" -I -c 'import sys'`): a `python3`
/// that starts but does not run Python, such as the Windows Store stub, is
/// passed over rather than failing the test.
fn python() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|py| {
        std::process::Command::new(py)
            .args(["-I", "-c", "import sys"])
            .output()
            .is_ok_and(|out| out.status.success())
    })
}

/// Runs the script's own functions under `-I` with the network stubbed, so
/// what is asserted is what the code does, not what its text says. Every CI
/// runner has Python (the release workflows run their scripts with it), and
/// there a missing one fails the test; on a machine without it the test is
/// skipped, as `audio-decoder/tests/formats.rs` does for `afconvert`.
fn run_python(code: &str) -> Option<String> {
    let Some(py) = python() else {
        assert!(
            std::env::var_os("CI").is_none(),
            "neither python3 nor python runs Python on CI"
        );
        eprintln!("SKIPPED: neither python3 nor python runs Python here");
        return None;
    };
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(SCRIPT);
    let out = std::process::Command::new(py)
        .args(["-I", "-c", code])
        .arg(&script)
        .output()
        .unwrap_or_else(|e| panic!("{py} could not be run: {e}"));
    assert!(
        out.status.success(),
        "{py} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"))
}

const PY_STUBS: &str = r#"
import importlib.util, io, os, sys, tempfile, urllib.error
spec = importlib.util.spec_from_file_location("fbt", sys.argv[1])
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

class R(io.BytesIO):
    def __init__(self, data, length=None):
        super().__init__(data)
        self.headers = {} if length is None else {"Content-Length": str(length)}
    def __enter__(self):
        return self
    def __exit__(self, *a):
        return False

def no_sleep(seconds):
    pass
"#;

/// A download whose bytes are not the pinned digest fails the step and leaves
/// nothing behind; a cut-off body is retried, not accepted.
#[test]
fn a_digest_mismatch_fails_and_a_short_body_is_retried() {
    let code = format!(
        "{PY_STUBS}{}",
        r#"
with tempfile.TemporaryDirectory() as d:
    m.urllib.request.urlopen = lambda req, timeout=None: R(b"not the tool")
    try:
        m.verified(d, m.LINUX_APPRUN, no_sleep)
        print("mismatch: no error")
    except m.IntegrityError:
        print("mismatch: IntegrityError")
    print("left:", sorted(os.listdir(d)))

    calls = []
    def cut_then_whole(req, timeout=None):
        calls.append(1)
        return R(b"abc", 10) if len(calls) == 1 else R(b"0123456789", 10)
    m.urllib.request.urlopen = cut_then_whole
    dest = os.path.join(d, "out")
    err = m.download("https://example.invalid/x", dest, no_sleep)
    print("short:", err, len(calls), open(dest, "rb").read())

    m.urllib.request.urlopen = lambda req, timeout=None: R(b"abc", 10)
    err = m.download("https://example.invalid/y", os.path.join(d, "z"), no_sleep)
    print("always short:", type(err).__name__, m.retryable(err))
    try:
        m.download("http://example.invalid/x", dest, no_sleep)
    except ValueError:
        print("http: refused")
"#
    );
    let Some(out) = run_python(&code) else {
        return;
    };
    for line in [
        "mismatch: IntegrityError",
        "left: []",
        "short: None 2 b'0123456789'",
        "always short: ShortBodyError True",
        "http: refused",
    ] {
        assert!(
            out.lines().any(|l| l == line),
            "missing {line:?} in:\n{out}"
        );
    }
}

/// The NSIS folder and its plugin are installed independently: a folder
/// without the plugin still gets it.
#[test]
fn the_nsis_plugin_is_installed_into_an_existing_folder() {
    let code = format!(
        "{PY_STUBS}{}",
        r#"
import zipfile
with tempfile.TemporaryDirectory() as d:
    tools = os.path.join(d, "tools")
    os.makedirs(os.path.join(tools, "NSIS"))
    dll = os.path.join(d, "u.dll")
    open(dll, "wb").write(b"dll")
    zp = os.path.join(d, "n.zip")
    zipfile.ZipFile(zp, "w").close()
    m.verified = lambda cache, entry, sleep=None: dll if entry is m.NSIS_TAURI_UTILS else zp
    m.windows(d, tools)
    target = os.path.join(tools, "NSIS", "Plugins", "x86-unicode", "additional", "nsis_tauri_utils.dll")
    print("plugin:", os.path.isfile(target))
    print("nsis kept:", sorted(os.listdir(os.path.join(tools, "NSIS"))) == ["Plugins"])
"#
    );
    let Some(out) = run_python(&code) else {
        return;
    };
    assert!(out.lines().any(|l| l == "plugin: True"), "{out}");
    assert!(out.lines().any(|l| l == "nsis kept: True"), "{out}");
}

/// The job whose steps hold tauri-action: from `  build:` to the next job.
fn build_job(yaml: &str) -> &str {
    let start = yaml.find("\n  build:\n").expect("a build job") + 1;
    let rest = &yaml[start..];
    let to_end = rest.len();
    let end = rest.find("\n  publish:").unwrap_or(to_end);
    &rest[..end]
}

#[test]
fn the_action_caches_the_verified_downloads_then_fetches() {
    let script = repo_file(SCRIPT);
    let bundler = script_constant(&script, "TAURI_BUNDLER_VERSION");
    let action = effective(&repo_file(ACTION));

    let uses = action
        .lines()
        .find(|l| l.trim_start().starts_with("uses: actions/cache@"))
        .expect("the action uses actions/cache");
    let sha = uses
        .trim()
        .trim_start_matches("uses: actions/cache@")
        .trim();
    assert!(
        sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()),
        "pin actions/cache to a commit, not {sha}"
    );
    assert!(
        action.contains(&format!(
            "key: bundler-tools-${{{{ runner.os }}}}-tauri-bundler-{bundler}-\
             ${{{{ hashFiles('{SCRIPT}') }}}}"
        )),
        "the cache key names the OS, the bundler version ({bundler}) and the script that \
         holds the pins:\n{action}"
    );
    assert!(action.contains("path: ${{ runner.temp }}/bundler-tool-downloads"));
    assert!(
        action.contains(&format!(
            "-I {SCRIPT} \"$RUNNER_TEMP/bundler-tool-downloads\""
        )),
        "the fetch reads the folder the cache keeps:\n{action}"
    );
    assert!(
        action.find("actions/cache@").unwrap() < action.find(SCRIPT).unwrap(),
        "cache first, then fetch"
    );
    assert!(
        !action.contains("continue-on-error"),
        "a hash mismatch has to fail the leg"
    );
}

#[test]
fn both_release_workflows_call_it_before_tauri_action_in_the_build_job() {
    for workflow in WORKFLOWS {
        let yaml = effective(&repo_file(workflow));
        let job = build_job(&yaml);

        let call = step(job, FETCH_STEP);
        assert!(call.contains(USES), "{workflow}: call the action:\n{call}");
        // macOS downloads no tools, so it neither restores nor saves a cache.
        assert!(
            call.contains("if: matrix.platform.runner != 'macos-15'"),
            "{workflow}: the Linux and Windows legs only:\n{call}"
        );
        assert!(
            !call.contains("continue-on-error"),
            "{workflow}: a hash mismatch has to fail the leg"
        );

        let build = job
            .find("uses: tauri-apps/tauri-action@")
            .unwrap_or_else(|| panic!("{workflow}: no tauri-action in the build job"));
        assert!(
            position(job, FETCH_STEP) < build,
            "{workflow}: fetch the tools before tauri-action builds"
        );
    }
}
