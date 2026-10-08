//! PLA desktop app: Tauri shell around pla-core.

mod commands;
pub mod fileops_cmds;
pub mod media_cmds;
pub mod memory_cmds;
pub mod summary_cmds;
pub mod qa_cmds;
pub mod indexer;
pub mod links_cmds;
pub mod metrics_cmds;
pub mod model_download;
pub mod model_paths;
pub mod wizard;
mod notify;
mod tray;
pub mod scheduler;
pub mod settings_screen;
pub mod system;
pub mod watcher;
pub mod worker;

pub fn run() {
    let app = tauri::Builder::default()
        // FR-VLT-020: a second launch (e.g. while PLA sits in the tray) shows the running window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .setup(|app| {
            // FR-SET-012: tray and notifications speak the language chosen in PLA.
            if let Some(root) = pla_core::vault::default_app_root() {
                notify::set_ui_language(pla_core::settings::peek_language(&root).as_deref());
            }
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
            commands::pending_reminders,
            commands::dismiss_missed,
            commands::model_status,
            commands::model_download_start,
            commands::model_download_pause,
            commands::model_use_local,
            commands::model_remove,
            commands::wizard_defaults,
            commands::inspect_vault_folder,
            commands::setup_vault,
            commands::set_language,
            commands::finish_setup,
            commands::settings_get,
            commands::settings_set,
            commands::set_autostart,
            commands::send_test_notification,
            fileops_cmds::link_count,
            fileops_cmds::rename_note,
            fileops_cmds::move_entry,
            fileops_cmds::rename_folder,
            fileops_cmds::delete_entry,
            fileops_cmds::create_folder,
            fileops_cmds::list_templates,
            fileops_cmds::create_note_in,
            fileops_cmds::open_today,
            media_cmds::note_meta,
            media_cmds::image_bytes,
            media_cmds::save_image,
            qa_cmds::qa_ask,
            qa_cmds::qa_stop,
            qa_cmds::qa_history,
            qa_cmds::qa_clear,
            qa_cmds::qa_undo,
            memory_cmds::memory_status,
            memory_cmds::memory_download_start,
            memory_cmds::memory_download_pause,
            summary_cmds::day_summary,
            summary_cmds::summary_regenerate,
            links_cmds::search_notes,
            links_cmds::quick_open,
            links_cmds::backlinks,
            links_cmds::list_tags,
            links_cmds::open_link,
            metrics_cmds::metrics_overview,
            metrics_cmds::metrics_summary,
            metrics_cmds::metric_records,
            metrics_cmds::metric_log,
            metrics_cmds::metric_edit,
            metrics_cmds::metric_delete,
            metrics_cmds::metric_resolve,
            settings_screen::backup_status,
            settings_screen::export_data,
            settings_screen::notification_status,
            settings_screen::open_place,
            settings_screen::about_info,
            commands::hide_to_tray,
        ])
        .build(tauri::generate_context!())
        .expect("error while building PLA");
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            // Tauri ends the process without running destructors: stop the worker (and with it the
            // model) explicitly. The job object in pla-core is the backstop for crashes.
            use tauri::Manager;
            handle.state::<commands::AppState>().downloads.shutdown(std::time::Duration::from_secs(2));
            handle.state::<commands::AppState>().memory_downloads.shutdown(std::time::Duration::from_secs(2));
            let session = handle.state::<commands::AppState>().session.lock().expect("session lock").take();
            drop(session);
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_window_fits_a_1080p_screen_at_125_percent() {
        // User test finding 1: the status bar ended up under the taskbar
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let window = &conf["app"]["windows"][0];
        let fits = |h: f64| h * 1.25 + 40.0 <= 1080.0 - 48.0;
        assert!(fits(window["height"].as_f64().unwrap()), "{window}");
        assert!(fits(window["minHeight"].as_f64().unwrap()), "{window}");
        assert_eq!(window["center"], true, "Windows' default position (y≈134) pushes even a fitting window under the taskbar");
    }

    #[test]
    fn the_window_may_be_closed() {
        // Final review C1: with a close-requested listener, closing needs permission to destroy the window
        let caps: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        assert!(caps["permissions"].as_array().unwrap().iter().any(|p| p == "core:window:allow-destroy"));
    }

    fn png_size(bytes: &[u8]) -> (u32, u32) {
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
        let be = |i: usize| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap());
        (be(16), be(20))
    }

    #[test]
    fn the_tray_has_its_own_small_logo() {
        // Design spec § 8: a simplified 32 px logo stays sharp at 16 px in the tray
        assert_eq!(png_size(include_bytes!("../icons/tray.png")), (32, 32));
    }

    #[test]
    fn the_window_icon_has_small_and_large_frames() {
        let ico = include_bytes!("../icons/icon.ico");
        let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
        let sizes: Vec<u32> = (0..count).map(|i| match ico[6 + i * 16] { 0 => 256, w => w as u32 }).collect();
        for want in [16, 32, 256] {
            assert!(sizes.contains(&want), "missing {want} px frame: {sizes:?}");
        }
    }

    #[test]
    fn a_fresh_clone_has_the_llama_folder_tauri_build_needs() {
        // Final review I2: tauri-build fails when a resource folder is missing; the binaries are
        // git-ignored, the explaining README is not
        assert!(include_str!("../llama/README.md").contains("npm run fetch-llama"));
    }

    #[test]
    fn the_bundled_llama_server_is_packaged() {
        // Model-manager spec § 5: src-tauri/llama/ ships as a resource
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["bundle"]["resources"]["llama/"], "llama/");
    }

    #[test]
    fn every_manifest_carries_the_same_version() {
        // Packaging: the installer's name, the About screen and Windows' app list agree
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let npm: serde_json::Value = serde_json::from_str(include_str!("../../package.json")).unwrap();
        let version = conf["version"].as_str().unwrap();
        assert_eq!(npm["version"], version);
        assert_eq!(env!("CARGO_PKG_VERSION"), version);
        assert!(include_str!("../../../../crates/pla-core/Cargo.toml").contains(&format!("version = \"{version}\"")));
    }

    #[test]
    fn notifications_use_the_identifier_the_installer_registers() {
        // Packaging: the Start menu shortcut's AppUserModelID is the bundle identifier
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], crate::notify::IDENTIFIER);
    }

    #[test]
    fn the_installer_is_per_user_and_keeps_the_vault_safe() {
        // Owner decisions 2026-10-08: NSIS for this user only, no admin; WebView2 bootstrapper inside
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let bundle = &conf["bundle"];
        assert_eq!(bundle["active"], true);
        assert_eq!(bundle["targets"], serde_json::json!(["nsis"]));
        let nsis = &bundle["windows"]["nsis"];
        assert_eq!(nsis["installMode"], "currentUser");
        assert_eq!(nsis["installerHooks"], "installer/hooks.nsh");
        assert_eq!(bundle["windows"]["webviewInstallMode"]["type"], "embedBootstrapper");
        // NFR-SEC-010: what the hooks do is tested with makensis (scripts/installer-hooks.test.mjs);
        // here, that Tauri's installer calls them: it inserts only these macro names
        let hooks = include_str!("../installer/hooks.nsh");
        for name in ["NSIS_HOOK_POSTINSTALL", "NSIS_HOOK_PREUNINSTALL", "NSIS_HOOK_POSTUNINSTALL"] {
            assert!(hooks.contains(&format!("!macro {name}\n")) || hooks.contains(&format!("!macro {name}\r\n")), "{name}");
        }
        assert!(hooks.contains("!insertmacro PLA_REMOVE_DATA \"$APPDATA\\PLA\" \"$LOCALAPPDATA\\PLA\""), "on PLA's real folders");
        // without this value the uninstaller opens with a language dialog
        assert!(hooks.contains("\"Installer Language\" $LANGUAGE"));
    }
}
