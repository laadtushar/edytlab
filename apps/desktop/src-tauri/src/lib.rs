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
    accept_b, approve_plan, clear_api_key, clear_api_key_for, get_active_model,
    get_active_provider, get_graph, get_node, get_session_head, has_api_key, has_api_key_for,
    list_models_for, list_providers, open_project, prepare_compare, render_preview, send_message,
    set_active_model, set_active_provider, set_api_key, set_api_key_for, test_api_key,
    test_api_key_for, try_load_api_key_at_startup,
};
use crate::state::AppState;
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
