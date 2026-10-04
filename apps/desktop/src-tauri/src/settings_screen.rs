//! Commands behind the settings screen's sections that are not plain settings (FR-SET-011):
//! backup status, CSV export, notification status, opening PLA's folders and About.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use pla_core::export::CsvStyle;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::commands::{app_root, AppState};

/// The region's list separator and decimal mark, as Windows reports them, made safe for CSV.
pub fn csv_style(list: &str, decimal: &str) -> CsvStyle {
    let usable = |c: &char| !matches!(c, '"' | '\r' | '\n') && !c.is_alphanumeric();
    let decimal = decimal.chars().next().filter(usable).unwrap_or('.');
    let separator = list.chars().next().filter(usable).unwrap_or(',');
    if separator == decimal {
        return CsvStyle { separator: ';', decimal: ',' };
    }
    CsvStyle { separator, decimal }
}

/// Windows shows PLA's notifications unless they are off for all apps or for PLA (FR-TSK-015).
pub fn notifications_on(all_apps: Option<u32>, this_app: Option<u32>) -> bool {
    all_apps != Some(0) && this_app != Some(0)
}

/// The folders the settings screen may open; the UI names one, the path is worked out here, so
/// no path from the UI is ever opened (settings Review Focus 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind {
    Vault,
    Backups,
    AppData,
    LocalData,
    ModelFolder,
    NotificationSettings,
}

pub struct Places {
    pub vault: Option<PathBuf>,
    pub app_root: Option<PathBuf>,
    pub local_root: Option<PathBuf>,
    pub model: Option<PathBuf>,
}

pub fn place(kind: PlaceKind, places: &Places) -> Option<OsString> {
    let path = match kind {
        PlaceKind::Vault => places.vault.clone(),
        PlaceKind::Backups => places.vault.as_ref().map(|v| v.join(".pla").join("backup")),
        PlaceKind::AppData => places.app_root.clone(),
        PlaceKind::LocalData => places.local_root.clone(),
        PlaceKind::ModelFolder => places.model.as_deref().and_then(Path::parent).map(Path::to_path_buf),
        PlaceKind::NotificationSettings => return Some(OsString::from("ms-settings:notifications")),
    };
    path.map(PathBuf::into_os_string)
}

/// The export files' names for today, in the UI language.
pub fn export_names(lang: &str, today: &str) -> (String, String) {
    let (tasks, metrics) = if lang == "tr" { ("gorevler", "olcumler") } else { ("tasks", "metrics") };
    (format!("PLA-{tasks}-{today}.csv"), format!("PLA-{metrics}-{today}.csv"))
}

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub name: &'static str,
    pub licence: &'static str,
    pub url: &'static str,
}

/// FR-SET-017: the third-party parts that ship with PLA. Kept by hand for now; a list generated
/// from every Rust and npm dependency belongs to packaging.
pub const COMPONENTS: &[Component] = &[
    Component { name: "llama.cpp", licence: "MIT", url: "https://github.com/ggml-org/llama.cpp" },
    Component { name: "LLVM OpenMP", licence: "Apache-2.0 WITH LLVM-exception", url: "https://openmp.llvm.org" },
    Component { name: "Tauri", licence: "MIT OR Apache-2.0", url: "https://tauri.app" },
    Component { name: "Svelte", licence: "MIT", url: "https://svelte.dev" },
    Component { name: "CodeMirror", licence: "MIT", url: "https://codemirror.net" },
    Component { name: "Lucide", licence: "ISC", url: "https://lucide.dev" },
    Component { name: "SQLite", licence: "Public domain", url: "https://sqlite.org" },
    Component { name: "rusqlite", licence: "MIT", url: "https://github.com/rusqlite/rusqlite" },
    Component { name: "serde", licence: "MIT OR Apache-2.0", url: "https://serde.rs" },
    Component { name: "chrono", licence: "MIT OR Apache-2.0", url: "https://github.com/chronotope/chrono" },
    Component { name: "ureq", licence: "MIT OR Apache-2.0", url: "https://github.com/algesten/ureq" },
];

