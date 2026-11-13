// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod commands;
mod dto;
mod state;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::initial_path,
            commands::list_volumes,
            commands::start_scan,
            commands::cancel_scan,
            commands::get_children,
            commands::get_nodes,
            commands::get_ancestors,
            commands::get_path,
            commands::get_treemap,
            commands::get_summary,
            commands::import_ncdu,
            commands::export_ncdu,
            actions::get_selection_info,
            actions::delete_nodes,
            actions::reveal,
            actions::request_elevation,
            actions::toggle_preview,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Diskscape")
        .run(|app, event| {
            // A preview window belongs to this app: do not leave it behind.
            if let tauri::RunEvent::Exit = event {
                use tauri::Manager;
                app.state::<state::AppState>().close_preview();
            }
        });
}
