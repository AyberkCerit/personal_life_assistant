//! Language models already on this PC (FR-MDL-008): the folders where PLA itself, Ollama,
//! LM Studio, the Hugging Face cache, GPT4All, Jan and llama.cpp keep GGUF files. Only these known
//! folders are looked at, never the whole disk, and each file only by its header (a few KB), so a
//! scan takes milliseconds. Nothing is copied: a chosen file is used where it is.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Pla,
    Ollama,
    LmStudio,
    HuggingFace,
    Gpt4All,
    Jan,
    LlamaCpp,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FoundModel {
    pub path: PathBuf,
    pub name: String,
    pub source: Source,
    pub size: u64,
    /// `general.architecture`, e.g. `gemma3`, `llama`, `qwen2`.
    pub architecture: Option<String>,
    /// `general.size_label`, e.g. `4B`.
    pub size_label: Option<String>,
    /// The catalogue model, whole, in PLA's own models folder.
    pub recommended: bool,
}

/// The folders to look in, in order, from the environment and the home folder (both passed in so
/// tests stay away from the real ones).
pub fn known_places(env: &dyn Fn(&str) -> Option<OsString>, home: Option<&Path>, models_dir: Option<&Path>) -> Vec<(Source, PathBuf)> {
    let var = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut places = Vec::new();
    places.extend(models_dir.map(|d| (Source::Pla, d.to_path_buf())));
    places.extend(var("OLLAMA_MODELS").map(|p| (Source::Ollama, p)));
    places.extend(var("HF_HUB_CACHE").map(|p| (Source::HuggingFace, p)));
    places.extend(var("HF_HOME").map(|p| (Source::HuggingFace, p.join("hub"))));
    places.extend(var("LLAMA_CACHE").map(|p| (Source::LlamaCpp, p)));
    if let Some(home) = home {
        places.push((Source::Ollama, home.join(".ollama").join("models")));
        places.push((Source::LmStudio, home.join(".lmstudio").join("models")));
        places.push((Source::LmStudio, home.join(".cache").join("lm-studio").join("models")));
        places.push((Source::HuggingFace, home.join(".cache").join("huggingface").join("hub")));
        places.push((Source::Jan, home.join("jan").join("models")));
    }
    if cfg!(windows) {
        if let Some(local) = var("LOCALAPPDATA") {
            places.push((Source::Gpt4All, local.join("nomic.ai").join("GPT4All")));
            places.push((Source::LlamaCpp, local.join("llama.cpp")));
        }
        if let Some(roaming) = var("APPDATA") {
            places.push((Source::Jan, roaming.join("Jan").join("data").join("llamacpp").join("models")));
        }
    } else if let Some(home) = home {
        let data = var("XDG_DATA_HOME").filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".local").join("share"));
        let cache = var("XDG_CACHE_HOME").filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".cache"));
        places.push((Source::Gpt4All, data.join("nomic.ai").join("GPT4All")));
        places.push((Source::Jan, data.join("Jan").join("data").join("llamacpp").join("models")));
        places.push((Source::LlamaCpp, cache.join("llama.cpp")));
        // Ollama's Linux installer runs it as its own user; readable only if the user allowed it
        places.push((Source::Ollama, PathBuf::from("/usr/share/ollama/.ollama/models")));
    }
    places
}

/// The places of this PC: the real environment and home folder.
pub fn this_pc() -> Vec<(Source, PathBuf)> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).filter(|h| !h.is_empty()).map(PathBuf::from);
    known_places(&|k| std::env::var_os(k), home.as_deref(), super::models_dir().as_deref())
}

/// Chat models in `places`: the recommended one first, then by name. Embedding models, vision
/// projectors and the later parts of split files are left out; unreadable folders are skipped.
pub fn scan(places: &[(Source, PathBuf)]) -> Vec<FoundModel> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for (source, dir) in places {
        let files = match source {
            Source::Ollama => ollama_models(dir),
            _ => gguf_files(dir).into_iter().map(|p| (p, None)).collect(),
        };
        for (path, ollama_name) in files {
            let key = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !seen.insert(key) {
                continue;
            }
            if let Some(model) = inspect(&path, *source, ollama_name) {
                found.push(model);
            }
        }
    }
    found.sort_by(|a, b| b.recommended.cmp(&a.recommended).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    found
}

