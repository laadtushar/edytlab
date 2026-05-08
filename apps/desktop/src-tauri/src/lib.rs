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
    accept_b, approve_plan, clear_api_key, clear_api_key_for, get_active_provider, get_graph,
    get_node, get_session_head, has_api_key, has_api_key_for, list_providers, open_project,
    prepare_compare, render_preview, send_message, set_active_provider, set_api_key,
    set_api_key_for, test_api_key, test_api_key_for, try_load_api_key_at_startup,
};
use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState::new();

    tauri::Builder::default()
        .manage(app_state.clone())
        .setup(move |_app| {
            // Best-effort: read the API key from the OS keychain at
            // launch. Not having one is fine — the frontend will
            // surface the settings modal and the user calls
            // `set_api_key` to provide one.
            try_load_api_key_at_startup(&app_state);
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
            get_active_provider,
            set_active_provider,
            get_session_head,
            get_node,
            get_graph,
            render_preview,
            prepare_compare,
            accept_b,
            approve_plan,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
