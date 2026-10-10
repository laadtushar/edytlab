//! No command that waits on a lock a running tool holds runs on the main
//! thread (#421).
//!
//! Tauri 2 runs a command declared without `async` on the main thread,
//! and a running agent tool can hold the store, engine, dispatcher and
//! clipboard locks for the whole of its call. A main-thread command that
//! waits on one of them freezes the window for as long as the tool runs:
//! no repaint, no input, no transport. A time-stretch of a 32 s track
//! froze it for about ten seconds.
//!
//! The lock can be reached through a helper as easily as directly:
//! twelve synchronous commands — the mixer's faders and mutes, renames,
//! clip moves — took it inside `run_track_tool`, so a scan of command
//! bodies alone passed while a fader click during a long tool still froze
//! the window. The scan therefore follows calls into the crate's free
//! functions and its `impl` methods, as far as they go.
//!
//! It matches on what holds a lock, not on how a lock is spelt. A body
//! counts when it names the `engine`, `dispatcher` or `clipboard` field
//! of `AppState` (`state.engine.lock()`, `lock_std(&state.engine, ..)`,
//! `let e = &state.engine`, `Arc::clone(&state.dispatcher)` all do), or
//! reaches the session store (`store_handle()`, `lock_std(&store ..)`), or
//! calls a function or method that does. What it does not see: a function
//! reached through a name it does not match (a call through a closure or a
//! trait object), `AppState` taken apart by a pattern
//! (`let AppState { engine, .. } = ..`), and any `.rs` file below
//! `src/` rather than directly in it. Functions are matched by name, not
//! by type, so a lock-taking method and an unrelated method of the same
//! name are the same to it; that errs towards a false alarm.
//!
//! The source is scanned rather than the behaviour exercised because the
//! mock runtime does not model Tauri's threads.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// The `AppState` fields whose lock a running agent tool can hold for the
/// whole of its call: the engine, the dispatcher and the clipboard. A body
/// that names one either locks it or takes a handle to it that the caller
/// locks next, so the field is matched wherever it appears rather than in
/// any one spelling of the lock.
///
/// That includes `clipboard_handle()`, which only clones the `Arc` and
/// takes no lock itself. It is counted, and so are its callers, because
/// the handle is handed out to be locked: this errs on the safe side.
const TURN_LOCK_FIELDS: [&str; 3] = ["engine", "dispatcher", "clipboard"];

/// How the session store is reached. It has no field to match:
/// `AppState::store` holds only the outer `Option` that is swapped when a
/// project opens, and the lock a tool holds is the inner `Arc<Mutex<Store>>`
/// inside it. `store_handle()` takes the outer lock for an instant and
/// returns that inner handle, which the caller then locks; the other two
/// are the locks themselves.
const STORE_LOCKS: [&str; 3] = ["store_handle()", "lock_std(&*store", "lock_std(&store"];

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether `body` names the field `field`: `.field` not continued into a
/// longer name and not a method call, so `state.engine` and
/// `&state.engine.lock()` count, `audio_engine::` and `.engine_id` and
/// `.engine()` do not.
fn names_field(body: &str, field: &str) -> bool {
    let needle = format!(".{field}");
    let mut from = 0;
    while let Some(i) = body[from..].find(&needle) {
        let end = from + i + needle.len();
        let after = body[end..].chars().next();
        if !after.is_some_and(|c| is_ident_char(c) || c == '(') {
            return true;
        }
        from = end;
    }
    false
}

fn takes_a_turn_lock(body: &str) -> bool {
    STORE_LOCKS.iter().any(|t| body.contains(t))
        || TURN_LOCK_FIELDS.iter().any(|f| names_field(body, f))
}

/// How a function is called. A free function is `name(..)` or
/// `module::name(..)`; a method, or an associated function, is declared in
/// an `impl` block and called `x.name(..)` or `Type::name(..)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Free,
    Method,
}

/// A function, free or in an `impl`, as (kind, name).
type Callee = (Kind, String);

/// Whether `body` calls `name` the way a `kind` is called: `name(` not
/// preceded by an identifier character, which would make it another name,
/// and for a free function not by a `.`, which would make it a method.
/// A method is a `.name(` or `Type::name(`.
fn calls(body: &str, name: &str, kind: Kind) -> bool {
    let needle = format!("{name}(");
    let mut from = 0;
    while let Some(i) = body[from..].find(&needle) {
        let at = from + i;
        let before = &body[..at];
        let is_call = match kind {
            Kind::Free => {
                !before
                    .chars()
                    .next_back()
                    .is_some_and(|c| is_ident_char(c) || c == '.')
                    && !before.ends_with("fn ")
            }
            Kind::Method => before.ends_with('.') || before.ends_with("::"),
        };
        if is_call {
            return true;
        }
        from = at + needle.len();
    }
    false
}

