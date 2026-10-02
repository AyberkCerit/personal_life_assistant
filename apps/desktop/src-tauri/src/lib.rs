//! PLA desktop app: Tauri shell around pla-core.

mod commands;
pub mod watcher;
pub mod worker;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::startup,
            commands::open_vault,
            commands::list_tree,
            commands::read_note,
            commands::save_note,
            commands::save_copy,
            commands::create_note,
            commands::queue_note,
        ])
        .run(tauri::generate_context!())
        .expect("error while running PLA");
}