/// Enough for any real model library; a huge folder (e.g. a whole Hugging Face cache of datasets)
/// stops the walk instead of the window.
const MAX_ENTRIES: usize = 20_000;
const MAX_DEPTH: usize = 6;

/// `*.gguf` files under `dir` (symbolic links followed: the Hugging Face cache is made of them).
fn gguf_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    let mut visited = 0;
    while let Some((d, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            visited += 1;
            if visited > MAX_ENTRIES {
                return out;
            }
            let path = e.path();
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            if meta.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
            } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("gguf")) {
                out.push(path);
            }
        }
    }
    out
}

/// Ollama keeps models as nameless blobs; its manifests say which blob is which `model:tag`.
fn ollama_models(dir: &Path) -> Vec<(PathBuf, Option<String>)> {
    let manifests = dir.join("manifests");
    let mut out = Vec::new();
    let mut stack = vec![(manifests.clone(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let Some(blob) = std::fs::read(&path).ok().and_then(|b| model_blob(&b)) else { continue };
            let blob = dir.join("blobs").join(blob.replace(':', "-"));
            out.push((blob, ollama_name(&manifests, &path)));
        }
    }
    out
}

/// The digest of a manifest's model layer.
fn model_blob(manifest: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(manifest).ok()?;
    let layers = v.get("layers")?.as_array()?;
    let layer = layers.iter().find(|l| l.get("mediaType").and_then(|m| m.as_str()) == Some("application/vnd.ollama.image.model"))?;
    let digest = layer.get("digest")?.as_str()?;
    // only `sha256:<hex>`: the name becomes a path
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| digest.to_owned())
}

/// `manifests/<registry>/<namespace>/<model>/<tag>` → `model:tag` (`namespace/model:tag` outside
/// the official library), as `ollama list` shows it.
fn ollama_name(manifests: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(manifests).ok()?;
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    match parts.as_slice() {
        [_, ns, model, tag] if ns == "library" => Some(format!("{model}:{tag}")),
        [_, ns, model, tag] => Some(format!("{ns}/{model}:{tag}")),
        _ => None,
    }
}

fn inspect(path: &Path, source: Source, ollama_name: Option<String>) -> Option<FoundModel> {
    let size = std::fs::metadata(path).ok().filter(|m| m.is_file())?.len();
    let file_name = path.file_name()?.to_string_lossy().into_owned();
    if is_later_split_part(&file_name) {
        return None;
    }
    let header = read_header(path).ok()?;
    if header.is_embedding || header.architecture.as_deref() == Some("clip") {
        return None;
    }
    let entry = super::catalog::recommended();
    let recommended = source == Source::Pla && file_name.eq_ignore_ascii_case(entry.file_name) && size == entry.size;
    let name = if recommended {
        entry.name.to_owned()
    } else {
        ollama_name.or(header.name.clone()).unwrap_or_else(|| file_name.trim_end_matches(".gguf").to_owned())
    };
    Some(FoundModel { path: path.to_path_buf(), name, source, size, architecture: header.architecture, size_label: header.size_label, recommended })
}

/// `x-00002-of-00003.gguf`: llama-server opens a split model from its first part.
fn is_later_split_part(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix(".gguf") else { return false };
    let Some((before, total)) = stem.rsplit_once("-of-") else { return false };
    let part = before.rsplit('-').next().unwrap_or("");
    !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) && total.bytes().all(|b| b.is_ascii_digit()) && part.trim_start_matches('0') != "1"
}

#[derive(Debug, Default, PartialEq)]
pub struct Header {
    pub architecture: Option<String>,
    pub name: Option<String>,
    pub size_label: Option<String>,
    /// Pooling (`<arch>.pooling_type`) or an encoder architecture: makes vectors, cannot chat.
    pub is_embedding: bool,
}

