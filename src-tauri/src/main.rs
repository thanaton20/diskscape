// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod dto;
mod state;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running Diskscape");
}
