//! The tray icon (FR-SCH-001/002, FR-SET-016): PLA keeps running with the window closed.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

/// Kept so a pause from the status bar also updates the tray check (final review I4).
struct TrayItems {
    pause: CheckMenuItem<Wry>,
}

pub fn set_pause_checked(app: &AppHandle, paused: bool) {
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.pause.set_checked(paused);
    }
}

struct Labels {
    open: &'static str,
    new_note: &'static str,
    pause: &'static str,
    autostart: &'static str,
    quit: &'static str,
}

fn labels() -> Labels {
    if crate::notify::strings().done == "Tamamlandı" {
        Labels { open: "Aç", new_note: "Yeni not", pause: "Arka plan YZ'yi duraklat", autostart: "Windows ile başlat", quit: "Çık" }
    } else {
        Labels { open: "Open", new_note: "New note", pause: "Pause background AI", autostart: "Start with Windows", quit: "Quit" }
    }
}

/// The simplified logo (no dot, thicker P): sharp at the tray's 16 px (design spec § 8).
fn tray_icon() -> tauri::image::Image<'static> {
    tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png")).expect("bundled tray icon")
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let l = labels();
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let open = MenuItem::with_id(app, "open", l.open, true, None::<&str>)?;
    let new_note = MenuItem::with_id(app, "new_note", l.new_note, true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", l.pause, true, false, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(app, "autostart", l.autostart, true, autostart_on, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", l.quit, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &new_note, &separator, &pause, &autostart, &separator, &quit])?;

    app.manage(TrayItems { pause: pause.clone() });
    let pause_item = pause.clone();
    let autostart_item = autostart.clone();
    TrayIconBuilder::with_id("pla")
        .icon(tray_icon())
        .tooltip("PLA")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "new_note" => {
                show_main(app);
                let _ = app.emit("new-note", ());
            }
            "pause" => crate::commands::apply_pause(app, pause_item.is_checked().unwrap_or(false)),
            "autostart" => {
                let on = autostart_item.is_checked().unwrap_or(false);
                let manager = app.autolaunch();
                let _ = if on { manager.enable() } else { manager.disable() };
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