/// No real key or metadata value comes near these; a damaged file stops here.
const MAX_STRING: u64 = 1 << 20;
const MAX_KV: u64 = 100_000;

/// The metadata at the start of a GGUF file (v2/v3), read up to the tokenizer's keys, which come
/// after everything needed here and make up most of the header.
pub fn read_header(path: &Path) -> std::io::Result<Header> {
    let mut r = BufReader::new(File::open(path)?);
    let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, "not a GGUF file");
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    let version = read_u32(&mut r)?;
    if &magic != b"GGUF" || !(2..=3).contains(&version) {
        return Err(bad());
    }
    let _tensors = read_u64(&mut r)?;
    let kv_count = read_u64(&mut r)?;
    if kv_count > MAX_KV {
        return Err(bad());
    }
    let mut h = Header::default();
    for _ in 0..kv_count {
        let key = read_string(&mut r)?;
        if key.starts_with("tokenizer.") {
            break;
        }
        let ty = read_u32(&mut r)?;
        match key.as_str() {
            "general.architecture" | "general.name" | "general.size_label" if ty == STRING => {
                let v = read_string(&mut r)?;
                match key.as_str() {
                    "general.architecture" => h.architecture = Some(v),
                    "general.name" => h.name = Some(v),
                    _ => h.size_label = Some(v),
                }
            }
            k if k.ends_with(".pooling_type") => {
                h.is_embedding = true;
                skip_value(&mut r, ty)?;
            }
            _ => skip_value(&mut r, ty)?,
        }
    }
    if h.architecture.as_deref().is_some_and(|a| a.contains("bert") || a.contains("embedding") || a == "t5encoder") {
        h.is_embedding = true;
    }
    Ok(h)
}

const STRING: u32 = 8;
const ARRAY: u32 = 9;

fn fixed_size(ty: u32) -> Option<i64> {
    match ty {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    }
}

fn skip_value<R: Read + Seek>(r: &mut BufReader<R>, ty: u32) -> std::io::Result<()> {
    if let Some(n) = fixed_size(ty) {
        return r.seek_relative(n);
    }
    match ty {
        STRING => skip_string(r),
        ARRAY => {
            let item = read_u32(r)?;
            let len = read_u64(r)?;
            if let Some(n) = fixed_size(item) {
                let bytes = len.checked_mul(n as u64).filter(|b| *b <= i64::MAX as u64).ok_or_else(|| invalid("array too long"))?;
                r.seek(SeekFrom::Current(bytes as i64)).map(|_| ())
            } else {
                for _ in 0..len {
                    skip_value(r, item)?;
                }
                Ok(())
            }
        }
        _ => Err(invalid("unknown value type")),
    }
}

fn invalid(what: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_owned())
}

