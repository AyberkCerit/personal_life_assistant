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

/// What the window is asked to do while its page is not listening yet (being made, or loading
/// after a reopen): it hears it once the page asks (`take_ui_events`), as an event emitted before
/// then would be lost. The flag says the page listens; both live under one lock, so nothing falls
/// between the page asking and the next request.
#[derive(Default)]
pub struct UiQueue(Mutex<(Vec<(String, serde_json::Value)>, bool)>);

impl UiQueue {
    /// Queues `event` unless the page listens; returns it back when it does (to emit at once).
    pub fn offer(&self, event: &str, payload: serde_json::Value) -> Option<(String, serde_json::Value)> {
        let mut q = self.0.lock().expect("ui queue lock");
        if q.1 {
            return Some((event.to_owned(), payload));
        }
        q.0.push((event.to_owned(), payload));
        None
    }

    /// The page listens now: what waited for it.
    pub fn take(&self) -> Vec<(String, serde_json::Value)> {
        let mut q = self.0.lock().expect("ui queue lock");
        q.1 = true;
        std::mem::take(&mut q.0)
    }

    /// The page is gone or being made again: queue until it asks.
    pub fn not_listening(&self) {
        self.0.lock().expect("ui queue lock").1 = false;
    }
}

/// The main window, made again when it is gone: in the tray PLA keeps no WebView at all (NFR-PERF-006,
/// M6 performance decision a). Returns whether it was there already.
pub fn show_main(app: &AppHandle) -> bool {
    let existed = app.get_webview_window("main").is_some();
    if !existed {
        if let Some(queue) = app.try_state::<UiQueue>() {
            queue.not_listening();
        }
    }
    let window = match app.get_webview_window("main") {
        Some(w) => Some(w),
        None => app.config().app.windows.first().and_then(|cfg| tauri::WebviewWindowBuilder::from_config(app, cfg).ok()).and_then(|b| b.build().ok()),
    };
    if let Some(window) = window {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    existed
}

/// Shows the window and tells it `event`: at once when its page listens, once it is up otherwise.
pub fn to_ui(app: &AppHandle, event: &str, payload: serde_json::Value) {
    show_main(app);
    let ready = match app.try_state::<UiQueue>() {
        Some(queue) => queue.offer(event, payload),
        None => Some((event.to_owned(), payload)),
    };
    if let Some((event, payload)) = ready {
        let _ = app.emit(&event, payload);
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
    app.manage(UiQueue::default());
    let menu = menu(app)?;
    TrayIconBuilder::with_id("pla")
        .icon(tray_icon())
        .tooltip("PLA")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => {
                show_main(app);
            }
            "new_note" => to_ui(app, "new-note", serde_json::Value::Null),
            "quick_metric" => to_ui(app, "open-quick-metric", serde_json::Value::Null),
            "settings" => to_ui(app, "open-settings", serde_json::Value::Null),
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
                let _ = show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::UiQueue;
    use serde_json::json;

    #[test]
    fn a_request_waits_until_the_page_listens() {
        // M6 final review: a click right after a reopen reached a page still loading and was lost
        let q = UiQueue::default();
        assert!(q.offer("open-note", json!(1)).is_none(), "no page yet");
        assert_eq!(q.take(), vec![("open-note".to_owned(), json!(1))]);
        assert_eq!(q.offer("new-note", json!(2)), Some(("new-note".to_owned(), json!(2))), "the page listens");
        assert!(q.take().is_empty());
        q.not_listening(); // to the tray, or made again
        assert!(q.offer("open-settings", json!(3)).is_none());
        assert_eq!(q.take().len(), 1);
    }
}