/// A call in `body` to any of `set`, if there is one.
fn first_call<'a>(set: &'a BTreeSet<Callee>, body: &str) -> Option<&'a Callee> {
    set.iter().find(|(kind, name)| calls(body, name, *kind))
}

/// How a callee is written in a message.
fn spelt((kind, name): &Callee) -> String {
    match kind {
        Kind::Free => format!("{name}()"),
        Kind::Method => format!(".{name}()"),
    }
}

/// The source of every `.rs` file in `src`, with Windows line endings
/// normalised: the scans split on "\n".
fn sources() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("src") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path)
            .expect("read")
            .replace("\r\n", "\n");
        out.push((
            path.file_name().unwrap().to_string_lossy().into_owned(),
            src,
        ));
    }
    out
}

/// The name a line declares, if it declares a function: `fn name`, with
/// any of `pub`, `pub(crate)`, `const`, `async`, `unsafe` in front.
fn fn_name(line: &str) -> Option<String> {
    let mut rest = line.trim_start();
    if let Some(r) = rest.strip_prefix("pub ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("pub(") {
        rest = &r[r.find(')')? + 1..];
    }
    rest = rest.trim_start();
    while let Some(r) = ["const ", "async ", "unsafe "]
        .iter()
        .find_map(|q| rest.strip_prefix(q))
    {
        rest = r;
    }
    let name: String = rest
        .strip_prefix("fn ")?
        .chars()
        .take_while(|c| is_ident_char(*c))
        .collect();
    (!name.is_empty()).then_some(name)
}

struct Func {
    kind: Kind,
    name: String,
    body: String,
}

/// Every function declared in `src` that a command can call: those at
/// column 0 are free, those indented inside an `impl` block are methods.
/// Any other indented one is passed over: a helper nested in a function is
/// part of that function's body, and a function in a `mod tests` is
/// reached by no command. The body runs from the signature to the first
/// closing brace at the signature's own indentation, which is where rustfmt
/// puts it.
fn functions_in(src: &str) -> Vec<Func> {
    let mut out = Vec::new();
    let mut in_impl = false;
    let mut offset = 0;
    for line in src.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let text = line.trim_start();
        let indent = &line[..line.len() - text.len()];
        // The column-0 item the following lines are inside.
        if indent.is_empty() && text.starts_with(|c: char| c.is_alphabetic()) {
            in_impl = text.starts_with("impl ") || text.starts_with("impl<");
        }
        let Some(name) = fn_name(text) else { continue };
        let kind = match (indent.is_empty(), in_impl) {
            (true, _) => Kind::Free,
            (false, true) => Kind::Method,
            (false, false) => continue,
        };
        let signature = text.trim_end();
        let end =
            if signature.ends_with(';') || (signature.contains('{') && signature.ends_with('}')) {
                // A declaration without a body, or a one-line function.
                line.len()
            } else {
                let closer = format!("\n{indent}}}\n");
                let rest = &src[start..];
                rest.find(&closer)
                    .map(|i| i + closer.len() - 1)
                    .unwrap_or(rest.len())
            };
        out.push(Func {
            kind,
            name,
            body: src[start..start + end].to_string(),
        });
    }
    out
}

/// The functions and methods that take a turn lock, directly or through
/// another one: grown until nothing new joins.
fn lock_takers(sources: &[(String, String)]) -> BTreeSet<Callee> {
    let funcs: Vec<Func> = sources.iter().flat_map(|(_, s)| functions_in(s)).collect();
    let mut set: BTreeSet<Callee> = funcs
        .iter()
        .filter(|f| takes_a_turn_lock(&f.body))
        .map(|f| (f.kind, f.name.clone()))
        .collect();
    loop {
        let before = set.len();
        for f in &funcs {
            let key = (f.kind, f.name.clone());
            if !set.contains(&key) && first_call(&set, &f.body).is_some() {
                set.insert(key);
            }
        }
        if set.len() == before {
            return set;
        }
    }
}