fn read_u32(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> std::io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_string(r: &mut impl Read) -> std::io::Result<String> {
    let len = read_u64(r)?;
    if len > MAX_STRING {
        return Err(invalid("string too long"));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn skip_string<R: Read + Seek>(r: &mut BufReader<R>) -> std::io::Result<()> {
    let len = read_u64(r)?;
    if len > MAX_STRING {
        return Err(invalid("string too long"));
    }
    r.seek_relative(len as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    enum V<'a> {
        Str(&'a str),
        U32(u32),
        Strs(&'a [&'a str]),
        F32s(usize),
    }

    fn string(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(&(s.len() as u64).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    }

    /// A GGUF v3 header with these keys and no tensors.
    fn gguf(kv: &[(&str, V)]) -> Vec<u8> {
        let mut b = b"GGUF".to_vec();
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&(kv.len() as u64).to_le_bytes());
        for (k, v) in kv {
            string(&mut b, k);
            match v {
                V::Str(s) => {
                    b.extend_from_slice(&STRING.to_le_bytes());
                    string(&mut b, s);
                }
                V::U32(n) => {
                    b.extend_from_slice(&4u32.to_le_bytes());
                    b.extend_from_slice(&n.to_le_bytes());
                }
                V::Strs(items) => {
                    b.extend_from_slice(&ARRAY.to_le_bytes());
                    b.extend_from_slice(&STRING.to_le_bytes());
                    b.extend_from_slice(&(items.len() as u64).to_le_bytes());
                    for s in *items {
                        string(&mut b, s);
                    }
                }
                V::F32s(n) => {
                    b.extend_from_slice(&ARRAY.to_le_bytes());
                    b.extend_from_slice(&6u32.to_le_bytes());
                    b.extend_from_slice(&(*n as u64).to_le_bytes());
                    b.extend(std::iter::repeat_n(0u8, n * 4));
                }
            }
        }
        b
    }

    fn chat(arch: &str, name: &str) -> Vec<u8> {
        gguf(&[
            ("general.architecture", V::Str(arch)),
            ("general.name", V::Str(name)),
            ("general.size_label", V::Str("4B")),
            ("llama.rope.freqs", V::F32s(16)),
            ("general.tags", V::Strs(&["a", "bb"])),
            ("tokenizer.ggml.tokens", V::Strs(&["<s>", "</s>"])),
        ])
    }

    fn put(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn the_header_gives_architecture_name_and_size() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("m.gguf");
        put(&p, &chat("qwen2", "Qwen2.5 7B Instruct"));
        let h = read_header(&p).unwrap();
        assert_eq!(h, Header { architecture: Some("qwen2".into()), name: Some("Qwen2.5 7B Instruct".into()), size_label: Some("4B".into()), is_embedding: false });
    }

    #[test]
    fn embedding_models_are_recognised() {
        let tmp = tempfile::tempdir().unwrap();
        let pooled = tmp.path().join("e.gguf");
        put(&pooled, &gguf(&[("general.architecture", V::Str("gemma-embedding")), ("gemma-embedding.pooling_type", V::U32(1))]));
        assert!(read_header(&pooled).unwrap().is_embedding);
        let bert = tmp.path().join("b.gguf");
        put(&bert, &gguf(&[("general.architecture", V::Str("nomic-bert"))]));
        assert!(read_header(&bert).unwrap().is_embedding);
    }

    #[test]
    fn damaged_headers_are_refused_without_reading_on() {
        let tmp = tempfile::tempdir().unwrap();
        let text = tmp.path().join("t.gguf");
        put(&text, b"just text, renamed");
        assert!(read_header(&text).is_err());
        let mut huge = chat("llama", "x")[..24].to_vec();
        huge[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        let p = tmp.path().join("h.gguf");
        put(&p, &huge);
        assert!(read_header(&p).is_err(), "an absurd key count");
        let mut cut = chat("llama", "x");
        cut.truncate(40);
        put(&p, &cut);
        assert!(read_header(&p).is_err(), "a truncated header");
    }

    #[test]
    fn finds_models_in_the_known_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        put(&home.join(".lmstudio/models/lmstudio-community/Qwen/qwen.gguf"), &chat("qwen2", "Qwen2.5 7B"));
        put(&home.join(".lmstudio/models/x/mmproj-model.gguf"), &gguf(&[("general.architecture", V::Str("clip"))]));
        put(&home.join(".lmstudio/models/x/nomic.gguf"), &gguf(&[("general.architecture", V::Str("nomic-bert"))]));
        put(&home.join(".lmstudio/models/big/big-00001-of-00002.gguf"), &chat("llama", "Big"));
        put(&home.join(".lmstudio/models/big/big-00002-of-00002.gguf"), &chat("llama", "Big"));
        put(&home.join(".lmstudio/models/readme.txt"), b"not a model");
        // the Hugging Face cache: a snapshot links to a nameless blob
        let blob = home.join(".cache/huggingface/hub/models--a--b/blobs/0123abcd");
        put(&blob, &chat("llama", "Llama 3.2 3B"));
        let snap = home.join(".cache/huggingface/hub/models--a--b/snapshots/rev/llama.gguf");
        std::fs::create_dir_all(snap.parent().unwrap()).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&blob, &snap).unwrap();
        #[cfg(windows)]
        std::fs::copy(&blob, &snap).unwrap();
        let places = known_places(&|_| None, Some(home), None);
        let found = scan(&places);
        let names: Vec<&str> = found.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Big", "Llama 3.2 3B", "Qwen2.5 7B"]);
        assert_eq!(found[2].source, Source::LmStudio);
        assert_eq!((found[2].architecture.as_deref(), found[2].size_label.as_deref()), (Some("qwen2"), Some("4B")));
        assert_eq!(found[1].source, Source::HuggingFace);
    }

    #[test]
    fn ollama_blobs_get_their_names_from_the_manifests() {
        let tmp = tempfile::tempdir().unwrap();
        let models = tmp.path().join("ollama");
        let digest = "a".repeat(64);
        put(&models.join(format!("blobs/sha256-{digest}")), &chat("gemma3", "Gemma 3 4b It"));
        let manifest = format!(r#"{{"layers":[{{"mediaType":"application/vnd.ollama.image.template","digest":"sha256:{}"}},{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{digest}"}}]}}"#, "b".repeat(64));
        put(&models.join("manifests/registry.ollama.ai/library/gemma3/4b"), manifest.as_bytes());
        put(&models.join("manifests/registry.ollama.ai/someone/tuned/latest"), manifest.as_bytes());
        put(&models.join("manifests/registry.ollama.ai/library/evil/x"), br#"{"layers":[{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:../../etc"}]}"#);
        let found = scan(&[(Source::Ollama, models)]);
        assert_eq!(found.len(), 1, "one blob, however many tags point at it");
        assert!(["gemma3:4b", "someone/tuned:latest"].contains(&found[0].name.as_str()));
        assert_eq!(found[0].source, Source::Ollama);
    }

    #[test]
    fn plas_own_catalogue_model_comes_first_as_recommended() {
        let tmp = tempfile::tempdir().unwrap();
        let entry = crate::models::catalog::recommended();
        let ours = tmp.path().join("pla").join(entry.file_name);
        put(&ours, &chat("gemma4", "Gemma 4 E2B It"));
        std::fs::File::options().write(true).open(&ours).unwrap().set_len(entry.size).unwrap(); // sparse
        put(&tmp.path().join("lms/a.gguf"), &chat("llama", "Aaa"));
        let found = scan(&[(Source::LmStudio, tmp.path().join("lms")), (Source::Pla, tmp.path().join("pla"))]);
        assert_eq!(found[0].name, entry.name);
        assert!(found[0].recommended && !found[1].recommended);
        // an unfinished copy is not the recommended model
        std::fs::File::options().write(true).open(&ours).unwrap().set_len(entry.size - 1).unwrap();
        assert!(!scan(&[(Source::Pla, tmp.path().join("pla"))])[0].recommended);
    }

    #[test]
    fn split_parts_after_the_first_are_left_out() {
        assert!(is_later_split_part("m-00002-of-00003.gguf"));
        assert!(!is_later_split_part("m-00001-of-00003.gguf"));
        assert!(!is_later_split_part("gemma-4-E2B-it-Q3_K_M.gguf"));
    }

    #[test]
    fn places_follow_the_environment() {
        let env = |k: &str| match k {
            "OLLAMA_MODELS" => Some(OsString::from("/data/ollama")),
            "HF_HOME" => Some(OsString::from("/data/hf")),
            "HF_HUB_CACHE" => Some(OsString::new()),
            _ => None,
        };
        let places = known_places(&env, None, Some(Path::new("/pla/models")));
        assert_eq!(places, [(Source::Pla, PathBuf::from("/pla/models")), (Source::Ollama, PathBuf::from("/data/ollama")), (Source::HuggingFace, PathBuf::from("/data/hf/hub"))]);
    }
}
