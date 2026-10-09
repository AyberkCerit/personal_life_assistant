//! The tray icon (FR-SCH-001/002, FR-SET-016): PLA keeps running with the window closed.

use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

/// The check items of the current menu, so a change made in the window also updates the tray
/// (final review I4); replaced when the menu is rebuilt in another language.
#[derive(Default)]
struct TrayItems(Mutex<Option<(CheckMenuItem<Wry>, CheckMenuItem<Wry>)>>);

pub fn set_pause_checked(app: &AppHandle, paused: bool) {
    if let Some((pause, _)) = app.try_state::<TrayItems>().and_then(|items| items.0.lock().expect("tray lock").clone()) {
        let _ = pause.set_checked(paused);
    }
}

pub fn set_autostart_checked(app: &AppHandle, on: bool) {
    if let Some((_, autostart)) = app.try_state::<TrayItems>().and_then(|items| items.0.lock().expect("tray lock").clone()) {
        let _ = autostart.set_checked(on);
    }
}

/// The simplified logo (no dot, thicker P): sharp at the tray's 16 px (design spec § 8).
fn tray_icon() -> tauri::image::Image<'static> {
    tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png")).expect("bundled tray icon")
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        crate::webview_memory::set_low(&window, false);
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// FR-SET-016: one path for the tray item and the settings screen, so both agree.
pub fn apply_autostart(app: &AppHandle, on: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    if on { manager.enable() } else { manager.disable() }.map_err(|e| e.to_string())?;
    set_autostart_checked(app, on);
    let _ = app.emit("autostart-changed", on);
    Ok(())
}

/// The menu in the current UI language, with the checks as they are now.
fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let s = crate::notify::strings();
    let paused = app.state::<crate::commands::AppState>().paused.load(std::sync::atomic::Ordering::SeqCst);
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let open = MenuItem::with_id(app, "open", s.tray_open, true, None::<&str>)?;
    let new_note = MenuItem::with_id(app, "new_note", s.tray_new_note, true, None::<&str>)?;
    let quick_metric = MenuItem::with_id(app, "quick_metric", s.tray_quick_metric, true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", s.tray_pause, true, paused, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(app, "autostart", s.tray_autostart, true, autostart_on, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", s.tray_settings, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", s.tray_quit, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &new_note, &quick_metric, &separator, &pause, &autostart, &settings, &separator, &quit])?;
    *app.state::<TrayItems>().0.lock().expect("tray lock") = Some((pause, autostart));
    Ok(menu)
}

/// Rebuilds the menu after a language change (FR-SET-012).
pub fn refresh(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("pla"), menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn checked(app: &AppHandle, pick: impl Fn(&(CheckMenuItem<Wry>, CheckMenuItem<Wry>)) -> &CheckMenuItem<Wry>) -> bool {
    let items = app.state::<TrayItems>().0.lock().expect("tray lock").clone();
    items.as_ref().is_some_and(|i| pick(i).is_checked().unwrap_or(false))
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    app.manage(TrayItems::default());
    let menu = menu(app)?;
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
            "quick_metric" => {
                show_main(app);
                let _ = app.emit("open-quick-metric", ());
            }
            "settings" => {
                show_main(app);
                let _ = app.emit("open-settings", ());
            }
            "pause" => crate::commands::apply_pause(app, checked(app, |i| &i.0)),
            "autostart" => {
                let on = checked(app, |i| &i.1);
                if apply_autostart(app, on).is_err() {
                    set_autostart_checked(app, !on); // the check shows what Windows really has
                }
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
