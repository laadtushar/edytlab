//! edytlab desktop entry point.
//!
//! Wires the Tauri shell to the Phase 1 core crates. The app boots with
//! an empty state; the user's first action is either to set their
//! Anthropic API key or to open a project. The agent is constructed
//! lazily once both have been provided.

pub mod commands;
pub mod events;
pub mod state;

use crate::commands::{
    accept_b, add_marker, approve_plan, clear_api_key, clear_api_key_for, delete_agent_profile,
    delete_mcp_server, delete_skill, get_active_agent_profile, get_active_model,
    get_active_provider, get_graph, get_node, get_session_head, has_api_key, has_api_key_for,
    list_agent_profiles, list_capabilities, list_markers, list_mcp_servers, list_models_for,
    list_providers, list_skills, open_project, prepare_compare, read_agent_profile,
    read_mcp_server, read_memory, read_skill, remove_marker, render_preview, restart_mcp_server,
    send_message, set_active_agent_profile, set_active_model, set_active_provider, set_api_key,
    set_api_key_for, set_selection_context, test_api_key, test_api_key_for,
    try_load_api_key_at_startup, upsert_agent_profile, upsert_mcp_server, upsert_skill,
    write_memory,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state.clone())
        .setup(move |app| {
            try_load_api_key_at_startup(&app_state);

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

            // Install the MCP config (~/.edytlab/mcp.json). Servers
            // are not auto-started here — the user explicitly
            // restarts each one from the Settings panel.
            let mcp_path = resolve_global_mcp_path(app);
            app_state.install_mcp(mcp_path);

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
                            app_state.set_store(Some(Arc::new(Mutex::new(store))));
                            app_state.set_project_dir(Some(project_dir.clone()));
                            // If a key was already loaded above, kick a
                            // rebuild so the agent is ready for the very
                            // first chat turn — no manual save needed.
                            let state_for_rebuild = app_state.clone();
                            tauri::async_runtime::spawn(async move {
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
            let menu = MenuBuilder::new(app).item(&file_menu).build()?;
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
            send_message,
            set_api_key,
            set_api_key_for,
            has_api_key,
            has_api_key_for,
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
            prepare_compare,
            accept_b,
            approve_plan,
            set_selection_context,
            add_marker,
            remove_marker,
            list_markers,
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
        ])
        .run(tauri::generate_context!())
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
fn resolve_global_agent_profiles_dir<R: tauri::Runtime>(
    app: &tauri::App<R>,
) -> std::path::PathBuf {
    if let Ok(home) = app.path().home_dir() {
        return home.join(".edytlab").join("agents");
    }
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".edytlab")
        .join("agents")
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