/// Every `#[tauri::command]` in `sources`, as (file, name, attribute,
/// is_async_fn, body).
fn commands_in(sources: &[(String, String)]) -> Vec<(String, String, String, bool, String)> {
    let mut out = Vec::new();
    for (file, src) in sources {
        let mut rest = src.as_str();
        while let Some(at) = rest.find("#[tauri::command") {
            let after = &rest[at..];
            let attr_end = after.find(']').expect("attribute end");
            let attr = after[..=attr_end].to_string();
            let sig_at = after.find("pub ").expect("command signature");
            let sig = &after[sig_at..];
            let is_async = sig.starts_with("pub async fn");
            let name_start = sig.find("fn ").expect("fn") + 3;
            let name: String = sig[name_start..]
                .chars()
                .take_while(|c| is_ident_char(*c))
                .collect();
            // The body runs to the first closing brace at column 0.
            let body_end = sig.find("\n}\n").map(|i| i + 2).unwrap_or(sig.len());
            out.push((
                file.clone(),
                name,
                attr,
                is_async,
                sig[..body_end].to_string(),
            ));
            rest = &after[sig_at + body_end.max(1)..];
        }
    }
    out
}

/// The commands in `sources` that are not async and wait on a turn lock,
/// directly or through a function or method, as "file: name".
fn main_thread_offenders(sources: &[(String, String)]) -> Vec<String> {
    let takers = lock_takers(sources);
    commands_in(sources)
        .iter()
        .filter(|(_, _, attr, is_async, _)| !is_async && !attr.contains("async"))
        .filter_map(|(file, name, _, _, body)| {
            if takes_a_turn_lock(body) {
                return Some(format!("{file}: {name}"));
            }
            first_call(&takers, body).map(|c| format!("{file}: {name} (through `{}`)", spelt(c)))
        })
        .collect()
}

#[test]
fn no_main_thread_command_waits_on_a_lock_a_tool_holds() {
    let sources = sources();
    let all = commands_in(&sources);
    assert!(
        all.len() > 50,
        "found only {} commands; the scan is broken",
        all.len()
    );
    let takers = lock_takers(&sources);
    // One for each way a lock is reached that the scan has to see; if the
    // collection of functions or the matching on fields breaks, one of
    // these goes first.
    for (kind, name, why) in [
        (
            Kind::Free,
            "run_track_tool",
            "a free function that locks the store, the engine and the dispatcher",
        ),
        (
            Kind::Method,
            "all_tool_names",
            "an `impl AppState` method that locks the dispatcher",
        ),
        (
            Kind::Free,
            "apply_blacklist",
            "a free function that reaches the dispatcher only through a method",
        ),
    ] {
        assert!(
            takers.contains(&(kind, name.to_string())),
            "the scan no longer finds `{name}`, {why}; the scan is broken: {takers:?}"
        );
    }
    let offenders = main_thread_offenders(&sources);
    assert!(
        offenders.is_empty(),
        "these commands run on the main thread and wait on a lock a running \
         tool holds, freezing the window: {offenders:?}. Declare them \
         `#[tauri::command(async)]` or `pub async fn`."
    );
}

/// Every way a lock was reached that an earlier version of the scan let
/// through, as sync commands; the scan has to name each one. The helpers
/// they go through are written the way `state.rs` and `commands.rs` write
/// theirs. The last two are fine and must not be named.
const BYPASSES: &str = r#"
impl AppState {
    pub fn all_tool_names(&self) -> Vec<String> {
        self.dispatcher
            .lock()
            .expect("dispatcher mutex poisoned")
            .tool_names()
    }
}

fn apply_blacklist(list: Option<Vec<String>>, state: &AppState) -> Option<Vec<String>> {
    list.or_else(|| Some(state.all_tool_names()))
}

#[tauri::command]
pub fn through_a_method(state: State<'_, AppState>) -> Vec<String> {
    state.all_tool_names()
}

#[tauri::command]
pub fn through_a_free_function_and_a_method(state: State<'_, AppState>) -> usize {
    apply_blacklist(None, &state).map_or(0, |l| l.len())
}

#[tauri::command]
pub fn engine_lock_called_directly(state: State<'_, AppState>) -> CmdResult<()> {
    let _engine = state.engine.lock().map_err(|_| "poisoned")?;
    Ok(())
}

#[tauri::command]
pub fn clipboard_lock_called_directly(state: State<'_, AppState>) -> CmdResult<()> {
    let _clip = state.clipboard.lock().map_err(|_| "poisoned")?;
    Ok(())
}

#[tauri::command]
pub fn engine_lock_through_a_binding(state: State<'_, AppState>) -> CmdResult<()> {
    let e = &state.engine;
    let _engine = lock_std(e, "engine")?;
    Ok(())
}

#[tauri::command]
pub fn engine_lock_through_lock_std(state: State<'_, AppState>) -> CmdResult<()> {
    let _engine = lock_std(&state.engine, "engine")?;
    Ok(())
}

#[tauri::command]
pub async fn fine_because_it_is_async(state: State<'_, AppState>) -> Vec<String> {
    state.all_tool_names()
}