/// One value of the user's Windows region (list separator, decimal mark).
#[cfg(windows)]
fn locale_info(kind: u32) -> String {
    use windows_sys::Win32::Globalization::GetLocaleInfoEx;
    let mut buf = [0u16; 16];
    // SAFETY: a null locale name means the user's default locale; the buffer length is passed.
    let len = unsafe { GetLocaleInfoEx(std::ptr::null(), kind, buf.as_mut_ptr(), buf.len() as i32) };
    if len <= 1 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize - 1])
}

pub fn region_style() -> CsvStyle {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Globalization::{LOCALE_SDECIMAL, LOCALE_SLIST};
        csv_style(&locale_info(LOCALE_SLIST), &locale_info(LOCALE_SDECIMAL))
    }
    #[cfg(not(windows))]
    {
        CsvStyle::default()
    }
}

#[cfg(windows)]
fn user_dword(subkey: &str, value: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (key, name) = (wide(subkey), wide(value));
    let mut data = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: NUL-terminated names, a DWORD-sized out buffer and its size.
    let status = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), (&mut data as *mut u32).cast(), &mut size) };
    (status == 0).then_some(data)
}

/// FR-TSK-015: whether Windows will show PLA's notifications.
pub fn windows_notifications_on() -> bool {
    #[cfg(windows)]
    {
        let all = user_dword(r"Software\Microsoft\Windows\CurrentVersion\PushNotifications", "ToastEnabled");
        let app_key = format!(r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\{}", crate::notify::app_id());
        notifications_on(all, user_dword(&app_key, "Enabled"))
    }
    #[cfg(not(windows))]
    {
        true
    }
}


#[derive(Serialize)]
pub struct BackupStatus {
    job: pla_core::jobs::JobStatus,
    backups: Vec<String>,
}

#[tauri::command]
pub fn backup_status(state: State<AppState>) -> Result<BackupStatus, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("no_vault")?;
    let job = pla_core::jobs::status(&session.db.lock().expect("db lock"), pla_core::jobs::DAILY).map_err(|e| e.to_string())?;
    Ok(BackupStatus { job, backups: pla_core::jobs::backups(&session.vault.root) })
}

/// NFR-SEC-008: tasks and metrics as two CSV files in the folder the user picked; returns their paths.
#[tauri::command(async)]
pub fn export_data(state: State<AppState>, dir: String, lang: String) -> Result<Vec<String>, String> {
    let dir = PathBuf::from(dir);
    if !dir.is_absolute() || !dir.is_dir() {
        return Err(format!("missing|{}|", dir.display()));
    }
    let style = region_style();
    let (tasks, metrics) = {
        let guard = state.session.lock().expect("session lock");
        let session = guard.as_ref().ok_or("no_vault")?;
        let db = session.db.lock().expect("db lock");
        let tasks = pla_core::export::tasks_csv(&db, &lang, style).map_err(|e| e.to_string())?;
        (tasks, pla_core::export::metrics_csv(&db, &lang, style).map_err(|e| e.to_string())?)
    };
    let (tasks_name, metrics_name) = export_names(&lang, &chrono::Local::now().format("%Y-%m-%d").to_string());
    let mut written = Vec::new();
    for (name, text) in [(tasks_name, tasks), (metrics_name, metrics)] {
        let path = dir.join(name);
        pla_core::fs_atomic::write_atomic(&path, text.as_bytes())
            .map_err(|e| format!("not_writable|{}|{}", dir.display(), crate::wizard::write_failure(&e)))?;
        written.push(path.to_string_lossy().into_owned());
    }
    Ok(written)
}

#[tauri::command]
pub fn notification_status() -> bool {
    windows_notifications_on()
}

/// Opens one of PLA's own places in Explorer (or Windows' notification settings).
#[tauri::command]
pub fn open_place(state: State<AppState>, kind: PlaceKind) -> Result<(), String> {
    let settings = pla_core::settings::load_settings(&app_root()?).map_err(|e| e.to_string())?.settings;
    let places = Places {
        vault: state.session.lock().expect("session lock").as_ref().map(|s| s.vault.root.clone()),
        app_root: app_root().ok(),
        local_root: std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("PLA")),
        model: settings.model_path,
    };
    let target = place(kind, &places).ok_or("not_found||")?;
    if kind != PlaceKind::NotificationSettings && !Path::new(&target).exists() {
        return Err(format!("not_found|{}|", Path::new(&target).display()));
    }
    let mut explorer = std::process::Command::new("explorer.exe");
    #[cfg(windows)]
    {
        // Quoted as one argument: explorer reads a bare comma (`C:\Notlar,2026`) as a switch
        // separator (settings final review M7). Windows paths cannot contain a quote.
        use std::os::windows::process::CommandExt;
        explorer.raw_arg(format!("\"{}\"", target.to_string_lossy()));
    }
    #[cfg(not(windows))]
    explorer.arg(&target);
    explorer.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Serialize)]
