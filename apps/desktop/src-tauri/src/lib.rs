//! edytlab desktop entry point.
//!
//! Wires the Tauri shell to the Phase 1 core crates. The app boots with
//! an empty state; the user's first action is either to set their
//! Anthropic API key or to open a project. The agent is constructed
//! lazily once both have been provided.

pub mod commands;
pub mod compare;
pub mod events;
pub mod live_stores;
pub mod mcp_tool;
pub mod project;
pub mod reclaimer;
pub mod settings;
pub mod state;

use crate::commands::{
    accept_b, add_marker, apply_template, approve_plan, batch_load, cancel_long_running_tool,
    clear_api_key, clear_api_key_for, cut_transcript_words, default_base_url_for,
    delete_agent_profile, delete_mcp_server, delete_skill, duplicate_track, end_compare,
    forget_recent_project, get_active_agent_profile, get_active_model, get_active_provider,
    get_base_url_for, get_effort_for, get_graph, get_head_trail, get_keychain_persistence,
    get_node, get_plan_first, get_project_meta, get_render_warnings, get_session_head,
    get_sync_lock, get_transcript, get_view_state, has_api_key, has_api_key_for,
    install_bundled_skills, install_plugin, list_agent_profiles, list_capabilities, list_markers,
    list_mcp_servers, list_models_for, list_providers, list_recent_projects, list_skills,
    list_templates, list_tracks, ml_runtime_status, move_clip, open_project, prepare_compare,
    read_agent_profile, read_mcp_server, read_memory, read_skill, reject_plan, remove_clip,
    remove_marker, remove_track, rename_node, rename_track, render_preview, render_range,
    restart_mcp_server, save_head_trail, save_project_as, save_view_state, send_message,
    set_active_agent_profile, set_active_model, set_active_provider, set_api_key, set_api_key_for,
    set_base_url_for, set_clip_envelope, set_effort_for, set_head_to, set_plan_first,
    set_project_meta, set_selection_context, set_sync_lock, set_track_gain, set_track_muted,
    set_track_pan, set_track_soloed, start_recording, stop_recording, test_api_key,
    test_api_key_for, timer_record, try_load_api_key_at_startup, update_marker,
    upsert_agent_profile, upsert_mcp_server, upsert_skill, write_memory, RecorderState,
};
use crate::state::AppState;
use std::sync::{Arc, Mutex};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    Emitter, Manager,
};

/// Event name the frontend listens for when the user picks `File > Open
/// Audio…` from the native menu. The webview's own dialog button uses
/// the same flow client-side, so this event keeps the menu and toolbar
/// behavioural parity in one place.
const MENU_OPEN_FILE_EVENT: &str = "menu://open-file";

/// Register the directory the installer puts the ONNX Runtime library in,
/// so `ml_pipeline::runtime::ensure` looks there after `ORT_DYLIB_PATH` and
/// before the executable's own directory (#383).
///
/// Where that is depends on the platform (`runtime::bundle_dir_for`): the
/// app bundle's `Contents/Frameworks` on macOS, Tauri's resource directory
/// (`resource_dir`) on Linux (`/usr/lib/edytlab` for a deb, inside the
/// mount for an AppImage), and on Windows nowhere new, because the DLL sits
/// next to the exe, which is always searched. Registering nothing is fine:
/// a build that is not installed (`cargo run`, `tauri dev`) has the library
/// next to the executable or has none.
fn register_ml_runtime_bundle(resource_dir: Option<&std::path::Path>) {
    let exe = std::env::current_exe().ok();
    let Some(dir) =
        ml_pipeline::runtime::bundle_dir_for(std::env::consts::OS, exe.as_deref(), resource_dir)
    else {
        return;
    };
    if !ml_pipeline::runtime::set_bundle_dir(dir.clone()) {
        tracing::warn!(dir = %dir.display(), "could not register the ONNX Runtime bundle directory");
    }
}

/// The one argument that makes the app print the answer Settings' ONNX
/// Runtime line shows, as one line of JSON on stdout (the `ml_runtime_status`
/// command's answer), and exit without opening a window (#383).
///
/// The release workflows run it against each unpacked installer
/// (`scripts/installer-ml-runtime.py check`) to show that the app, in that
/// installer's layout, finds the library the installer carries and calls it
/// bundled. It looks where the app looks, `ORT_DYLIB_PATH` first, and loads
/// the library to read its version, as the Settings line does; it reads and
/// writes nothing else.
pub const ML_RUNTIME_STATUS_ARG: &str = "--ml-runtime-status";