#[tauri::command]
pub fn fine_because_it_takes_no_such_lock(state: State<'_, AppState>) -> Option<String> {
    state.api_key_snapshot()
}
"#;

#[test]
fn the_scan_names_every_way_of_reaching_a_lock_that_it_once_missed() {
    let sources = [("bypasses.rs".to_string(), BYPASSES.to_string())];
    let named = main_thread_offenders(&sources);
    for must in [
        "through_a_method",
        "through_a_free_function_and_a_method",
        "engine_lock_called_directly",
        "clipboard_lock_called_directly",
        "engine_lock_through_a_binding",
        "engine_lock_through_lock_std",
    ] {
        assert!(
            named
                .iter()
                .any(|n| n.starts_with(&format!("bypasses.rs: {must}"))),
            "the scan lets `{must}` through: {named:?}"
        );
    }
    assert_eq!(
        named.len(),
        6,
        "the scan names a command that takes no such lock, or is async: {named:?}"
    );
}

/// The call check is what the helper scan rests on, so it is checked
/// against the shapes it must tell apart.
#[test]
fn the_call_check_tells_a_call_from_a_lookalike() {
    let free = |body, name| calls(body, name, Kind::Free);
    assert!(free(
        "    run_track_tool(&state, \"gain\", args)",
        "run_track_tool"
    ));
    assert!(free(
        "crate::commands::run_track_tool(&state)",
        "run_track_tool"
    ));
    assert!(free("let x = (run_track_tool(&state));", "run_track_tool"));
    assert!(!free("    my_run_track_tool(&state)", "run_track_tool"));
    assert!(!free("    state.run_track_tool(x)", "run_track_tool"));
    assert!(!free(
        "fn run_track_tool(state: &AppState)",
        "run_track_tool"
    ));

    let method = |body, name| calls(body, name, Kind::Method);
    assert!(method("    state.all_tool_names()", "all_tool_names"));
    assert!(method("state\n    .all_tool_names()", "all_tool_names"));
    assert!(method("AppState::all_tool_names(&state)", "all_tool_names"));
    assert!(method("Self::all_tool_names(self)", "all_tool_names"));
    assert!(!method("    all_tool_names(&state)", "all_tool_names"));
    assert!(!method("    state.my_all_tool_names()", "all_tool_names"));
    assert!(!method(
        "    pub fn all_tool_names(&self)",
        "all_tool_names"
    ));
}

#[test]
fn the_field_check_tells_a_field_from_a_lookalike() {
    assert!(names_field("state.engine.lock()", "engine"));
    assert!(names_field("lock_std(&state.engine, \"engine\")", "engine"));
    assert!(names_field("let e = &state.engine;", "engine"));
    assert!(names_field("Arc::clone(&state.dispatcher)", "dispatcher"));
    assert!(names_field("self.clipboard\n.lock()", "clipboard"));
    assert!(names_field("a.engine_id; b.engine", "engine"));
    assert!(!names_field("audio_engine::Error", "engine"));
    assert!(!names_field("state.engine_id", "engine"));
    assert!(!names_field("state.engine()", "engine"));
    assert!(!names_field("engine: &mut engine", "engine"));
    assert!(!names_field("lock_std(&state.dispatcher)", "engine"));
}

#[test]
fn functions_are_collected_with_their_kind_and_their_own_body() {
    let src = "\
use std::sync::Mutex;

impl AppState {
    pub fn locks(&self) {
        self.engine.lock();
    }

    fn one_line(&self) -> u8 { 1 }

    pub(crate) async fn later(&self) {
        let nested = || {
            1
        };
    }
}

pub fn free_one() {
    let _ = 1;
}

mod tests {
    fn in_a_module() {}
}
";
    let funcs = functions_in(src);
    let by_name = |n: &str| funcs.iter().find(|f| f.name == n).unwrap();
    assert_eq!(
        funcs.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["locks", "one_line", "later", "free_one"]
    );
    assert_eq!(by_name("locks").kind, Kind::Method);
    assert_eq!(by_name("one_line").kind, Kind::Method);
    assert_eq!(by_name("later").kind, Kind::Method);
    assert_eq!(by_name("free_one").kind, Kind::Free);
    // Each body stops at its own closing brace.
    assert!(by_name("locks").body.contains("self.engine.lock()"));
    assert!(!by_name("locks").body.contains("one_line"));
    assert_eq!(
        by_name("one_line").body.trim(),
        "fn one_line(&self) -> u8 { 1 }"
    );
    assert!(by_name("later").body.contains("|| {"));
    assert!(!by_name("later").body.contains("free_one"));
    assert!(!by_name("free_one").body.contains("mod tests"));
}
