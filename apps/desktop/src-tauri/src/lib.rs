//! PLA desktop app: Tauri shell around pla-core.

mod commands;
mod notify;
mod tray;
pub mod scheduler;
pub mod system;
pub mod watcher;
pub mod worker;

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .setup(|app| {
            tray::build(app.handle())?;
            // FR-SET-016: started by Windows sign-in → stay in the tray.
            if !std::env::args().any(|a| a == "--hidden") {
                tray::show_main(app.handle());
            }
            Ok(())
        })
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
            commands::vault_info,
            commands::worker_status,
            commands::list_tasks,
            commands::add_task,
            commands::edit_task,
            commands::set_task_done,
            commands::delete_task,
            commands::undo_item,
            commands::list_review,
            commands::accept_review,
            commands::reject_review,
            commands::reminder_done,
            commands::reminder_snooze,
            commands::backup_now,
            commands::set_paused,
            commands::hide_to_tray,
        ])
        .build(tauri::generate_context!())
        .expect("error while building PLA");
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            // Tauri ends the process without running destructors: stop the worker (and with it the
            // model) explicitly. The job object in pla-core is the backstop for crashes.
            use tauri::Manager;
            let session = handle.state::<commands::AppState>().session.lock().expect("session lock").take();
            drop(session);
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_window_may_be_closed() {
        // Final review C1: with a close-requested listener, closing needs permission to destroy the window
        let caps: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        assert!(caps["permissions"].as_array().unwrap().iter().any(|p| p == "core:window:allow-destroy"));
    }
}