pub struct About {
    version: String,
    model: Option<String>,
    model_licence: Option<&'static str>,
    components: &'static [Component],
}

/// FR-SET-017: version, active model and the bundled parts' licences.
#[tauri::command]
pub fn about_info(app: AppHandle) -> Result<About, String> {
    let settings = pla_core::settings::load_settings(&app_root()?).map_err(|e| e.to_string())?.settings;
    let entry = settings.model_id.as_deref().and_then(pla_core::models::catalog::by_id);
    let model = settings.model_path.as_deref().and_then(|p| crate::commands::describe_model(p, settings.model_id.as_deref())).map(|m| m.name);
    Ok(About { version: app.package_info().version.to_string(), model_licence: model.as_ref().and(entry.map(|e| e.licence)), model, components: COMPONENTS })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turkish_region_gets_semicolons_and_decimal_commas() {
        assert_eq!(csv_style(";", ","), CsvStyle { separator: ';', decimal: ',' });
        assert_eq!(csv_style(",", "."), CsvStyle { separator: ',', decimal: '.' });
        assert_eq!(csv_style("", ""), CsvStyle { separator: ',', decimal: '.' }, "nothing reported");
        assert_eq!(csv_style(",", ","), CsvStyle { separator: ';', decimal: ',' }, "a separator equal to the decimal mark would split numbers");
        assert_eq!(csv_style("\"", "."), CsvStyle { separator: ',', decimal: '.' }, "a quote cannot separate");
    }

    #[test]
    fn notifications_are_off_when_windows_says_so_for_all_or_for_pla() {
        assert!(notifications_on(None, None), "never touched: on");
        assert!(notifications_on(Some(1), Some(1)));
        assert!(!notifications_on(Some(0), None), "all apps off");
        assert!(!notifications_on(None, Some(0)), "PLA off");
    }

    #[test]
    fn only_plas_own_places_can_be_opened() {
        let places = Places {
            vault: Some(PathBuf::from(r"C:\Kasa")),
            app_root: Some(PathBuf::from(r"C:\Users\a\AppData\Roaming\PLA")),
            local_root: Some(PathBuf::from(r"C:\Users\a\AppData\Local\PLA")),
            model: Some(PathBuf::from(r"D:\Modeller\gemma.gguf")),
        };
        assert_eq!(place(PlaceKind::Vault, &places), Some(OsString::from(r"C:\Kasa")));
        assert_eq!(place(PlaceKind::Backups, &places), Some(Path::new(r"C:\Kasa").join(".pla").join("backup").into_os_string()));
        assert_eq!(place(PlaceKind::ModelFolder, &places), Some(OsString::from(r"D:\Modeller")));
        assert_eq!(place(PlaceKind::NotificationSettings, &places), Some(OsString::from("ms-settings:notifications")));
        let none = Places { vault: None, app_root: None, local_root: None, model: None };
        assert_eq!(place(PlaceKind::Vault, &none), None);
        assert!(serde_json::from_str::<PlaceKind>(r#""C:\\Windows""#).is_err(), "a path is not a place");
    }

    #[test]
    fn export_files_are_named_for_the_day_and_language() {
        assert_eq!(export_names("tr", "2026-10-05"), ("PLA-gorevler-2026-10-05.csv".into(), "PLA-olcumler-2026-10-05.csv".into()));
        assert_eq!(export_names("en", "2026-10-05"), ("PLA-tasks-2026-10-05.csv".into(), "PLA-metrics-2026-10-05.csv".into()));
    }

    #[test]
    fn about_lists_the_bundled_parts_with_licences() {
        // FR-SET-017
        for name in ["llama.cpp", "Tauri", "Svelte", "SQLite"] {
            assert!(COMPONENTS.iter().any(|c| c.name == name && !c.licence.is_empty()), "{name}");
        }
    }
}
