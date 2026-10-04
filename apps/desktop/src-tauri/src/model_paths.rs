//! Which llama-server and which model PLA uses (model-manager spec § 5).

use std::path::{Path, PathBuf};

use pla_core::llm::ServerConfig;
use pla_core::settings::AppSettings;

/// The first candidate that is an existing file.
pub fn first_existing(candidates: impl IntoIterator<Item = Option<PathBuf>>) -> Option<PathBuf> {
    candidates.into_iter().flatten().find(|p| p.is_file())
}

/// Server: settings, `PLA_LLAMA_SERVER`, then the bundled copy. Model: settings, then `PLA_MODEL`.
pub fn server_config(settings: &AppSettings, env: &dyn Fn(&str) -> Option<PathBuf>, bundled_server: Option<PathBuf>) -> Option<ServerConfig> {
    let model = first_existing([settings.model_path.clone(), env("PLA_MODEL")])?;
    let bin = first_existing([settings.llama_server.clone(), env("PLA_LLAMA_SERVER"), bundled_server])?;
    Some(ServerConfig::new(bin, model))
}

/// The real environment, for `server_config`.
pub fn process_env(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from)
}

/// FR-MDL-019: what removing the active model does. Only a model PLA downloaded itself (a catalogue
/// id, inside PLA's models folder) is deleted; a file the user chose stays where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    Delete(PathBuf),
    Unlink,
}

pub fn removal(settings: &AppSettings, models_dir: Option<&Path>) -> Option<Removal> {
    let path = settings.model_path.as_ref()?;
    let ours = settings.model_id.is_some()
        && models_dir.zip(path.parent()).is_some_and(|(dir, parent)| same_dir(dir, parent));
    Some(if ours { Removal::Delete(path.clone()) } else { Removal::Unlink })
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.to_string_lossy().trim_end_matches(['/', '\\']).eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches(['/', '\\'])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn file(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, b"x").unwrap();
        p
    }

    #[test]
    fn settings_win_over_the_environment_which_wins_over_the_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        let (set_bin, env_bin, bundled) = (file(tmp.path(), "a.exe"), file(tmp.path(), "b.exe"), file(tmp.path(), "c.exe"));
        let (set_model, env_model) = (file(tmp.path(), "a.gguf"), file(tmp.path(), "b.gguf"));
        let env = |k: &str| match k {
            "PLA_LLAMA_SERVER" => Some(env_bin.clone()),
            "PLA_MODEL" => Some(env_model.clone()),
            _ => None,
        };
        let s = AppSettings { llama_server: Some(set_bin.clone()), model_path: Some(set_model.clone()), ..AppSettings::default() };
        let cfg = server_config(&s, &env, Some(bundled.clone())).unwrap();
        assert_eq!((cfg.server_bin, cfg.model), (set_bin, set_model));
        let cfg = server_config(&AppSettings::default(), &env, Some(bundled.clone())).unwrap();
        assert_eq!((cfg.server_bin, cfg.model), (env_bin, env_model.clone()));
        let only_model = |k: &str| (k == "PLA_MODEL").then(|| env_model.clone());
        assert_eq!(server_config(&AppSettings::default(), &only_model, Some(bundled.clone())).unwrap().server_bin, bundled);
    }

    #[test]
    fn only_a_model_pla_downloaded_is_deleted() {
        // FR-MDL-019, settings Review Focus 2
        let tmp = tempfile::tempdir().unwrap();
        let models = tmp.path().join("PLA").join("models");
        std::fs::create_dir_all(&models).unwrap();
        let ours = file(&models, "gemma.gguf");
        let theirs = file(tmp.path(), "benim.gguf");
        let downloaded = AppSettings { model_path: Some(ours.clone()), model_id: Some("gemma-4-e2b-it-q3km".into()), ..AppSettings::default() };
        assert_eq!(removal(&downloaded, Some(&models)), Some(Removal::Delete(ours.clone())));
        let chosen = AppSettings { model_path: Some(theirs.clone()), model_id: None, ..AppSettings::default() };
        assert_eq!(removal(&chosen, Some(&models)), Some(Removal::Unlink));
        let chosen_inside = AppSettings { model_path: Some(ours.clone()), model_id: None, ..AppSettings::default() };
        assert_eq!(removal(&chosen_inside, Some(&models)), Some(Removal::Unlink), "no catalogue id: the user's own file");
        let id_elsewhere = AppSettings { model_path: Some(theirs), model_id: Some("x".into()), ..AppSettings::default() };
        assert_eq!(removal(&id_elsewhere, Some(&models)), Some(Removal::Unlink), "outside PLA's folder");
        assert_eq!(removal(&downloaded, None), Some(Removal::Unlink), "no LOCALAPPDATA: never guess");
        assert_eq!(removal(&AppSettings::default(), Some(&models)), None);
    }

    #[test]
    fn missing_files_are_skipped_and_no_model_means_no_config() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = file(tmp.path(), "c.exe");
        let s = AppSettings { llama_server: Some(tmp.path().join("gone.exe")), model_path: Some(tmp.path().join("gone.gguf")), ..AppSettings::default() };
        assert_eq!(server_config(&s, &|_| None, Some(bundled.clone())), None, "no model file");
        let model = file(tmp.path(), "m.gguf");
        let s = AppSettings { llama_server: Some(tmp.path().join("gone.exe")), model_path: Some(model), ..AppSettings::default() };
        assert_eq!(server_config(&s, &|_| None, Some(bundled.clone())).unwrap().server_bin, bundled);
    }
}
