//! Opt-in crash reports (#958).
//!
//! The owner's decision on the issue: the app can send error reports to
//! Sentry, **off by default**, turned on by the person in Settings → Account
//! ("Send crash reports"). Three things have to hold before anything leaves
//! the computer, and each is checked here, in one place:
//!
//! * **The build has an address.** The DSN is read at build time from
//!   `EDYTLAB_SENTRY_DSN` ([`build_dsn`]); none is committed, and a build
//!   without one (every build from source, and CI's) has no client to make
//!   and so cannot send. The release workflows pass it from a repository
//!   variable or secret.
//! * **The person said yes.** The choice is `crash_reports` in
//!   `~/.edytlab/settings.json` (`crate::settings`), the app's own file for
//!   non-secret choices that must be known at launch, never the keychain.
//!   Until it is `true` no [`Client`] exists, so there is no transport, no
//!   thread and no request. Turning it off closes the client; turning it on
//!   again makes a new one. Neither needs a restart.
//! * **The report is scrubbed** ([`scrub_event`], the client's
//!   `before_send`): no paths (the home folder and every project folder
//!   opened this launch, and any other absolute path in a message), no
//!   quoted text (which is how chat text, tool arguments and tool results
//!   reach an error message: `{:?}` and `JSON.stringify` quote them), no API
//!   keys or tokens, no email addresses, no host name, no user, no
//!   breadcrumbs, no local variables or source lines. Audio is never part of
//!   an event in the first place.
//!
//! What is sent: the error's type and scrubbed message, where in edytlab's
//! code it happened (the stack's function names and source positions), the
//! app version, and the operating system and processor (the `os`, `device`
//! and `rust` contexts). Panics are reported from a hook installed once at
//! startup (release builds unwind since #908, so a panic on a worker thread
//! no longer ends the app and would otherwise go unseen); the webview's
//! uncaught errors come through the `report_webview_error` command, so there
//! is one client, one consent check and no change to the CSP.
//!
//! The client is built by hand ([`Client::with_options`]) rather than with
//! `sentry::init`, so that nothing reads `SENTRY_DSN` or the other `SENTRY_*`
//! variables from the environment, no default integration installs a panic
//! hook of its own, and events never depend on a thread-local hub that
//! captured an earlier client. It sends through [`HttpTransport`], which uses
//! the `reqwest` 0.12 and rustls the app already has: no second TLS stack.
//! No tracing, no session tracking, no logs, no metrics, no replay.

use std::borrow::Cow;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, LazyLock, Mutex, Once, RwLock};
use std::time::{Duration, Instant};

use regex::{Captures, Regex};
use sentry::protocol::{Event, Exception, Frame, Level, Mechanism, Stacktrace};
use sentry::types::Dsn;
use sentry::{Client, ClientOptions, Envelope, Transport, TransportFactory, TransportOptions};
use serde::{Deserialize, Serialize};

/// The DSN this build was given, if any: `EDYTLAB_SENTRY_DSN` when it was
/// compiled. Never committed; see the module docs.
const BUILD_DSN: Option<&str> = option_env!("EDYTLAB_SENTRY_DSN");

/// How long a report may take to leave when the client is closed or a panic
/// is being reported. A panic waits for it, so it is short.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

/// Reports waiting to be sent. A burst past this is dropped, not queued.
const QUEUE: usize = 30;

/// The longest string an event keeps, after scrubbing. A message longer than
/// this is cut, so a long dump of state does not travel whole.
const MAX_TEXT: usize = 1_000;
/// The most of one string the scrubber reads. It only bounds the work: a
/// quotation cut here still runs to the end of what is read, and is removed.
const MAX_SCRUB_INPUT: usize = 64 * 1024;

/// The most frames kept from a webview stack.
const MAX_JS_FRAMES: usize = 50;

/// The DSN this build was given, parsed. `None` for a build without one, or
/// one whose value is empty or not a DSN; then no report can ever be sent.
pub fn build_dsn() -> Option<Dsn> {
    parse_dsn(BUILD_DSN)
}

fn parse_dsn(raw: Option<&str>) -> Option<Dsn> {
    let raw = raw.map(str::trim).filter(|s| !s.is_empty())?;
    match raw.parse::<Dsn>() {
        Ok(dsn) => Some(dsn),
        Err(_) => {
            // The value is not printed: it is the build's, not the person's,
            // but a log line is no place for it either.
            tracing::warn!("EDYTLAB_SENTRY_DSN is not a valid DSN; crash reports are unavailable in this build");
            None
        }
    }
}

/// What Settings shows: whether the person turned reporting on, and whether
/// this build can send at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CrashReportsStatus {
    /// The person's choice, as saved. Off on a fresh install.
    pub enabled: bool,
    /// Whether this build has a DSN. Without one the switch changes nothing
    /// but the saved choice, and Settings says the build cannot send.
    pub available: bool,
}

/// An uncaught error in the webview, as `lib/crashReports.ts` forwards it.
#[derive(Debug, Clone, Deserialize)]
pub struct WebviewErrorReport {
    /// `"error"` (`window.onerror`) or `"unhandledrejection"`.
    #[serde(default)]
    pub kind: String,
    /// The error's `name`, such as `TypeError`.
    #[serde(default)]
    pub name: String,
    /// The error's `message`.
    #[serde(default)]
    pub message: String,
    /// The error's `stack`, as the engine wrote it.
    #[serde(default)]
    pub stack: Option<String>,
}

/// The one crash reporter of the process: the DSN, the person's choice, the
/// client while the choice is on, and what the scrubber removes.
pub struct CrashReporter {
    dsn: Option<Dsn>,
    transport: Arc<dyn TransportFactory>,
    enabled: AtomicBool,
    client: RwLock<Option<Arc<Client>>>,
    redactions: Arc<Redactions>,
}

impl std::fmt::Debug for CrashReporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrashReporter")
            .field("available", &self.available())
            .field("enabled", &self.enabled())
            .field("active", &self.active())
            .finish()
    }
}

impl Default for CrashReporter {
    /// The reporter of this build: its DSN, if it was given one, and the
    /// HTTP transport. Off until [`CrashReporter::set_enabled`].
    fn default() -> Self {
        Self::new(build_dsn())
    }
}

impl CrashReporter {
    /// A reporter for `dsn` that sends over HTTP. Off: nothing is built
    /// until the person turns it on.
    pub fn new(dsn: Option<Dsn>) -> Self {
        Self::with_transport(dsn, Arc::new(HttpTransportFactory))
    }