/// `edytlab-desktop --ml-runtime-status`: see [`ML_RUNTIME_STATUS_ARG`].
/// Registers the bundle directory as `setup` would, from Tauri's own
/// resource-directory rule, and answers on this thread, before any runtime
/// or window exists. Returns the process's exit code: 0 once the answer is
/// written, whatever it says, and 1 if it could not be written.
fn print_ml_runtime_status(package_info: &tauri::PackageInfo) -> i32 {
    use std::io::Write;

    let resource_dir =
        tauri::utils::platform::resource_dir(package_info, &tauri::Env::default()).ok();
    register_ml_runtime_bundle(resource_dir.as_deref());
    let status = commands::ml_runtime_status_now();
    let Ok(line) = serde_json::to_string(&status) else {
        return 1;
    };
    let mut out = std::io::stdout().lock();
    match writeln!(out, "{line}").and_then(|()| out.flush()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let context = tauri::generate_context!();
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == ML_RUNTIME_STATUS_ARG {
        std::process::exit(print_ml_runtime_status(context.package_info()));
    }

    let app_state = AppState::new();
    // Pending from the first moment, before any command can ask: the
    // keychain reads that settle it start in `setup` below and can take
    // as long as the user does (#497).
    app_state.begin_startup_reads();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state.clone())
        .manage(RecorderState(std::sync::Mutex::new(None)))
        .setup(move |app| {
            // Where a long tool's progress goes (#169 §1). Registered
            // once, here, because `tools` reports without knowing who
            // is listening — see `tools::progress`. The handle is
            // cloned into the closure so the sink outlives setup.
            let progress_handle = app.handle().clone();
            tools::progress::set_sink(move |event| {
                let _ = progress_handle.emit("tool-progress", event);
            });

            // Tell the ONNX Runtime loader where the installer puts the
            // library (#383). Before anything can ask it to load one.
            register_ml_runtime_bundle(app.path().resource_dir().ok().as_deref());

            // Install the memory store. Spec location is `~/.edytlab/
            // memory.md`; if the user's home dir can't be resolved we
            // fall back to the app data dir so the store is still
            // usable on sandboxed / non-HOME builds.
            let memory_path = resolve_global_memory_path(app);
            app_state.install_memory_store(memory_path);

            // Install the skill library. Same fallback logic as
            // memory — produces an empty library when the directory
            // doesn't exist yet, so first-run is clean.
            let skills_dir = resolve_global_skills_dir(app);
            app_state.install_skill_library(skills_dir);

            // Install the agent-profile library. Restores the active
            // profile selection from the on-disk sidecar so the
            // user's last choice survives restart.
            let agents_dir = resolve_global_agent_profiles_dir(app);
            app_state.install_agent_profile_library(agents_dir);

            // Read the settings file (~/.edytlab/settings.json). On this
            // thread, before the window can ask for anything: Plan first is
            // an approval gate, and the first message of the launch has to
            // find it as the person left it (#569). A missing or corrupt
            // file means off and never stops the app starting.
            app_state.install_settings(resolve_global_settings_path(app));

            // Install the MCP config (~/.edytlab/mcp.json), then, on a
            // background thread, read the keychain and start every enabled
            // server. Without the second part a registered server
            // contributes no tools until the user opens Settings and
            // restarts it by hand — every launch, for every server.
            //
            // Off the setup thread, both: each server costs a process
            // spawn plus an `initialize` round trip, and a locked Secret
            // Service's unlock prompt has no time limit (dbus-secret-service
            // waits a year), so on the setup thread no window would appear
            // until the user answered it (#497). The commands that need
            // what the reads fill in wait for them (`startup_reads_settled`),
            // and the frontend says it is waiting for the keyring.
            //
            // The servers start after the reads, in this one thread: whether
            // the keychain refused decides which of them may be started.
            let mcp_path = resolve_global_mcp_path(app);
            app_state.install_mcp(mcp_path);
            {
                let state_for_startup = app_state.clone();
                std::thread::spawn(move || {
                    {
                        // Ends the wait however this block ends.
                        let _settled = state_for_startup.startup_reads_guard();
                        try_load_api_key_at_startup(&state_for_startup);
                    }
                    crate::commands::start_enabled_mcp_servers(&state_for_startup);
                });
            }

            // Auto-initialise a default project store under the OS app
            // data dir so the agent can be built immediately after
            // set_api_key. Without this, rebuild_agent stays a no-op
            // (it requires both an api key AND a store) and the user
            // would see "no agent configured" forever despite saving
            // a key. A user-driven `open_project` later overwrites
            // this default seamlessly.
            if let Ok(data_dir) = app.path().app_data_dir() {
                let project_dir = data_dir.join("default-project");
                if let Err(e) = std::fs::create_dir_all(&project_dir) {
                    tracing::warn!(
                        error = %e,
                        path = %project_dir.display(),
                        "could not create default project dir"
                    );
                } else {
                    match session::Store::open(&project_dir) {
                        Ok(store) => {
                            crate::commands::sweep_orphaned_audio(&store);
                            crate::commands::allow_assets_in_dir(app.handle(), &project_dir);
                            // Registered, so a project opened again later
                            // finds this store while anything still holds it.
                            let store = app_state
                                .live_stores
                                .register(&project_dir, Arc::new(Mutex::new(store)));
                            app_state.set_open_project(store, project_dir.clone());
                            // If a key was loaded by the startup reads,
                            // kick a rebuild so the agent is ready for the
                            // very first chat turn — no manual save
                            // needed. After them, not alongside: the
                            // store is in place by now, and the key may
                            // not be.
                            let state_for_rebuild = app_state.clone();
                            tauri::async_runtime::spawn(async move {
                                state_for_rebuild.startup_reads_settled().await;
                                if let Err(e) =
                                    crate::commands::rebuild_agent_public(&state_for_rebuild).await
                                {
                                    tracing::warn!(
                                        error = ?e,
                                        "agent rebuild at startup failed"
                                    );
                                }
                            });
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = ?e,
                                path = %project_dir.display(),
                                "could not open default project store"
                            );
                        }
                    }
                }
            } else {
                tracing::warn!(
                    "could not resolve app data dir; agent will require manual open_project"
                );
            }

            // Keep the open project's derived audio under its cap (#98):
            // orphans went as the store opened; past the cap, the oldest
            // audio only undo history holds goes too, once a replay has
            // rebuilt it. Its own thread, because verifying takes replays.
            crate::reclaimer::spawn(app_state.clone());

            // Native menu: File > Open Audio… / Quit. Frontend listens
            // for `menu://open-file` and runs the dialog open + load
            // path; Quit uses tauri's built-in close behaviour.
            let open_audio = MenuItemBuilder::with_id("open_audio", "Open Audio…")
                .accelerator("CmdOrCtrl+O")
                .build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit")
                .accelerator("CmdOrCtrl+Q")
                .build(app)?;
            let file_menu = SubmenuBuilder::new(app, "File")
                .item(&open_audio)
                .separator()
                .item(&quit)
                .build()?;

            // Cut / Copy / Paste / Select All / Undo have to be real
            // menu items or they do not work at all.
            //
            // On macOS those shortcuts are not handled by the webview.
            // They are dispatched through the responder chain by the
            // Edit menu's *key equivalents*, so an application with no
            // Edit menu has no ⌘V — the keystroke reaches nothing and
            // is silently dropped.
            //
            // Tauri's default menu includes this submenu. Building a
            // menu from scratch with `MenuBuilder::new` and calling
            // `set_menu` replaces that default wholesale, so adding a
            // File menu had the side effect of deleting Edit — and with
            // it, pasting into every text field in the app: the chat
            // box, the API key and base URL fields, the model picker,
            // the graph's rename overlay, the label lane.
            //
            // Nothing about the failure points at the menu bar, which
            // is why it survived: the field just ignores ⌘V.
            let edit_menu = SubmenuBuilder::new(app, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;

            let menu = MenuBuilder::new(app)
                .item(&file_menu)
                .item(&edit_menu)
                .build()?;
            app.set_menu(menu)?;
            app.on_menu_event(|app_handle, event| match event.id().as_ref() {
                "open_audio" => {
                    let _ = app_handle.emit(MENU_OPEN_FILE_EVENT, ());
                }
                "quit" => {
                    app_handle.exit(0);
                }
                _ => {}
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_project,
            batch_load,
            send_message,
            set_api_key,
            set_api_key_for,
            get_base_url_for,
            set_base_url_for,
            default_base_url_for,
            get_effort_for,
            set_effort_for,
            set_plan_first,
            get_plan_first,
            reject_plan,
            has_api_key,
            has_api_key_for,
            get_keychain_persistence,
            ml_runtime_status,
            clear_api_key,
            clear_api_key_for,
            test_api_key,
            test_api_key_for,
            list_providers,
            list_models_for,
            get_active_provider,
            set_active_provider,
            get_active_model,
            set_active_model,
            get_session_head,
            get_node,
            get_graph,
            render_preview,
            get_render_warnings,
            render_range,
            prepare_compare,
            end_compare,
            accept_b,
            approve_plan,
            set_selection_context,
            add_marker,
            remove_marker,
            update_marker,
            cancel_long_running_tool,
            list_markers,
            get_transcript,
            cut_transcript_words,
            list_tracks,
            set_track_gain,
            set_track_pan,
            set_track_muted,
            rename_track,
            remove_track,
            duplicate_track,
            get_project_meta,
            set_project_meta,
            get_view_state,
            save_view_state,
            get_head_trail,
            save_head_trail,
            save_project_as,
            list_recent_projects,
            forget_recent_project,
            set_track_soloed,
            set_sync_lock,
            get_sync_lock,
            set_clip_envelope,
            move_clip,
            remove_clip,
            set_head_to,
            rename_node,
            list_capabilities,
            list_skills,
            read_skill,
            upsert_skill,
            delete_skill,
            list_agent_profiles,
            read_agent_profile,
            upsert_agent_profile,
            delete_agent_profile,
            get_active_agent_profile,
            set_active_agent_profile,
            list_mcp_servers,
            read_mcp_server,
            upsert_mcp_server,
            delete_mcp_server,
            restart_mcp_server,
            read_memory,
            write_memory,
            list_templates,
            apply_template,
            start_recording,
            timer_record,
            stop_recording,
            install_bundled_skills,
            install_plugin,
        ])
        .run(context)
        .expect("error while running tauri application");
}

/// Resolve `~/.edytlab/memory.md`. Uses Tauri's path resolver so the
/// home directory resolves consistently across Linux / macOS /
/// Windows and through sandbox wrappers; falls back to the OS app
/// data dir on the rare platform where `home_dir()` is unavailable.
fn resolve_global_memory_path<R: tauri::Runtime>(app: &tauri::App<R>) -> std::path::PathBuf {
    if let Ok(home) = app.path().home_dir() {
        return home.join(".edytlab").join("memory.md");
    }
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".edytlab")
        .join("memory.md")
}

/// Resolve `~/.edytlab/skills/`. Same fallback as the memory path.
fn resolve_global_skills_dir<R: tauri::Runtime>(app: &tauri::App<R>) -> std::path::PathBuf {
    if let Ok(home) = app.path().home_dir() {
        return home.join(".edytlab").join("skills");
    }
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".edytlab")
        .join("skills")
}

/// Resolve `~/.edytlab/agents/`. Same fallback strategy.
fn resolve_global_agent_profiles_dir<R: tauri::Runtime>(app: &tauri::App<R>) -> std::path::PathBuf {
    if let Ok(home) = app.path().home_dir() {
        return home.join(".edytlab").join("agents");
    }
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".edytlab")
        .join("agents")
}

/// Resolve `~/.edytlab/settings.json`. Same fallback strategy.
fn resolve_global_settings_path<R: tauri::Runtime>(app: &tauri::App<R>) -> std::path::PathBuf {
    if let Ok(home) = app.path().home_dir() {
        return home.join(".edytlab").join(settings::SETTINGS_FILE);
    }
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".edytlab")
        .join(settings::SETTINGS_FILE)
}

/// Resolve `~/.edytlab/mcp.json`. Same fallback strategy.
fn resolve_global_mcp_path<R: tauri::Runtime>(app: &tauri::App<R>) -> std::path::PathBuf {
    if let Ok(home) = app.path().home_dir() {
        return home.join(".edytlab").join("mcp.json");
    }
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".edytlab")
        .join("mcp.json")
}
