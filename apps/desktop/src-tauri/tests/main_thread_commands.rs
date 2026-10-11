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
//!
//! The same scan answers a second question (#498). An `async` command
//! does not freeze the window, but a lock it waits on inside its task
//! holds one of the async runtime's workers until the lock is free, and
//! enough of them leave none to run any async command. So an `async`
//! command that waits on a turn lock must do the waiting inside
//! `blocking(..)` (or `spawn_blocking(..)`), which moves it to the
//! blocking pool. The text left once those calls are cut out is what runs
//! on the worker, and it must take no turn lock and call nothing that does.
//! `tests/blocking_commands.rs` shows the effect on a real runtime for the
//! commands it names; this covers every command.

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
/// returns that inner handle, which the caller then locks; so does the
/// registry of live stores (`state.live_stores.find(..)` / `.register(..)`,
/// #501), which is how opening a project reaches the store without going
/// through `AppState::store` first. The other two are the locks themselves.
const STORE_LOCKS: [&str; 4] = [
    "store_handle()",
    ".live_stores.",
    "lock_std(&*store",
    "lock_std(&store",
];

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
    lock_takers_except(sources, &[])
}

/// [`lock_takers`], leaving out the functions named in `except` and so
/// anything that reaches a lock only through them.
fn lock_takers_except(sources: &[(String, String)], except: &[&str]) -> BTreeSet<Callee> {
    let funcs: Vec<Func> = sources
        .iter()
        .flat_map(|(_, s)| functions_in(s))
        .filter(|f| !except.contains(&f.name.as_str()))
        .collect();
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

/// Functions that name the engine, the dispatcher and the store only to
/// clone their `Arc`s into a new agent, and wait on none of the locks a
/// tool holds. `rebuild_agent_from` (which `rebuild_agent_into` calls with the
/// real keychain reads) hands the handles to `Agent::new`. The
/// commands that rebuild the agent (a key, a model, a profile) reach them,
/// and are not waiting on a tool for it.
///
/// That is all this list says. The rebuild is not free of waiting:
/// `rebuild_agent` awaits the agent's own `tokio` mutex, which does not
/// block a worker, but which `send_message` holds for a whole turn, and a
/// plan card can keep a turn open for five minutes. A command that waits
/// for the rebuild waits for the turn, which this scan does not look for.
/// It also reads the keychain synchronously (`load_base_url`, `load_effort`
/// and, for a profile that pins another provider, `load_api_key`) on the
/// worker it runs on. So a command in the session's line must not call
/// `rebuild_agent`: `open_project` calls `rebuild_agent_soon`, which never
/// waits for a turn, and `tests/blocking_commands.rs` holds it to that.
const HANDLE_ONLY: [&str; 5] = [
    "rebuild_agent",
    "rebuild_agent_public",
    "rebuild_agent_soon",
    "rebuild_agent_into",
    "rebuild_agent_from",
];

/// Where a call to a function whose name ends in `blocking` starts in
/// `text`: the offset of its name (`blocking`, or `spawn_blocking` with its
/// prefix) and of its opening parenthesis. `blocking(` inside a longer
/// name (`is_blocking(`) is not one.
fn next_blocking_call(text: &str) -> Option<(usize, usize)> {
    let needle = "blocking(";
    let mut from = 0;
    while let Some(i) = text[from..].find(needle) {
        let at = from + i;
        let before = &text[..at];
        let start = if before.ends_with("spawn_") {
            at - "spawn_".len()
        } else {
            at
        };
        let earlier = text[..start].chars().next_back();
        if !earlier.is_some_and(|c| is_ident_char(c) || c == '.') {
            return Some((start, at + needle.len() - 1));
        }
        from = at + needle.len();
    }
    None
}

/// Where the parenthesis that `text` opens with is closed, passing over
/// string literals, character literals and `//` comments, which may hold
/// parentheses of their own.
fn closing_paren(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'\'' => {
                // `'x'` and `'\n'` are characters; `'a` is a lifetime, with
                // no closing quote to skip to.
                if bytes.get(i + 2) == Some(&b'\'') {
                    i += 2;
                } else if bytes.get(i + 1) == Some(&b'\\') && bytes.get(i + 3) == Some(&b'\'') {
                    i += 3;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// `body` with every `blocking(..)` and `spawn_blocking(..)` call cut out,
/// arguments and all. What is left runs on the async runtime's worker.
fn outside_blocking(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some((start, open)) = next_blocking_call(rest) {
        out.push_str(&rest[..start]);
        match closing_paren(&rest[open..]) {
            Some(close) => rest = &rest[open + close + 1..],
            // A call the scan cannot see the end of is not cut at all, so
            // that what is inside it is read as the worker's.
            None => {
                rest = &rest[start..];
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The `async` commands in `sources` that wait on a turn lock on the
/// runtime's worker: those that take one, or call what does, outside any
/// `blocking(..)`, as "file: name".
fn worker_offenders(sources: &[(String, String)]) -> Vec<String> {
    let takers = lock_takers_except(sources, &HANDLE_ONLY);
    commands_in(sources)
        .iter()
        .filter(|(_, _, attr, is_async, _)| *is_async || attr.contains("async"))
        .filter_map(|(file, name, _, _, body)| {
            let on_the_worker = outside_blocking(body);
            if takes_a_turn_lock(&on_the_worker) {
                return Some(format!("{file}: {name}"));
            }
            first_call(&takers, &on_the_worker)
                .map(|c| format!("{file}: {name} (through `{}`)", spelt(c)))
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

#[test]
fn no_async_command_waits_on_a_lock_on_the_runtimes_worker() {
    let sources = sources();
    // The scan has to be looking at the right thing: the commands that
    // were moved to the blocking pool are in it, and say so.
    let moved = commands_in(&sources)
        .iter()
        .filter(|(_, _, attr, is_async, body)| {
            (*is_async || attr.contains("async")) && outside_blocking(body).len() < body.len()
        })
        .count();
    assert!(
        moved > 30,
        "found only {moved} commands that wait through `blocking(..)`; the scan is broken"
    );
    let offenders = worker_offenders(&sources);
    assert!(
        offenders.is_empty(),
        "these async commands wait on a lock a running tool can hold, on the \
         async runtime's worker, and enough of them leave no worker to run any \
         async command: {offenders:?}. Do the waiting inside `blocking(&state, \
         |state| ..)`."
    );
}

/// Async commands, as the scan above has to read them. The first four wait
/// on a worker in a way it has to name; the last three do not.
const ON_THE_WORKER: &str = r#"
#[tauri::command]
pub async fn waits_directly(state: State<'_, AppState>) -> CmdResult<()> {
    let store = lock_std(&state.store_handle().unwrap(), "store")?;
    drop(store);
    Ok(())
}

#[tauri::command(async)]
pub fn waits_directly_but_is_async_by_attribute(state: State<'_, AppState>) -> CmdResult<()> {
    let _engine = lock_std(&state.engine, "engine")?;
    Ok(())
}

#[tauri::command]
pub async fn waits_through_a_helper(state: State<'_, AppState>) -> CmdResult<String> {
    run_track_tool(&state, "mute_track", serde_json::json!({}))
}

#[tauri::command]
pub async fn waits_outside_the_one_call_it_moved(state: State<'_, AppState>) -> CmdResult<()> {
    blocking(&state, |state| {
        let _store = lock_std(&state.store_handle().unwrap(), "store")?;
        Ok(())
    })
    .await?;
    // A second wait, left where it was.
    let _engine = lock_std(&state.engine, "engine")?;
    Ok(())
}

#[tauri::command]
pub async fn waits_on_the_blocking_pool(state: State<'_, AppState>) -> CmdResult<String> {
    blocking(&state, |state| {
        // A parenthesis in a comment ( and in text: ")" and '(' ...
        let _msg = "closes ) early?";
        let _open = '(';
        let _store = lock_std(&state.store_handle().unwrap(), "store")?;
        run_track_tool(state, "mute_track", serde_json::json!({}))
    })
    .await
}

#[tauri::command]
pub async fn waits_on_the_blocking_pool_directly(state: State<'_, AppState>) -> CmdResult<()> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _engine = lock_std(&state.engine, "engine").unwrap();
    })
    .await
    .unwrap();
    Ok(())
}

#[tauri::command]
pub async fn rebuilds_the_agent(state: State<'_, AppState>) -> CmdResult<()> {
    rebuild_agent(&state).await?;
    Ok(())
}

async fn rebuild_agent(state: &AppState) -> Result<(), CommandError> {
    let _store = state.store_handle();
    let _engine = Arc::clone(&state.engine);
    Ok(())
}

fn run_track_tool(state: &AppState, tool: &str, args: serde_json::Value) -> CmdResult<String> {
    let _store = lock_std(&state.store_handle().unwrap(), "store")?;
    Ok(String::new())
}
"#;

#[test]
fn the_scan_names_the_async_commands_that_wait_on_the_worker_and_only_those() {
    let sources = [("on_the_worker.rs".to_string(), ON_THE_WORKER.to_string())];
    let named = worker_offenders(&sources);
    for must in [
        "waits_directly",
        "waits_directly_but_is_async_by_attribute",
        "waits_through_a_helper",
        "waits_outside_the_one_call_it_moved",
    ] {
        assert!(
            named
                .iter()
                .any(|n| n.starts_with(&format!("on_the_worker.rs: {must}"))),
            "the scan lets `{must}` through: {named:?}"
        );
    }
    assert_eq!(
        named.len(),
        4,
        "the scan names a command that waits on the blocking pool, or on no \
         lock: {named:?}"
    );
}

#[test]
fn the_blocking_cut_takes_out_the_call_and_nothing_else() {
    let cut = |text: &str| outside_blocking(text);
    assert_eq!(cut("a; blocking(&s, |s| f(s)).await; b"), "a; .await; b");
    assert_eq!(cut("x(); spawn_blocking(move || g()).await"), "x(); .await");
    // Parentheses inside the call's strings, characters and comments do
    // not end it early.
    assert_eq!(cut(r#"blocking(s, |s| h(")", '(')) tail"#), " tail");
    assert_eq!(cut("blocking(s, |s| { // )\n f(s) }) tail"), " tail");
    // Two calls.
    assert_eq!(cut("blocking(a) mid blocking(b) end"), " mid  end");
    // A longer name, or a method, is not the call.
    assert_eq!(cut("is_blocking(a)"), "is_blocking(a)");
    assert_eq!(cut("x.blocking(a)"), "x.blocking(a)");
    // A call the scan cannot see the end of is left in, to be read as the worker's.
    assert_eq!(cut("pre blocking(a, b"), "pre blocking(a, b");
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
pub fn store_lock_through_the_registry(state: State<'_, AppState>) -> CmdResult<()> {
    let handle = state.live_stores.find(&dir).ok_or("no live store")?;
    let _store = lock_std(&handle, "store")?;
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
        "store_lock_through_the_registry",
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
        7,
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