    /// A reporter that sends through `transport` (the tests' recorder).
    pub fn with_transport(dsn: Option<Dsn>, transport: Arc<dyn TransportFactory>) -> Self {
        Self {
            dsn,
            transport,
            enabled: AtomicBool::new(false),
            client: RwLock::new(None),
            redactions: Arc::new(Redactions::default()),
        }
    }

    /// Whether this build can send reports at all.
    pub fn available(&self) -> bool {
        self.dsn.is_some()
    }

    /// The person's choice.
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Whether a client exists: the choice is on and the build has a DSN.
    pub fn active(&self) -> bool {
        self.client
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    pub fn status(&self) -> CrashReportsStatus {
        CrashReportsStatus {
            enabled: self.enabled(),
            available: self.available(),
        }
    }

    /// Remove `path` from every report from now on, writing `replacement` in
    /// its place: `"~"` for the home folder, `"<project>"` for a project.
    pub fn redact_path(&self, path: &Path, replacement: &'static str) {
        self.redactions.add(path, replacement);
    }

    /// Turn reporting on or off for this launch. On builds the client (and
    /// so the transport's thread) if the build has a DSN; off closes it,
    /// waiting up to two seconds for a report already queued, so call it
    /// off the async runtime's workers.
    pub fn set_enabled(&self, on: bool) {
        let closed = {
            let mut slot = self.client.write().unwrap_or_else(|p| p.into_inner());
            // Stored under the client lock, so `enabled` and the client
            // never disagree for another caller.
            self.enabled.store(on, Ordering::SeqCst);
            match (&self.dsn, on) {
                (Some(dsn), true) => {
                    if slot.is_none() {
                        *slot = Some(Arc::new(self.build_client(dsn.clone())));
                    }
                    None
                }
                _ => slot.take(),
            }
        };
        if let Some(client) = closed {
            client.close(Some(FLUSH_TIMEOUT));
        }
    }

    fn build_client(&self, dsn: Dsn) -> Client {
        let mut options = ClientOptions::default();
        options.dsn = Some(dsn);
        options.release = Some(Cow::Owned(format!("edytlab@{}", env!("CARGO_PKG_VERSION"))));
        options.environment = Some(Cow::Borrowed(if cfg!(debug_assertions) {
            "development"
        } else {
            "production"
        }));
        options.send_default_pii = false;
        options.attach_stacktrace = false;
        options.max_breadcrumbs = 0;
        options.auto_session_tracking = false;
        options.default_integrations = false;
        options.server_name = None;
        options.integrations = vec![
            // The OS, the processor and the Rust version. Its `setup` fills
            // `server_name` with the host name; the scrubber removes it.
            Arc::new(sentry::integrations::contexts::ContextIntegration::new()),
            // Trims the frames of the reporting machinery off a stack.
            Arc::new(sentry::integrations::backtrace::ProcessStacktraceIntegration::new()),
        ];
        options.transport = Some(Arc::clone(&self.transport));
        let redactions = Arc::clone(&self.redactions);
        options.before_send = Some(Arc::new(move |event| scrub_event(event, &redactions)));
        options.before_breadcrumb = Some(Arc::new(|_| None));
        Client::with_options(options)
    }

    fn client(&self) -> Option<Arc<Client>> {
        self.client
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Send `event` if reporting is on. Says whether it was handed to the
    /// client (which may still drop it, as the scrubber does with an event
    /// it cannot read back).
    pub fn capture(&self, event: Event<'static>) -> bool {
        match self.client() {
            Some(client) => {
                client.capture_event(event, None);
                true
            }
            None => false,
        }
    }

    /// Report an uncaught webview error, if reporting is on.
    pub fn report_webview_error(&self, report: &WebviewErrorReport) -> bool {
        if !self.active() {
            return false;
        }
        self.capture(webview_event(report))
    }

    /// Wait up to `timeout` for what was captured to leave.
    pub fn flush(&self, timeout: Duration) -> bool {
        self.client()
            .is_none_or(|client| client.flush(Some(timeout)))
    }

    /// Report a panic, if reporting is on, and wait briefly for it to leave:
    /// a panic on the main thread may be the last thing the process does.
    fn report_panic(&self, info: &std::panic::PanicHookInfo<'_>) {
        // `try_read`: a panic while this thread holds the write lock (in
        // `set_enabled`) must not wait for itself.
        let client = match self.client.try_read() {
            Ok(slot) => slot.clone(),
            Err(_) => None,
        };
        let Some(client) = client else { return };
        let mut event =
            sentry::integrations::panic::PanicIntegration::new().event_from_panic_info(info);
        event.tags.insert("origin".into(), "rust".into());
        client.capture_event(event, None);
        client.flush(Some(FLUSH_TIMEOUT));
    }
}

/// Install the panic hook that reports panics through `reporter`, once per
/// process, in front of the hook already there (which still runs). Nothing
/// is installed for a build without a DSN. Returns whether it is installed.
pub fn install_panic_hook(reporter: Arc<CrashReporter>) -> bool {
    static INSTALLED: Once = Once::new();
    if !reporter.available() {
        return false;
    }
    INSTALLED.call_once(move || {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            thread_local! {
                static REPORTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
            }
            // A panic inside the report (the scrubber, the transport) runs
            // this hook again; that one only does what the hook before did.
            if !REPORTING.with(|r| r.replace(true)) {
                reporter.report_panic(info);
                REPORTING.with(|r| r.set(false));
            }
            previous(info);
        }));
    });
    true
}

// ---------------------------------------------------------------------------
// The webview's errors
// ---------------------------------------------------------------------------

/// The event for an uncaught webview error. The name is kept only when it
/// looks like an error class name; the message and stack go through the
/// scrubber like everything else.
fn webview_event(report: &WebviewErrorReport) -> Event<'static> {
    let name = clean_error_name(&report.name);
    let mechanism = if report.kind == "unhandledrejection" {
        "onunhandledrejection"
    } else {
        "onerror"
    };
    let frames = report.stack.as_deref().map(js_frames).unwrap_or_default();
    let mut event = Event {
        level: Level::Error,
        platform: "javascript".into(),
        exception: vec![Exception {
            ty: name,
            // Cut by the scrubber, after it has read the whole message.
            value: Some(report.message.clone()),
            mechanism: Some(Mechanism {
                ty: mechanism.into(),
                handled: Some(false),
                ..Default::default()
            }),
            stacktrace: (!frames.is_empty()).then_some(Stacktrace {
                frames,
                ..Default::default()
            }),
            ..Default::default()
        }]
        .into(),
        ..Default::default()
    };
    event.tags.insert("origin".into(), "webview".into());
    event
}

