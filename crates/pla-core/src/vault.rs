//! Opening a vault: system folders, `.pla/config` and the per-vault data folder (FR-VLT-006, -008, E-D1, E-D17).

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_atomic::write_atomic;

const CONFIG_PATH: &str = ".pla/config";
const CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultConfig {
    pub vault_id: String,
    pub schema_version: u32,
    pub folders: Folders,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self { vault_id: String::new(), schema_version: CONFIG_SCHEMA_VERSION, folders: Folders::default() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Folders {
    pub daily: String,
    pub inbox: String,
    pub notes: String,
    pub reports: String,
    pub attachments: String,
    pub templates: String,
}

impl Default for Folders {
    fn default() -> Self {
        Self {
            daily: "daily".into(),
            inbox: "inbox".into(),
            notes: "notes".into(),
            reports: "reports".into(),
            attachments: "attachments".into(),
            templates: "templates".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Vault {
    pub root: PathBuf,
    pub config: VaultConfig,
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("not a folder: {0}")]
    NotADirectory(PathBuf),
    #[error("network folders are not supported: {0}")]
    NetworkPathUnsupported(PathBuf),
    #[error("cannot read .pla/config: {0}")]
    CorruptConfig(String),
    #[error("folder must be a relative path inside the vault: {0:?}")]
    InvalidFolder(String),
    #[error("invalid vault_id: {0:?}")]
    InvalidVaultId(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn open_vault(root: &Path) -> Result<Vault, VaultError> {
    if is_network_path(root) {
        return Err(VaultError::NetworkPathUnsupported(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(VaultError::NotADirectory(root.to_path_buf()));
    }
    std::fs::create_dir_all(root.join(".pla/backup"))?;

    let config_path = root.join(CONFIG_PATH);
    let mut config = if config_path.exists() {
        let text = std::fs::read_to_string(&config_path)?;
        toml::from_str::<VaultConfig>(&text).map_err(|e| VaultError::CorruptConfig(e.to_string()))?
    } else {
        VaultConfig::default()
    };

    if config.vault_id.is_empty() {
        config.vault_id = new_vault_id();
        save_config(root, &config)?;
    } else if !is_valid_vault_id(&config.vault_id) {
        return Err(VaultError::InvalidVaultId(config.vault_id));
    }

    let f = &config.folders;
    for dir in [&f.inbox, &f.daily, &f.notes, &f.reports, &f.attachments, &f.templates] {
        if !is_valid_folder(dir) {
            return Err(VaultError::InvalidFolder(dir.clone()));
        }
    }
    for dir in [&f.inbox, &f.daily, &f.notes, &f.attachments, &f.templates] {
        std::fs::create_dir_all(root.join(dir))?;
    }
    std::fs::create_dir_all(root.join(&f.reports).join("weekly"))?;

    Ok(Vault { root: root.to_path_buf(), config })
}

/// What the first-run wizard found at a folder the user is about to make the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderState {
    /// Does not exist yet; it will be created (FR-SET-006).
    Missing,
    /// Exists with nothing visible in it (hidden entries such as `.obsidian` or `desktop.ini` do not count).
    Empty,
    /// Holds Markdown notes; opened as it is, nothing moved or changed (FR-SET-005).
    Notes,
    /// Holds files but no notes (or could not be listed).
    Other,
    /// A network (UNC) path, which PLA does not support (FR-SET-007).
    Network,
    /// A file, not a folder.
    NotFolder,
    /// Not a full path (`Notlar`, `C:Notlar`): it would depend on PLA's working folder.
    Relative,
    /// Inside PLA's own data folders, which the uninstaller may delete (NFR-SEC-010).
    AppData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FolderCheck {
    pub state: FolderState,
    /// Markdown files found (hidden entries and links skipped).
    pub md_files: usize,
    /// The count stopped at a limit, so there may be more.
    pub more: bool,
}

const MD_COUNT_LIMIT: usize = 10_000;
/// Entries looked at before the count gives up, so a folder like `C:\` answers quickly.
const ENTRY_LIMIT: usize = 50_000;

/// Looks at a folder without changing anything (first-run wizard, FR-SET-005…007).
pub fn inspect_folder(path: &Path) -> FolderCheck {
    let state = folder_state(path);
    if state != FolderState::Other {
        return FolderCheck { state, md_files: 0, more: false };
    }
    let (md_files, more) = count_notes(path, MD_COUNT_LIMIT, ENTRY_LIMIT);
    FolderCheck { state: if md_files > 0 { FolderState::Notes } else { FolderState::Other }, md_files, more }
}

/// `inspect_folder` without counting notes: a folder with visible entries is `Other`.
pub fn folder_state(path: &Path) -> FolderState {
    if is_network_path(path) {
        return FolderState::Network;
    }
    if !path.is_absolute() {
        return FolderState::Relative;
    }
    if inside_app_data(path, &app_data_roots()) {
        return FolderState::AppData;
    }
    if !path.exists() {
        return FolderState::Missing;
    }
    if !path.is_dir() {
        return FolderState::NotFolder;
    }
    match std::fs::read_dir(path) {
        Ok(entries) => {
            let mut entries = entries;
            if entries.any(|e| e.is_ok_and(|e| !is_hidden(&e))) {
                FolderState::Other
            } else {
                FolderState::Empty
            }
        }
        Err(_) => FolderState::Other, // an unreadable folder promises nothing; the write check reports it
    }
}

/// Dot-named, or marked hidden/system by Windows (`desktop.ini`, `Thumbs.db`).
fn is_hidden(entry: &std::fs::DirEntry) -> bool {
    if entry.file_name().to_string_lossy().starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN_OR_SYSTEM: u32 = 0x2 | 0x4;
        if entry.metadata().is_ok_and(|m| m.file_attributes() & HIDDEN_OR_SYSTEM != 0) {
            return true;
        }
    }
    false
}

/// Counts `.md` files up to `limit`, looking at no more than `max_entries` entries and never following
/// links or junctions. Returns the count and whether a limit cut it short.
fn count_notes(dir: &Path, limit: usize, max_entries: usize) -> (usize, bool) {
    let (mut count, mut seen) = (0, 0);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            seen += 1;
            if seen > max_entries {
                return (count, true);
            }
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_symlink() || is_hidden(&e) {
                continue;
            }
            if kind.is_dir() {
                stack.push(e.path());
            } else if e.file_name().to_string_lossy().to_ascii_lowercase().ends_with(".md") {
                count += 1;
                if count >= limit {
                    return (count, true);
                }
            }
        }
    }
    (count, false)
}

/// `<app_root>/vaults/<vault_id>/` — where this vault's pla.db and cache.db live.
pub fn data_dir(app_root: &Path, vault_id: &str) -> Result<PathBuf, VaultError> {
    if !is_valid_vault_id(vault_id) {
        return Err(VaultError::InvalidVaultId(vault_id.to_owned()));
    }
    Ok(app_root.join("vaults").join(vault_id))
}

/// `%APPDATA%/PLA` on Windows.
pub fn default_app_root() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("PLA"))
}

/// PLA's own folders: `%APPDATA%\PLA` (settings, databases) and `%LOCALAPPDATA%\PLA` (models). The
/// uninstaller may delete both (NFR-SEC-010), so neither may hold the vault.
pub fn app_data_roots() -> Vec<PathBuf> {
    ["APPDATA", "LOCALAPPDATA"].iter().filter_map(|k| std::env::var_os(k)).map(|p| PathBuf::from(p).join("PLA")).collect()
}

/// Whether `path` is one of `roots` or inside one, compared as Windows does (case-insensitively).
pub fn inside_app_data(path: &Path, roots: &[PathBuf]) -> bool {
    let parts = |p: &Path| p.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect::<Vec<_>>();
    let path = parts(path);
    roots.iter().any(|root| {
        let root = parts(root);
        !root.is_empty() && path.starts_with(&root)
    })
}

const ROOT_MARKER: &str = "vault_root";

/// This vault's data folder (E-D1), created if missing. The folder remembers which vault folder it
/// belongs to: if that folder still exists with the same `vault_id`, this vault is a copy and gets a
/// new identity (and an empty database); if it no longer exists, the vault was moved and keeps its data.
pub fn resolve_data_dir(app_root: &Path, vault: &mut Vault) -> Result<PathBuf, VaultError> {
    let here = std::fs::canonicalize(&vault.root)?.to_string_lossy().into_owned();
    let dir = data_dir(app_root, &vault.config.vault_id)?;
    let marker = dir.join(ROOT_MARKER);
    let recorded = match std::fs::read_to_string(&marker) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    match recorded {
        Some(other) if other == here => Ok(dir),
        Some(other) if belongs_to(Path::new(&other), &vault.config.vault_id) => {
            vault.config.vault_id = new_vault_id();
            save_config(&vault.root, &vault.config)?;
            let dir = data_dir(app_root, &vault.config.vault_id)?;
            std::fs::create_dir_all(&dir)?;
            write_atomic(&dir.join(ROOT_MARKER), here.as_bytes())?;
            Ok(dir)
        }
        _ => {
            std::fs::create_dir_all(&dir)?;
            write_atomic(&marker, here.as_bytes())?;
            Ok(dir)
        }
    }
}

fn belongs_to(root: &Path, vault_id: &str) -> bool {
    std::fs::read_to_string(root.join(CONFIG_PATH))
        .ok()
        .and_then(|text| toml::from_str::<VaultConfig>(&text).ok())
        .is_some_and(|config| config.vault_id == vault_id)
}

fn new_vault_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn save_config(root: &Path, config: &VaultConfig) -> Result<(), VaultError> {
    let text = toml::to_string(config).map_err(|e| VaultError::CorruptConfig(e.to_string()))?;
    write_atomic(&root.join(CONFIG_PATH), text.as_bytes())?;
    Ok(())
}
fn is_valid_vault_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A system folder is a non-empty relative path inside the vault that is not hidden,
/// so it can never reach outside the vault, `.obsidian/` (FR-VLT-004) or `.pla/`.
fn is_valid_folder(dir: &str) -> bool {
    let mut parts = Path::new(dir).components().peekable();
    let first_visible = matches!(parts.peek(), Some(Component::Normal(p)) if !p.to_string_lossy().starts_with('.'));
    first_visible && parts.all(|c| matches!(c, Component::Normal(_)))
}

fn is_network_path(path: &Path) -> bool {
    use std::path::Prefix;
    match path.components().next() {
        Some(Component::Prefix(p)) => match p.kind() {
            Prefix::UNC(..) | Prefix::VerbatimUNC(..) => true,
            // `\\.\UNC\…` and a lowercase `\\?\unc\…` reach the network too
            Prefix::DeviceNS(name) | Prefix::Verbatim(name) => name.eq_ignore_ascii_case("UNC"),
            _ => false,
        },
        _ => {
            let s = path.as_os_str().to_string_lossy();
            s.starts_with(r"\\") || s.starts_with("//")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy_config(from: &Path, to: &Path) {
        std::fs::create_dir_all(to.join(".pla")).unwrap();
        std::fs::copy(from.join(".pla/config"), to.join(".pla/config")).unwrap();
    }

    #[test]
    fn same_folder_keeps_its_data_dir() {
        let app = tempfile::tempdir().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let mut v = open_vault(tmp.path()).unwrap();
        let first = resolve_data_dir(app.path(), &mut v).unwrap();
        let mut again = open_vault(tmp.path()).unwrap();
        assert_eq!(resolve_data_dir(app.path(), &mut again).unwrap(), first);
        assert!(first.join("vault_root").is_file());
    }

    #[test]
    fn copied_vault_gets_its_own_identity() {
        // Review Focus 4: a backup copy must not write into the original's database.
        let app = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let original = parent.path().join("Kasa");
        let copy = parent.path().join("Kasa - Kopya");
        std::fs::create_dir_all(&original).unwrap();
        let mut a = open_vault(&original).unwrap();
        let a_dir = resolve_data_dir(app.path(), &mut a).unwrap();

        std::fs::create_dir_all(&copy).unwrap();
        copy_config(&original, &copy);
        let mut b = open_vault(&copy).unwrap();
        assert_eq!(b.config.vault_id, a.config.vault_id, "precondition: copied config");
        let b_dir = resolve_data_dir(app.path(), &mut b).unwrap();

        assert_ne!(b.config.vault_id, a.config.vault_id);
        assert_ne!(b_dir, a_dir);
        let on_disk = std::fs::read_to_string(copy.join(".pla/config")).unwrap();
        assert!(on_disk.contains(&b.config.vault_id), "new id must be saved in the copy");
        let mut a_again = open_vault(&original).unwrap();
        assert_eq!(resolve_data_dir(app.path(), &mut a_again).unwrap(), a_dir, "original keeps its data");
    }

    #[test]
    fn moved_vault_keeps_its_data() {
        let app = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let before = parent.path().join("eski yer");
        let after = parent.path().join("yeni yer");
        std::fs::create_dir_all(&before).unwrap();
        let mut v = open_vault(&before).unwrap();
        let dir = resolve_data_dir(app.path(), &mut v).unwrap();
        let id = v.config.vault_id.clone();

        std::fs::rename(&before, &after).unwrap();
        let mut moved = open_vault(&after).unwrap();
        assert_eq!(resolve_data_dir(app.path(), &mut moved).unwrap(), dir);
        assert_eq!(moved.config.vault_id, id);
        let recorded = std::fs::read_to_string(dir.join("vault_root")).unwrap();
        assert!(recorded.contains("yeni yer"), "binding must follow the move: {recorded}");
    }

    const SYSTEM_DIRS: [&str; 8] = ["inbox", "daily", "notes", "reports/weekly", "attachments", "templates", ".pla", ".pla/backup"];

    #[test]
    fn new_vault_gets_folders_and_a_persistent_id() {
        let tmp = tempfile::tempdir().unwrap();
        // Review Focus 2: Turkish characters, spaces and emoji in the path
        let root = tmp.path().join("🏰 Kişisel Kasa ğüşıöç");
        std::fs::create_dir(&root).unwrap();

        let v = open_vault(&root).unwrap();
        for d in SYSTEM_DIRS {
            assert!(root.join(d).is_dir(), "{d} missing");
        }
        assert_eq!(v.config.vault_id.len(), 32);
        assert!(!root.join(".obsidian").exists(), "must never create .obsidian");

        let again = open_vault(&root).unwrap();
        assert_eq!(again.config.vault_id, v.config.vault_id);
    }

    #[test]
    fn custom_folder_paths_are_respected_and_kept() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".pla")).unwrap();
        std::fs::write(tmp.path().join(".pla/config"), "[folders]\ndaily = \"Günlük\"\n").unwrap();

        let v = open_vault(tmp.path()).unwrap();
        assert!(tmp.path().join("Günlük").is_dir());
        assert!(!tmp.path().join("daily").exists());
        let written = std::fs::read_to_string(tmp.path().join(".pla/config")).unwrap();
        assert!(written.contains("Günlük"));
        assert!(written.contains(&v.config.vault_id));
    }

    #[test]
    fn folders_must_stay_inside_the_vault() {
        for bad in ["../outside", "C:/Users/x/Desktop", "/abs", ".obsidian", ".pla", "", "notes/../../x"] {
            let tmp = tempfile::tempdir().unwrap();
            std::fs::create_dir(tmp.path().join(".pla")).unwrap();
            let config = format!("vault_id = \"0123456789abcdef0123456789abcdef\"
[folders]
daily = {bad:?}
");
            std::fs::write(tmp.path().join(".pla/config"), &config).unwrap();
            assert!(matches!(open_vault(tmp.path()), Err(VaultError::InvalidFolder(_))), "accepted {bad:?}");
        }
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".pla")).unwrap();
        std::fs::write(tmp.path().join(".pla/config"), "[folders]
notes = \"Notlar/Arşiv\"
").unwrap();
        open_vault(tmp.path()).unwrap();
        assert!(tmp.path().join("Notlar/Arşiv").is_dir());
    }

    #[test]
    fn corrupt_config_is_reported_and_left_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".pla")).unwrap();
        std::fs::write(tmp.path().join(".pla/config"), "vault_id = [broken").unwrap();
        assert!(matches!(open_vault(tmp.path()), Err(VaultError::CorruptConfig(_))));
        assert_eq!(std::fs::read_to_string(tmp.path().join(".pla/config")).unwrap(), "vault_id = [broken");
    }

    #[test]
    fn vault_id_cannot_escape_the_vaults_folder() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".pla")).unwrap();
        std::fs::write(tmp.path().join(".pla/config"), "vault_id = \"../../evil\"\n").unwrap();
        assert!(matches!(open_vault(tmp.path()), Err(VaultError::InvalidVaultId(_))));
        assert!(matches!(data_dir(Path::new("C:/app"), "../x"), Err(VaultError::InvalidVaultId(_))));
    }

    #[test]
    fn a_folder_inside_plas_own_data_cannot_be_the_vault() {
        // NFR-SEC-010: the uninstaller may delete these folders; a vault there would go with them
        let roots = [PathBuf::from(r"C:\Users\a\AppData\Roaming\PLA"), PathBuf::from(r"C:\Users\a\AppData\Local\PLA")];
        for p in [r"C:\Users\a\AppData\Roaming\PLA", r"c:\users\A\appdata\roaming\pla\Notlar", r"C:\Users\a\AppData\Local\PLA\models\x"] {
            assert!(inside_app_data(Path::new(p), &roots), "{p}");
        }
        for p in [r"C:\Users\a\AppData\Roaming\PLA Notlar", r"C:\Users\a\AppData\Roaming", r"D:\PLA", r"C:\Users\a\Documents\PLA Vault"] {
            assert!(!inside_app_data(Path::new(p), &roots), "{p}");
        }
    }

    #[test]
    fn data_dir_is_per_vault() {
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(data_dir(Path::new("C:/app/PLA"), id).unwrap(), Path::new("C:/app/PLA").join("vaults").join(id));
    }

    #[test]
    fn rejects_missing_folder_and_network_paths() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(open_vault(&tmp.path().join("yok")), Err(VaultError::NotADirectory(_))));
        assert!(matches!(open_vault(Path::new(r"\\server\share\vault")), Err(VaultError::NetworkPathUnsupported(_))));
        assert!(matches!(open_vault(Path::new(r"\\?\UNC\server\share\vault")), Err(VaultError::NetworkPathUnsupported(_))));
    }

    #[test]
    fn inspects_a_folder_before_it_becomes_the_vault() {
        // First-run wizard: FR-SET-005/006/007
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("Yeni Kasa ğüş");
        assert_eq!(inspect_folder(&missing), FolderCheck { state: FolderState::Missing, md_files: 0, more: false });

        let empty = tmp.path().join("bos");
        std::fs::create_dir_all(empty.join(".obsidian")).unwrap();
        assert_eq!(inspect_folder(&empty).state, FolderState::Empty, "hidden folders do not count");

        let notes = tmp.path().join("obsidian");
        std::fs::create_dir_all(notes.join("Projeler")).unwrap();
        std::fs::create_dir_all(notes.join(".trash")).unwrap();
        std::fs::write(notes.join("Günlük.md"), "x").unwrap();
        std::fs::write(notes.join("Projeler/PLA.MD"), "x").unwrap();
        std::fs::write(notes.join(".trash/silinen.md"), "x").unwrap();
        assert_eq!(inspect_folder(&notes), FolderCheck { state: FolderState::Notes, md_files: 2, more: false });

        let other = tmp.path().join("fotograflar");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("a.jpg"), "x").unwrap();
        assert_eq!(inspect_folder(&other).state, FolderState::Other);

        let file = tmp.path().join("dosya.txt");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(inspect_folder(&file).state, FolderState::NotFolder);

        assert_eq!(inspect_folder(Path::new(r"\\sunucu\paylasim\Kasa")).state, FolderState::Network);
    }

    #[test]
    fn network_paths_are_recognised_in_every_spelling() {
        // final review I1: the wizard's text field accepts whatever the user types
        for p in [r"\\sunucu\paylasim", "//sunucu/paylasim/Kasa", r"\/sunucu\paylasim", r"\\.\UNC\sunucu\paylasim\x", r"\\?\unc\sunucu\paylasim", r"\\?\UNC\sunucu\paylasim"] {
            assert!(is_network_path(Path::new(p)), "{p}");
            assert_eq!(inspect_folder(Path::new(p)).state, FolderState::Network, "{p}");
        }
        for p in [r"C:\Notlar", r"\\?\C:\Notlar", r"\\.\C:\Notlar"] {
            assert!(!is_network_path(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn relative_paths_are_no_place_for_the_vault() {
        // final review I2: they would resolve against the process's working folder
        for p in ["Notlar", r"C:Notlar", "", r"\Notlar"] {
            assert_eq!(inspect_folder(Path::new(p)).state, FolderState::Relative, "{p:?}");
        }
    }

    #[test]
    fn counting_notes_stops_at_its_limits() {
        // final review I3: typing `C:\` must not walk the whole disk
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..5 {
            std::fs::create_dir_all(tmp.path().join(format!("d{i}"))).unwrap();
            std::fs::write(tmp.path().join(format!("d{i}/n.md")), "x").unwrap();
        }
        assert_eq!(count_notes(tmp.path(), 3, 1000), (3, true), "note limit");
        let (found, more) = count_notes(tmp.path(), 100, 4);
        assert!(more && found < 5, "entry limit: {found}");
        assert_eq!(count_notes(tmp.path(), 100, 1000), (5, false));
    }
}
