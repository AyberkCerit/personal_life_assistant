//! While PLA sits in the tray, its WebView is asked to use little memory (nl-quality RAM plan § 3):
//! WebView2 then trims what it can rebuild, and gets it back when the window shows again.

/// Low while hidden, normal while shown. Nothing happens on a WebView2 too old to know the setting.
#[cfg(windows)]
pub fn set_low(window: &tauri::WebviewWindow, low: bool) {
    let _ = window.with_webview(move |webview| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };
        use windows_core::Interface;
        let level = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
        // SAFETY: COM calls on the webview's own thread (with_webview runs there), on live objects.
        unsafe {
            if let Ok(core) = webview.controller().CoreWebView2() {
                if let Ok(core) = core.cast::<ICoreWebView2_19>() {
                    let _ = core.SetMemoryUsageTargetLevel(level);
                }
            }
        }
    });
}

#[cfg(not(windows))]
pub fn set_low(_window: &tauri::WebviewWindow, _low: bool) {}