/// An error's `name` if it is an identifier (`TypeError`, `DOMException`),
/// else `Error`: a thrown string or object can put anything there.
fn clean_error_name(name: &str) -> String {
    static NAME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[A-Za-z_$][A-Za-z0-9_$.]{0,63}$").expect("regex"));
    if NAME.is_match(name) {
        name.to_string()
    } else {
        "Error".to_string()
    }
}

/// The frames of a JavaScript stack, oldest first as Sentry wants them.
/// Reads Chromium's `    at fn (url:line:col)` and `    at url:line:col`, and
/// WebKit's and Firefox's `fn@url:line:col`; a line that is neither (the
/// first line of a Chromium stack repeats the message) is left out.
fn js_frames(stack: &str) -> Vec<Frame> {
    let mut frames: Vec<Frame> = stack
        .lines()
        .filter_map(js_frame)
        .take(MAX_JS_FRAMES)
        .collect();
    frames.reverse();
    frames
}

fn js_frame(line: &str) -> Option<Frame> {
    let line = line.trim();
    let (function, location) = if let Some(rest) = line.strip_prefix("at ") {
        match rest.strip_suffix(')').and_then(|r| r.rsplit_once(" (")) {
            Some((function, location)) => (Some(function), location),
            None => (None, rest),
        }
    } else if let Some((function, location)) = line.split_once('@') {
        ((!function.is_empty()).then_some(function), location)
    } else {
        return None;
    };
    let (rest, col) = location.rsplit_once(':')?;
    let (file, lineno) = rest.rsplit_once(':')?;
    let lineno = lineno.parse::<u64>().ok()?;
    let colno = col.parse::<u64>().ok()?;
    Some(Frame {
        function: function.map(|f| truncate(f, 200).into_owned()),
        abs_path: Some(truncate(file, 300).into_owned()),
        lineno: Some(lineno),
        colno: Some(colno),
        in_app: Some(
            file.starts_with("tauri://")
                || file.contains("tauri.localhost")
                || file.contains("://localhost"),
        ),
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// The scrubber
// ---------------------------------------------------------------------------

/// Folders to take out of every report, with what goes in their place.
#[derive(Default)]
pub struct Redactions {
    prefixes: RwLock<Vec<(String, &'static str)>>,
}

impl Redactions {
    /// Take `path` out of every report from now on, as each spelling a
    /// report could carry: as given, canonical, and on Windows with either
    /// slash and without the `\\?\` prefix.
    pub fn add(&self, path: &Path, replacement: &'static str) {
        let mut spellings = vec![path.to_string_lossy().into_owned()];
        if let Ok(canonical) = std::fs::canonicalize(path) {
            spellings.push(canonical.to_string_lossy().into_owned());
        }
        let mut more = Vec::new();
        for s in &spellings {
            let s = s.strip_prefix(r"\\?\").unwrap_or(s);
            more.push(s.to_string());
            if s.contains('\\') {
                more.push(s.replace('\\', "/"));
            }
        }
        let mut prefixes = self.prefixes.write().unwrap_or_else(|p| p.into_inner());
        for s in more {
            let s = s.trim_end_matches(['/', '\\']).to_string();
            // The root, or a drive, is not a folder to remove: it would turn
            // every path into the replacement.
            if s.len() < 3 || prefixes.iter().any(|(p, _)| *p == s) {
                continue;
            }
            prefixes.push((s, replacement));
        }
        // Longest first, so a project inside the home folder is named a
        // project, not `~/…`.
        prefixes.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
    }

    fn apply(&self, text: &str) -> String {
        let prefixes = self.prefixes.read().unwrap_or_else(|p| p.into_inner());
        let mut out = text.to_string();
        for (prefix, replacement) in prefixes.iter() {
            if out.contains(prefix.as_str()) {
                out = out.replace(prefix.as_str(), replacement);
            }
        }
        out
    }
}

/// The fields whose values are the SDK's own and must stay as they are for
/// the event to be read back (an id, a time, a level, a context's `type`).
const KEEP: &[&str] = &[
    "event_id",
    "timestamp",
    "level",
    "platform",
    "release",
    "environment",
    "type",
    "sdk",
    "instruction_addr",
    "image_addr",
    "symbol_addr",
];

/// The fields of a stack frame that name code: their paths are scrubbed of
/// the home and project folders, but the rest is kept, because it is where
/// in edytlab's code the error happened.
const CODE: &[&str] = &[
    "abs_path",
    "filename",
    "module",
    "package",
    "function",
    "raw_function",
    "symbol",
];

/// The client's `before_send`: what may leave the computer.
///
/// Removed whole: the user, the request, the host name, breadcrumbs,
/// `extra`, the module list, debug images, and each frame's local variables
/// and source lines. Every other string is scrubbed by [`scrub_text`]. The
/// event is then read back; one that cannot be is dropped, never sent as it
/// was.
pub fn scrub_event(mut event: Event<'static>, redactions: &Redactions) -> Option<Event<'static>> {
    event.user = None;
    event.request = None;
    event.server_name = None;
    event.breadcrumbs = Default::default();
    event.extra = Default::default();
    event.modules = Default::default();
    event.debug_meta = Default::default();
    event.template = None;
    let clear_frames = |stacktrace: &mut Option<Stacktrace>| {
        if let Some(stacktrace) = stacktrace {
            stacktrace.registers.clear();
            for frame in &mut stacktrace.frames {
                frame.vars.clear();
                frame.pre_context.clear();
                frame.context_line = None;
                frame.post_context.clear();
            }
        }
    };
    for exception in &mut event.exception.values {
        clear_frames(&mut exception.stacktrace);
        clear_frames(&mut exception.raw_stacktrace);
    }
    for thread in &mut event.threads.values {
        clear_frames(&mut thread.stacktrace);
        clear_frames(&mut thread.raw_stacktrace);
    }
    clear_frames(&mut event.stacktrace);

    let mut value = serde_json::to_value(&event).ok()?;
    scrub_value(&mut value, None, redactions);
    serde_json::from_value(value).ok()
}

fn scrub_value(value: &mut serde_json::Value, key: Option<&str>, redactions: &Redactions) {
    use serde_json::Value;
    if key.is_some_and(|k| KEEP.contains(&k)) {
        return;
    }
    match value {
        Value::String(s) => {
            let code = key.is_some_and(|k| CODE.contains(&k));
            *s = scrub_text(s, code, redactions);
        }
        Value::Array(items) => {
            for item in items {
                scrub_value(item, key, redactions);
            }
        }
        Value::Object(fields) => {
            for (k, v) in fields.iter_mut() {
                scrub_value(v, Some(k.as_str()), redactions);
            }
        }
        _ => {}
    }
}

macro_rules! regex {
    ($re:expr) => {{
        static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new($re).expect("regex"));
        &*RE
    }};
}

/// One string of a report, scrubbed. `code` is a frame's path or function
/// name, which keeps its shape; anything else is free text.
///
/// In order:
/// 1. cut to [`MAX_SCRUB_INPUT`] characters, which only bounds the work;
/// 2. (free text) anything in double, single or typographic quotes becomes
///    `"<redacted>"`: a `{:?}` of a string, a JSON value, a quoted file
///    name, chat text, tool arguments and results. A quotation may span
///    lines, and one that is never closed (a message cut short somewhere
///    before it got here) runs to the end of the text;
/// 3. keys and tokens become `<redacted>`: Anthropic's, OpenAI's and
///    OpenRouter's (`sk` and a dash), the underscore kinds (`sk`, `pk`, `rk`
///    and Groq's `gsk`), Gemini's `AIza…`, Hugging Face's `hf_…`, GitHub's
///    `gh?_…`, JWTs (`eyJ…`), `Bearer …`,
///    and the value of any `key=`, `token:`, `secret=`, `password=` or
///    `authorization:` (and `?code=`, `&state=`… in a URL);
/// 4. email addresses become `<email>`;
/// 5. the project folders and the home folder registered with
///    [`Redactions`] become `<project>` and `~`;
/// 6. (free text) any absolute path left becomes `<path>`, to the end of the
///    path (spaces included);
/// 7. (free text) any other run of 32 or more letters and digits with both
///    in it (an unknown token, a content hash) becomes `<redacted>`;
/// 8. any other user folder left (`/Users/<name>`, `/home/<name>`,
///    `C:\Users\<name>`, in a frame's path) becomes `<user>`;
/// 9. cut to [`MAX_TEXT`] characters. Last, so a quotation whose close lies
///    past the cut is still found whole.
pub fn scrub_text(text: &str, code: bool, redactions: &Redactions) -> String {
    let mut out = truncate(text, MAX_SCRUB_INPUT).into_owned();

    if !code {
        // `$` is the end of the text: a quotation never closed runs to it.
        out = regex!(r#"(?s)"(?:[^"\\]|\\.)*(?:"|\\?$)"#)
            .replace_all(&out, "\"<redacted>\"")
            .into_owned();
        out = regex!(r"“[^”]*(?:”|$)")
            .replace_all(&out, "“<redacted>”")
            .into_owned();
        out = regex!(r"‘[^’]*(?:’|$)")
            .replace_all(&out, "‘<redacted>’")
            .into_owned();
        // A single quote that opens after a letter is an apostrophe ("can't"),
        // not a quotation.
        out = regex!(r"(^|[^\p{L}\p{N}])'[^']*(?:'|$)")
            .replace_all(&out, "$1'<redacted>'")
            .into_owned();
    }

    for re in [
        // Anthropic's (the dashes escaped so the source holds no key prefix).
        regex!(r"sk\-ant\-[A-Za-z0-9_\-]+"),
        regex!(r"\bsk-[A-Za-z0-9_\-]{8,}"),
        regex!(r"\b[sprk]k_[A-Za-z0-9_\-]{8,}"),
        regex!(r"\bgs[k]_[A-Za-z0-9]{8,}"),
        regex!(r"\bAIza[0-9A-Za-z_\-]{20,}"),
        regex!(r"\bhf_[A-Za-z0-9]{20,}"),
        regex!(r"\bgh[pousr]_[A-Za-z0-9]{20,}"),
        regex!(r"\beyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+(?:\.[A-Za-z0-9_\-]*)?"),
    ] {
        out = re.replace_all(&out, "<redacted>").into_owned();
    }
    out = regex!(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=\-]+")
        .replace_all(&out, "$1 <redacted>")
        .into_owned();
    out = regex!(
        r"(?i)\b((?:x-)?api[_-]?key|access[_-]?token|refresh[_-]?token|id[_-]?token|token|secret|client[_-]?secret|password|passwd|authorization)(\s*[:=]\s*)[^\s,;&]+"
    )
    .replace_all(&out, "$1$2<redacted>")
    .into_owned();
    out = regex!(
        r"(?i)([?&](?:code|state|key|token|access_token|refresh_token|sig|signature)=)[^&\s#]+"
    )
    .replace_all(&out, "$1<redacted>")
    .into_owned();

    out = regex!(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)*\.[A-Za-z]{2,}")
        .replace_all(&out, "<email>")
        .into_owned();

    out = redactions.apply(&out);

    if !code {
        // From a path's start to its end: a drive (`C:\`, `D:/`), `~/`, a
        // UNC share or the user folders above, and a root folder other code
        // keeps files under. Spaces are part of a path here (a folder named
        // "My Show"), so the path runs to a quote, a bracket, a colon, a
        // comma, a semicolon or the end of the line.
        out = regex!(
            r#"(?:\b[A-Za-z]:[\\/]|~[\\/]|\\\\|/(?:Users|home|Volumes|mnt|media|tmp|private|var|run|opt|srv|data|root|Library|Applications)\b)[^\n"'<>|:,;()\[\]{}]*"#
        )
        .replace_all(&out, |caps: &Captures| {
            let found = &caps[0];
            format!("<path>{}", &found[path_end(found)..])
        })
        .into_owned();

        out = regex!(r"[A-Za-z0-9_\-]{32,}")
            .replace_all(&out, |caps: &Captures| {
                let token = &caps[0];
                let letters = token.chars().any(|c| c.is_ascii_alphabetic());
                let digits = token.chars().any(|c| c.is_ascii_digit());
                if letters && digits {
                    "<redacted>".to_string()
                } else {
                    token.to_string()
                }
            })
            .into_owned();
    }

    // What is left of a user folder (in a frame's path, which keeps its
    // shape) loses the user's name.
    out = regex!(
        r"(?i)(/Users/|/home/|\b[A-Z]:[\\/]+Users[\\/]+|\\\\[^\\/\s]+\\Users\\)[^/\\\s:<>|]+"
    )
    .replace_all(&out, |caps: &Captures| format!("{}<user>", &caps[1]))
    .into_owned();
    truncate(&out, MAX_TEXT).into_owned()
}

/// Where the path at the start of `found` ends. A path may hold spaces, so
/// its last part ends after its last extension (`take 1.wav` in
/// `~/take 1.wav failed`) when it has one; without one the whole run is the
/// path (but for trailing space), since a cut too long hides words of the
/// message and a cut too short leaks part of a name.
fn path_end(found: &str) -> usize {
    let last = found.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let extension = regex!(r"\.[A-Za-z0-9]{1,8}(?:\s|$)");
    // The last extension, so `v1.2 final take.wav` is one name.
    match extension.find_iter(&found[last..]).last() {
        Some(m) => last + m.start() + m.as_str().trim_end().len(),
        None => found.trim_end().len(),
    }
}

/// `text` cut to `max` characters, on a character boundary.
fn truncate(text: &str, max: usize) -> Cow<'_, str> {
    match text.char_indices().nth(max) {
        Some((end, _)) => Cow::Owned(format!("{}…", &text[..end])),
        None => Cow::Borrowed(text),
    }
}

// ---------------------------------------------------------------------------
// The transport
// ---------------------------------------------------------------------------

/// Makes an [`HttpTransport`] for the client's DSN. Called only when a
/// client is built, which is only when the person has turned reporting on.
struct HttpTransportFactory;

impl TransportFactory for HttpTransportFactory {
    fn create_transport_with_options(&self, options: TransportOptions) -> Arc<dyn Transport> {
        Arc::new(HttpTransport::new(&options.dsn))
    }
}

enum Job {
    Send(Box<Envelope>),
    Flush(SyncSender<()>),
}

/// Sends envelopes to the DSN's envelope endpoint on a thread of its own,
/// with the `reqwest` blocking client the app already uses (rustls, the
/// system proxy settings). The queue is bounded: a burst past [`QUEUE`] is
/// dropped. A 429 stops sending for as long as `Retry-After` says (a minute
/// if it says nothing).
pub struct HttpTransport {
    sender: Mutex<Option<SyncSender<Job>>>,
}

impl HttpTransport {
    fn new(dsn: &Dsn) -> Self {
        let (sender, receiver) = mpsc::sync_channel(QUEUE);
        let url = dsn.envelope_api_url().to_string();
        let user_agent = format!("edytlab/{}", env!("CARGO_PKG_VERSION"));
        let auth = dsn.to_auth(Some(&user_agent)).to_string();
        let spawned = std::thread::Builder::new()
            .name("crash-reports".into())
            .spawn(move || send_loop(receiver, url, auth, user_agent));
        if spawned.is_err() {
            tracing::warn!("could not start the crash report thread; reports will not be sent");
        }
        Self {
            sender: Mutex::new(spawned.ok().map(|_| sender)),
        }
    }
}

fn send_loop(receiver: Receiver<Job>, url: String, auth: String, user_agent: String) {
    // Built here, on this thread: a blocking client must not be made or
    // dropped on an async runtime's thread.
    let client = match reqwest::blocking::Client::builder()
        .user_agent(user_agent)
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            tracing::warn!(error = %e, "could not build the crash report client");
            return;
        }
    };
    let mut paused_until: Option<Instant> = None;
    for job in receiver {
        match job {
            Job::Flush(done) => {
                let _ = done.send(());
            }
            Job::Send(envelope) => {
                if paused_until.is_some_and(|until| Instant::now() < until) {
                    continue;
                }
                let mut body = Vec::new();
                if envelope.to_writer(&mut body).is_err() {
                    continue;
                }
                let sent = client
                    .post(&url)
                    .header("X-Sentry-Auth", &auth)
                    .header("Content-Type", "application/x-sentry-envelope")
                    .body(body)
                    .send();
                match sent {
                    Ok(response) if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                        let seconds = response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.trim().parse::<u64>().ok())
                            .unwrap_or(60);
                        paused_until = Some(Instant::now() + Duration::from_secs(seconds));
                    }
                    Ok(_) => {}
                    // Offline, a proxy refusing: the report is lost, and that
                    // is all. It is never retried or written to disk.
                    Err(e) => tracing::debug!(error = %e, "a crash report was not sent"),
                }
            }
        }
    }
}

impl Transport for HttpTransport {
    fn send_envelope(&self, envelope: Envelope) {
        let sender = self.sender.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(sender) = sender.as_ref() {
            match sender.try_send(Job::Send(Box::new(envelope))) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => {}
                Err(TrySendError::Full(_)) => {
                    tracing::debug!("crash report queue full; report dropped")
                }
            }
        }
    }

    fn flush(&self, timeout: Duration) -> bool {
        let (done, wait) = mpsc::sync_channel(1);
        {
            let sender = self.sender.lock().unwrap_or_else(|p| p.into_inner());
            let Some(sender) = sender.as_ref() else {
                return true;
            };
            if sender.try_send(Job::Flush(done)).is_err() {
                return false;
            }
        }
        wait.recv_timeout(timeout).is_ok()
    }

    fn shutdown(&self, timeout: Duration) -> bool {
        let flushed = self.flush(timeout);
        // Dropping the sender ends the thread once its queue is empty.
        self.sender.lock().unwrap_or_else(|p| p.into_inner()).take();
        flushed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;

    /// A transport that keeps what it is given, and a count of how many
    /// times a client asked for one.
    #[derive(Default)]
    struct Recorder {
        made: AtomicUsize,
        envelopes: Mutex<Vec<Envelope>>,
    }

    struct Recording(Arc<Recorder>);

    impl Transport for Recording {
        fn send_envelope(&self, envelope: Envelope) {
            self.0.envelopes.lock().unwrap().push(envelope);
        }
    }

    struct RecordingFactory(Arc<Recorder>);

    impl TransportFactory for RecordingFactory {
        fn create_transport_with_options(&self, _: TransportOptions) -> Arc<dyn Transport> {
            self.0.made.fetch_add(1, Ordering::SeqCst);
            Arc::new(Recording(Arc::clone(&self.0)))
        }
    }

    fn test_dsn() -> Dsn {
        "https://0123456789abcdef0123456789abcdef@reports.example.invalid/0"
            .parse()
            .unwrap()
    }

    fn recording(dsn: Option<Dsn>) -> (CrashReporter, Arc<Recorder>) {
        let recorder = Arc::new(Recorder::default());
        let reporter =
            CrashReporter::with_transport(dsn, Arc::new(RecordingFactory(Arc::clone(&recorder))));
        (reporter, recorder)
    }

    fn sent_events(recorder: &Recorder) -> Vec<Event<'static>> {
        recorder
            .envelopes
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| e.event().cloned())
            .collect()
    }

    fn error_report(message: &str) -> WebviewErrorReport {
        WebviewErrorReport {
            kind: "error".into(),
            name: "TypeError".into(),
            message: message.into(),
            stack: Some(
                "TypeError: x is undefined\n    at render (tauri://localhost/assets/index-abc.js:10:20)\n    at tauri://localhost/assets/index-abc.js:3:4"
                    .into(),
            ),
        }
    }

    // --- Off unless both the build and the person say so ----------------

    #[test]
    fn a_new_reporter_is_off_and_builds_nothing() {
        let (reporter, recorder) = recording(Some(test_dsn()));
        assert_eq!(
            reporter.status(),
            CrashReportsStatus {
                enabled: false,
                available: true
            }
        );
        assert!(!reporter.active());
        assert!(!reporter.report_webview_error(&error_report("boom")));
        assert!(!reporter.capture(Event::default()));
        assert_eq!(
            recorder.made.load(Ordering::SeqCst),
            0,
            "no transport while off"
        );
        assert!(sent_events(&recorder).is_empty());
    }

    #[test]
    fn with_no_dsn_turning_it_on_builds_nothing_and_sends_nothing() {
        let (reporter, recorder) = recording(None);
        reporter.set_enabled(true);
        assert_eq!(
            reporter.status(),
            CrashReportsStatus {
                enabled: true,
                available: false
            }
        );
        assert!(!reporter.active());
        assert!(!reporter.report_webview_error(&error_report("boom")));
        assert_eq!(recorder.made.load(Ordering::SeqCst), 0);
        assert!(sent_events(&recorder).is_empty());
    }

    /// A build's DSN is read from `EDYTLAB_SENTRY_DSN` at compile time and
    /// is absent from every build that was not given one: an empty value or
    /// one that is not a DSN is no DSN.
    #[test]
    fn only_a_real_dsn_counts() {
        assert!(parse_dsn(None).is_none());
        assert!(parse_dsn(Some("")).is_none());
        assert!(parse_dsn(Some("   ")).is_none());
        assert!(parse_dsn(Some("not a dsn")).is_none());
        assert!(parse_dsn(Some("https://key@reports.example.invalid/0")).is_some());
    }

    #[test]
    fn on_it_sends_off_it_stops_and_on_again_it_sends_again() {
        let (reporter, recorder) = recording(Some(test_dsn()));
        reporter.set_enabled(true);
        assert!(reporter.active());
        assert!(reporter.report_webview_error(&error_report("first")));
        assert_eq!(sent_events(&recorder).len(), 1);

        reporter.set_enabled(false);
        assert!(!reporter.active());
        assert!(!reporter.report_webview_error(&error_report("while off")));
        assert_eq!(sent_events(&recorder).len(), 1, "nothing is sent while off");

        reporter.set_enabled(true);
        assert!(reporter.report_webview_error(&error_report("second")));
        assert_eq!(sent_events(&recorder).len(), 2);
        assert_eq!(
            recorder.made.load(Ordering::SeqCst),
            2,
            "a new client each time it is turned on"
        );
    }

    // --- The real transport: no request unless on --------------------------

    /// A listener on this machine stands in for Sentry. With reporting off
    /// nothing connects to it; on, one POST arrives at the DSN's envelope
    /// endpoint, carrying the scrubbed report.
    #[test]
    fn the_http_transport_makes_no_request_while_off_and_one_when_on() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let dsn: Dsn = format!("http://0123456789abcdef0123456789abcdef@127.0.0.1:{port}/42")
            .parse()
            .unwrap();
        let reporter = CrashReporter::new(Some(dsn));

        assert!(!reporter.report_webview_error(&error_report("while off")));
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            listener.accept().is_err(),
            "a request reached the stand-in for Sentry while reporting was off"
        );

        reporter.set_enabled(true);
        // Put together here so the source holds no key prefix.
        let secret = concat!("sk", "-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789");
        assert!(reporter.report_webview_error(&error_report(&format!(
            "could not save /home/jane/My Show/take 1.wav for jane@example.com with {secret}"
        ))));

        listener.set_nonblocking(false).unwrap();
        let (mut stream, _) = listener.accept().expect("a request when on");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        assert!(
            request_line.starts_with("POST /api/42/envelope/"),
            "{request_line}"
        );
        let mut length = 0usize;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).unwrap();
            if header == "\r\n" || header.is_empty() {
                break;
            }
            if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        use std::io::Write as _;
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}");
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("TypeError"), "{body}");
        for leaked in ["jane", "My Show", "take 1.wav", "example.com", secret] {
            assert!(
                !body.contains(leaked),
                "{leaked} reached the request: {body}"
            );
        }
        reporter.set_enabled(false);
    }

    // --- What a report carries ---------------------------------------------

    #[test]
    fn a_webview_report_carries_its_type_frames_and_origin() {
        let (reporter, recorder) = recording(Some(test_dsn()));
        reporter.set_enabled(true);
        reporter.report_webview_error(&error_report("x is undefined"));
        let event = sent_events(&recorder).pop().unwrap();
        let exception = &event.exception.values[0];
        assert_eq!(exception.ty, "TypeError");
        assert_eq!(exception.value.as_deref(), Some("x is undefined"));
        assert_eq!(exception.mechanism.as_ref().unwrap().ty, "onerror");
        let frames = &exception.stacktrace.as_ref().unwrap().frames;
        assert_eq!(frames.len(), 2);
        // Oldest first.
        assert_eq!(frames[1].function.as_deref(), Some("render"));
        assert_eq!(frames[1].lineno, Some(10));
        assert_eq!(frames[1].colno, Some(20));
        assert_eq!(frames[1].in_app, Some(true));
        assert_eq!(
            event.tags.get("origin").map(String::as_str),
            Some("webview")
        );
        assert_eq!(
            event.release.as_deref(),
            Some(&*format!("edytlab@{}", env!("CARGO_PKG_VERSION")))
        );
        assert!(event.server_name.is_none(), "the host name is not sent");
        assert!(event.user.is_none());
    }

    #[test]
    fn webkit_and_firefox_stacks_are_read_too() {
        let frames = js_frames("render@tauri://localhost/assets/a.js:1:2\n@tauri://localhost/assets/a.js:3:4\nnot a frame");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].function, None);
        assert_eq!(frames[1].function.as_deref(), Some("render"));
        assert_eq!(
            frames[1].abs_path.as_deref(),
            Some("tauri://localhost/assets/a.js")
        );
    }

    #[test]
    fn a_name_that_is_not_an_identifier_is_sent_as_error() {
        assert_eq!(clean_error_name("TypeError"), "TypeError");
        assert_eq!(clean_error_name("DOMException"), "DOMException");
        assert_eq!(clean_error_name("my secret chat message"), "Error");
        assert_eq!(clean_error_name(""), "Error");
    }

    // --- The scrubber --------------------------------------------------------

    fn scrubbed(text: &str) -> String {
        let redactions = Redactions::default();
        redactions.add(Path::new("/home/jane"), "~");
        redactions.add(Path::new("/srv/audio/Interview with Sam"), "<project>");
        scrub_text(text, false, &redactions)
    }

    #[test]
    fn paths_become_placeholders() {
        assert_eq!(
            scrubbed("could not open /srv/audio/Interview with Sam/take.wav"),
            "could not open <project>/take.wav"
        );
        assert_eq!(
            scrubbed("read /home/jane/Music/x.wav: denied"),
            "read <path>: denied"
        );
        assert_eq!(scrubbed("at /Users/sam/Desktop/a b.wav"), "at <path>");
        assert_eq!(scrubbed(r"at C:\Users\Sam\Music\a b.wav"), "at <path>");
        assert_eq!(
            scrubbed("in /Volumes/Show Drive/ep1.wav, stop"),
            "in <path>, stop"
        );
        assert_eq!(scrubbed("open ~/edits/x.wav"), "open <path>");
        assert_eq!(
            scrubbed("/Users/sam/My Show/v1.2 final take.wav failed"),
            "<path> failed"
        );
        let out = scrubbed("/home/jane/x and /home/jane/y");
        assert!(!out.contains("jane"), "{out}");
    }

    #[test]
    fn quoted_text_is_removed() {
        // How chat text, tool arguments and results reach a message.
        assert_eq!(
            scrubbed(
                r#"called `Result::unwrap()` on an `Err` value: Parse("make the intro louder")"#
            ),
            r#"called `Result::unwrap()` on an `Err` value: Parse("<redacted>")"#
        );
        assert_eq!(
            scrubbed(r#"bad tool input {"track":"Vocals","gain_db":3}"#),
            r#"bad tool input {"<redacted>":"<redacted>","<redacted>":3}"#
        );
        assert_eq!(scrubbed("reading 'secretProp'"), "reading '<redacted>'");
        assert_eq!(scrubbed("“private words”"), "“<redacted>”");
        // An apostrophe is not a quote.
        assert_eq!(
            scrubbed("can't decode, don't retry"),
            "can't decode, don't retry"
        );
    }

    #[test]
    fn a_quotation_longer_than_the_cut_is_removed_whole() {
        // The close lies past MAX_TEXT: scrubbing before the cut would leave
        // an open quote the rule cannot match, and the words in it.
        let words = "make the intro louder ".repeat(100);
        let out = scrubbed(&format!(r#"bad input Parse("{words}") here"#));
        assert!(!out.contains("intro"), "{out}");
        assert!(out.starts_with(r#"bad input Parse("<redacted>")"#), "{out}");
    }

    #[test]
    fn a_quotation_that_spans_lines_or_never_closes_is_removed() {
        let out = scrubbed("value: \"first line\nsecond line\" end");
        assert!(!out.contains("line"), "{out}");
        assert!(out.ends_with(" end"), "{out}");
        // Cut short before it reached the scrubber (the window cuts at 4,000).
        for text in [
            "value: \"private words that never clo",
            "value: “private words that never clo",
            "value: ‘private words that never clo",
            "value: 'private words that never clo",
            "value: \"private words ending in a backslash \\",
        ] {
            let out = scrubbed(text);
            assert!(!out.contains("private"), "{text} -> {out}");
        }
    }

    #[test]
    fn a_scrubbed_string_is_cut_after_scrubbing() {
        let out = scrubbed(&"a".repeat(MAX_TEXT * 3));
        assert_eq!(
            out.chars().count(),
            MAX_TEXT + 1,
            "the cut and its ellipsis"
        );
        assert!(out.ends_with('…'));
    }

    #[test]
    fn keys_and_tokens_are_removed() {
        for (text, gone) in [
            (concat!("key s", "k-ant-api03-AbCdEf_123-xyz"), "ant-api03"),
            ("key sk-proj-abcdefghijklmnop1234", "sk-proj"),
            ("key sk-or-v1-0123456789abcdef", "sk-or"),
            (concat!("stripe s", "k_live_abcdefgh1234"), "live_abcdefgh"),
            (concat!("groq gs", "k_abcdefgh12345678"), "abcdefgh12345678"),
            ("gemini AIzaSyA1234567890abcdefghijklmnop", "AIza"),
            ("hf hf_abcdefghijklmnopqrstuvwxyz", "hf_"),
            ("gh ghp_abcdefghijklmnopqrstuvwxyz0123", "ghp_"),
            ("jwt eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln", "eyJ"),
            (concat!("Authorization: Bea", "rer abc.def-ghi"), "abc.def"),
            ("refresh_token=rt_abcdef123", "rt_abcdef"),
            ("api_key: plainvalue123", "plainvalue"),
            ("https://auth.openai.com/cb?code=xyz123&state=abc", "xyz123"),
            ("hash 3f8a9c0b1d2e4f5a6b7c8d9e0f1a2b3c4d5e6f7a", "3f8a9c0b"),
        ] {
            let out = scrubbed(text);
            assert!(!out.contains(gone), "{text} -> {out}");
            assert!(out.contains("<redacted>"), "{text} -> {out}");
        }
        // Long words are not tokens.
        let words = "the_agent_built_at_launch_holds_edits_when_plan_first_was_left_on";
        assert_eq!(scrubbed(words), words);
    }

    #[test]
    fn emails_are_removed() {
        assert_eq!(
            scrubbed("signed in as jane.doe+edits@example.co.uk"),
            "signed in as <email>"
        );
    }

    #[test]
    fn frames_keep_their_shape_without_the_home_folder() {
        let redactions = Redactions::default();
        redactions.add(Path::new("/home/jane"), "~");
        assert_eq!(
            scrub_text("/home/jane/edytlab/crates/ai/src/lib.rs", true, &redactions),
            "~/edytlab/crates/ai/src/lib.rs"
        );
        assert_eq!(
            scrub_text(
                "/home/runner/.cargo/registry/src/tokio-1.0/src/lib.rs",
                true,
                &redactions
            ),
            "/home/<user>/.cargo/registry/src/tokio-1.0/src/lib.rs"
        );
        assert_eq!(
            scrub_text("tools::tool::trim::TrimTool::run", true, &redactions),
            "tools::tool::trim::TrimTool::run"
        );
    }

    #[test]
    fn a_project_inside_the_home_folder_is_named_a_project() {
        let redactions = Redactions::default();
        redactions.add(Path::new("/home/jane"), "~");
        redactions.add(Path::new("/home/jane/Shows/Ep 1"), "<project>");
        assert_eq!(
            scrub_text("/home/jane/Shows/Ep 1/.audiograph/x.wav", true, &redactions),
            "<project>/.audiograph/x.wav"
        );
    }

    /// The whole event: what is removed outright, what is scrubbed, and
    /// what the SDK needs is left so it can be read back and sent.
    #[test]
    fn the_scrubber_clears_an_event_of_the_person() {
        use sentry::protocol::{Breadcrumb, Request, User};

        let redactions = Redactions::default();
        redactions.add(Path::new("/home/jane"), "~");
        let mut event = Event {
            message: Some("failed for jane@example.com at /home/jane/a.wav".into()),
            server_name: Some("janes-laptop".into()),
            user: Some(User {
                email: Some("jane@example.com".into()),
                ..Default::default()
            }),
            request: Some(Request::default()),
            breadcrumbs: vec![Breadcrumb {
                message: Some("user typed: make it louder".into()),
                ..Default::default()
            }]
            .into(),
            exception: vec![Exception {
                ty: "panic".into(),
                value: Some(r#"tool failed: "trim the silence after Jane says hi""#.into()),
                stacktrace: Some(Stacktrace {
                    frames: vec![Frame {
                        function: Some("tools::tool::trim::run".into()),
                        abs_path: Some("/home/jane/edytlab/crates/tools/src/tool/trim.rs".into()),
                        context_line: Some("let secret = \"hunter2\";".into()),
                        vars: [("chat".to_string(), "make it louder".into())]
                            .into_iter()
                            .collect(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            }]
            .into(),
            ..Default::default()
        };
        event
            .extra
            .insert("tool_args".into(), "{\"track\":1}".into());
        event.tags.insert(
            "note".into(),
            concat!("s", "k-ant-api03-zzzzzzzzzzzz").into(),
        );
        let id = event.event_id;

        let out = scrub_event(event, &redactions).expect("an event that reads back");
        assert_eq!(out.event_id, id);
        assert!(out.user.is_none());
        assert!(out.request.is_none());
        assert!(out.server_name.is_none());
        assert!(out.breadcrumbs.values.is_empty());
        assert!(out.extra.is_empty());
        assert_eq!(out.message.as_deref(), Some("failed for <email> at <path>"));
        let exception = &out.exception.values[0];
        assert_eq!(exception.ty, "panic");
        assert_eq!(
            exception.value.as_deref(),
            Some(r#"tool failed: "<redacted>""#)
        );
        let frame = &exception.stacktrace.as_ref().unwrap().frames[0];
        assert_eq!(frame.function.as_deref(), Some("tools::tool::trim::run"));
        assert_eq!(
            frame.abs_path.as_deref(),
            Some("~/edytlab/crates/tools/src/tool/trim.rs")
        );
        assert!(frame.context_line.is_none());
        assert!(frame.vars.is_empty());
        assert_eq!(out.tags.get("note").map(String::as_str), Some("<redacted>"));

        let text = serde_json::to_string(&out).unwrap();
        for leaked in [
            "jane",
            "Jane",
            "janes-laptop",
            "hunter2",
            "louder",
            "ant-api03",
            "example.com",
        ] {
            assert!(!text.contains(leaked), "{leaked} left in {text}");
        }
    }

    /// Everything a report travels with is scrubbed before it is handed to
    /// the transport: the client's `before_send` is the scrubber.
    #[test]
    fn the_client_sends_only_what_the_scrubber_returns() {
        let (reporter, recorder) = recording(Some(test_dsn()));
        reporter.redact_path(Path::new("/home/jane"), "~");
        reporter.set_enabled(true);
        reporter.capture(Event {
            message: Some("open /home/jane/x.wav failed for jane@example.com".into()),
            ..Default::default()
        });
        let event = sent_events(&recorder).pop().unwrap();
        assert_eq!(
            event.message.as_deref(),
            Some("open <path> failed for <email>")
        );
        assert!(event.server_name.is_none());
        // The contexts are there (OS, processor, Rust), with nothing of the
        // person's in them.
        assert!(event.contexts.contains_key("os"), "{:?}", event.contexts);
    }

    /// A panic is reported once the hook is installed and reporting is on,
    /// scrubbed like everything else, and the hook that was there before
    /// still runs.
    #[test]
    fn a_panic_is_reported_through_the_hook() {
        let (reporter, recorder) = recording(Some(test_dsn()));
        let reporter = Arc::new(reporter);
        assert!(install_panic_hook(Arc::clone(&reporter)));
        reporter.set_enabled(true);

        let marker = "panic-marker-958";
        let joined = std::thread::spawn(move || {
            panic!("{marker} while reading /home/jane/secret.wav");
        })
        .join();
        assert!(joined.is_err());

        let events = sent_events(&recorder);
        let event = events
            .iter()
            .find(|e| {
                e.exception
                    .values
                    .first()
                    .and_then(|x| x.value.as_deref())
                    .is_some_and(|v| v.contains(marker))
            })
            .expect("the panic was reported");
        let exception = &event.exception.values[0];
        assert_eq!(exception.ty, "panic");
        assert_eq!(
            exception.value.as_deref(),
            Some(&*format!("{marker} while reading <path>"))
        );
        assert_eq!(event.level, Level::Fatal);
        assert_eq!(event.tags.get("origin").map(String::as_str), Some("rust"));
        reporter.set_enabled(false);
    }

    #[test]
    fn no_hook_is_installed_for_a_build_without_a_dsn() {
        assert!(!install_panic_hook(Arc::new(CrashReporter::new(None))));
    }

    /// The release workflows hand the build its DSN from the repository's
    /// variable or secret, on the step that builds; nothing else does, and
    /// no DSN is written in the repository.
    #[test]
    fn the_release_workflows_pass_the_dsn_to_the_build() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        for workflow in ["release-dev.yml", "release-signed.yml"] {
            let yaml =
                std::fs::read_to_string(root.join(".github/workflows").join(workflow)).unwrap();
            let line =
                "EDYTLAB_SENTRY_DSN: ${{ vars.EDYTLAB_SENTRY_DSN || secrets.EDYTLAB_SENTRY_DSN }}";
            assert_eq!(
                yaml.matches(line).count(),
                1,
                "{workflow} passes the DSN once"
            );
            let build = yaml.find("uses: tauri-apps/tauri-action").unwrap();
            let at = yaml.find(line).unwrap();
            // On the build step: after it starts, before the next step.
            let next_step = yaml[build..]
                .find("\n      - name:")
                .map(|i| build + i)
                .unwrap();
            assert!(
                at > build && at < next_step,
                "{workflow}: the DSN is not on the build step"
            );
            assert!(!yaml.contains("ingest.sentry.io"), "{workflow} holds a DSN");
        }
    }
}
