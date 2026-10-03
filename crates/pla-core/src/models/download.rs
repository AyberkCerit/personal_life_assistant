//! Downloads a catalogue model (FR-MDL-001…007): allow-listed hosts only, a free-space check,
//! resume from a `.part` file, progress, and a SHA-256 check before the file is used.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::catalog::CatalogEntry;
use super::policy::{PolicyError, UrlPolicy};

/// FR-MDL-003: keep this much room on the disk besides the model.
pub const SPARE_BYTES: u64 = 500 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Progress {
    pub received: u64,
    pub total: u64,
    pub bytes_per_sec: u64,
    pub eta_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DownloadError {
    #[error("network error: {0}")]
    Network(String),
    #[error("{0} is not an allowed download host")]
    HostNotAllowed(String),
    #[error("only https downloads are allowed")]
    NotHttps,
    #[error("not enough disk space: {needed} bytes needed, {available} available")]
    InsufficientSpace { needed: u64, available: u64 },
    #[error("the downloaded file does not match the catalogue checksum")]
    ChecksumMismatch,
    #[error("file error: {0}")]
    Io(String),
    #[error("paused")]
    Paused,
}

impl From<PolicyError> for DownloadError {
    fn from(e: PolicyError) -> Self {
        match e {
            PolicyError::NotHttps => Self::NotHttps,
            PolicyError::HostNotAllowed(h) => Self::HostNotAllowed(h),
            PolicyError::BadUrl(u) => Self::Network(format!("unusable address {u}")),
        }
    }
}

impl From<std::io::Error> for DownloadError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

pub struct Request<'a> {
    pub entry: &'a CatalogEntry,
    pub dir: &'a Path,
    pub policy: &'a dyn UrlPolicy,
    pub free_space: &'a dyn Fn(&Path) -> u64,
    pub cancel: &'a AtomicBool,
    /// How often progress is reported (1 s in the app).
    pub progress_every: Duration,
}

pub fn part_path(dir: &Path, entry: &CatalogEntry) -> PathBuf {
    dir.join(format!("{}.part", entry.file_name))
}

/// Downloads `entry` into `dir`, resuming a previous `.part`. On `Paused` or `Network` the part is
/// kept; on `ChecksumMismatch` it is deleted. Returns the verified file's path.
pub fn download(req: &Request, progress: &mut dyn FnMut(Progress)) -> Result<PathBuf, DownloadError> {
    let size = req.entry.size;
    fs::create_dir_all(req.dir)?;
    let part = part_path(req.dir, req.entry);
    let mut have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have > size {
        fs::remove_file(&part)?;
        have = 0;
    }
    let needed = size - have + SPARE_BYTES;
    let available = (req.free_space)(req.dir);
    if available < needed {
        return Err(DownloadError::InsufficientSpace { needed, available });
    }
    let mut hasher = Sha256::new();
    if have > 0 {
        hash_file(&part, &mut hasher)?;
    }
    if have < size {
        let response = open(req, have)?;
        let mut file = match response.status().as_u16() {
            206 => OpenOptions::new().append(true).open(&part)?,
            200 => {
                have = 0;
                hasher = Sha256::new();
                File::create(&part)?
            }
            code => return Err(DownloadError::Network(format!("the server answered HTTP {code}"))),
        };
        let mut reader = response.into_body().into_reader();
        let mut buf = vec![0u8; CHUNK];
        let started = Instant::now();
        let start_bytes = have;
        let mut last_report: Option<Instant> = None;
        loop {
            if req.cancel.load(Ordering::SeqCst) {
                file.flush()?;
                return Err(DownloadError::Paused);
            }
            let n = reader.read(&mut buf).map_err(|e| DownloadError::Network(e.to_string()))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            hasher.update(&buf[..n]);
            have += n as u64;
            if last_report.is_none_or(|t| t.elapsed() >= req.progress_every) || have == size {
                last_report = Some(Instant::now());
                let secs = started.elapsed().as_secs_f64().max(0.001);
                let bytes_per_sec = ((have - start_bytes) as f64 / secs) as u64;
                let eta_secs = (bytes_per_sec > 0).then(|| (size - have) / bytes_per_sec);
                progress(Progress { received: have, total: size, bytes_per_sec, eta_secs });
            }
        }
        file.flush()?;
        if have != size {
            return Err(DownloadError::Network(format!("the connection closed after {have} of {size} bytes")));
        }
    } else {
        progress(Progress { received: have, total: size, bytes_per_sec: 0, eta_secs: Some(0) });
    }
    if hex::encode(hasher.finalize()) != req.entry.sha256 {
        let _ = fs::remove_file(&part);
        return Err(DownloadError::ChecksumMismatch);
    }
    let target = req.dir.join(req.entry.file_name);
    rename_with_retry(&part, &target)?;
    Ok(target)
}

/// Follows up to five redirects itself, asking the policy before every connection.
fn open(req: &Request, from: u64) -> Result<ureq::http::Response<ureq::Body>, DownloadError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut url = req.entry.url.to_owned();
    for _ in 0..=MAX_REDIRECTS {
        req.policy.allows(&url)?;
        let mut call = agent.get(&url);
        if from > 0 {
            call = call.header("Range", format!("bytes={from}-"));
        }
        let response = call.call().map_err(|e| DownloadError::Network(e.to_string()))?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| DownloadError::Network("a redirect without a location".into()))?;
            url = absolute(&url, location)?;
            continue;
        }
        return Ok(response);
    }
    Err(DownloadError::Network("too many redirects".into()))
}

fn absolute(base: &str, location: &str) -> Result<String, DownloadError> {
    if location.contains("://") {
        return Ok(location.to_owned());
    }
    let (scheme, rest) = base.split_once("://").ok_or_else(|| DownloadError::Network(format!("unusable address {base}")))?;
    let authority = rest.split('/').next().unwrap_or("");
    if location.starts_with('/') {
        Ok(format!("{scheme}://{authority}{location}"))
    } else {
        Err(DownloadError::Network(format!("unsupported redirect {location}")))
    }
}

fn hash_file(path: &Path, hasher: &mut Sha256) -> Result<(), DownloadError> {
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        hasher.update(&buf[..n]);
    }
}

/// Antivirus scanners can hold a fresh 2 GB file for a moment (spec § 9).
fn rename_with_retry(from: &Path, to: &Path) -> Result<(), DownloadError> {
    let mut last = None;
    for _ in 0..10 {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(last.map(DownloadError::from).unwrap_or_else(|| DownloadError::Io("rename failed".into())))
}

/// Free bytes on the disk holding `dir` (which must exist); `u64::MAX` if Windows cannot tell.
#[cfg(windows)]
pub fn free_space(dir: &Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free: u64 = 0;
    // SAFETY: `wide` is NUL-terminated and outlives the call; the out pointers are valid or null.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    if ok == 0 { u64::MAX } else { free }
}

#[cfg(not(windows))]
pub fn free_space(_dir: &Path) -> u64 {
    u64::MAX
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::policy::{scheme_and_host, PolicyError, UrlPolicy};
    use sha2::{Digest, Sha256};
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    enum Mode {
        Normal,
        IgnoreRange,
        /// The first response announces the whole body but stops after this many bytes.
        DropFirstAfter(usize),
        RedirectTo(String),
        Corrupt,
    }

    /// A tiny HTTP/1.1 server on 127.0.0.1 that records each request's path and Range header.
    fn serve(body: Vec<u8>, mode: Mode) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for (n, stream) in listener.incoming().enumerate() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let path = first.split_whitespace().nth(1).unwrap_or("").to_owned();
                let mut from = 0usize;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        from = v.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                log.lock().unwrap().push(format!("{path} from={from}"));
                let mut data = body.clone();
                match &mode {
                    Mode::RedirectTo(to) if path == "/model" => {
                        write!(stream, "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                        continue;
                    }
                    Mode::Corrupt => data[0] ^= 0xff,
                    _ => {}
                }
                let honour_range = !matches!(mode, Mode::IgnoreRange) && from > 0;
                let part = if honour_range { &data[from..] } else { &data[..] };
                let status = if honour_range { "206 Partial Content" } else { "200 OK" };
                write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", part.len()).unwrap();
                match mode {
                    Mode::DropFirstAfter(k) if n == 0 => {
                        let _ = stream.write_all(&part[..k]);
                    }
                    _ => {
                        let _ = stream.write_all(part);
                    }
                }
            }
        });
        (url, seen)
    }

    struct LocalOnly;
    impl UrlPolicy for LocalOnly {
        fn allows(&self, url: &str) -> Result<(), PolicyError> {
            let (_, host) = scheme_and_host(url)?;
            if host == "127.0.0.1" { Ok(()) } else { Err(PolicyError::HostNotAllowed(host)) }
        }
    }

    fn leak(s: String) -> &'static str {
        Box::leak(s.into_boxed_str())
    }

    fn entry(url: &str, body: &[u8]) -> CatalogEntry {
        CatalogEntry {
            id: "test",
            name: "test",
            file_name: "model.gguf",
            size: body.len() as u64,
            sha256: leak(hex::encode(Sha256::digest(body))),
            url: leak(url.to_owned()),
            source: "test",
            licence: "MIT",
            licence_url: "",
        }
    }

    fn body() -> Vec<u8> {
        (0..300_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn plenty(_: &Path) -> u64 {
        u64::MAX
    }

    fn run(e: &CatalogEntry, dir: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(Progress)) -> Result<PathBuf, DownloadError> {
        let req = Request { entry: e, dir, policy: &LocalOnly, free_space: &plenty, cancel, progress_every: Duration::ZERO };
        download(&req, progress)
    }

    #[test]
    fn downloads_and_verifies_the_file() {
        let data = body();
        let (url, _) = serve(data.clone(), Mode::Normal);
        let tmp = tempfile::tempdir().unwrap();
        let e = entry(&url, &data);
        let mut last = None;
        let path = run(&e, tmp.path(), &AtomicBool::new(false), &mut |p| last = Some(p)).unwrap();
        assert_eq!(path, tmp.path().join("model.gguf"));
        assert_eq!(std::fs::read(&path).unwrap(), data);
        assert!(!part_path(tmp.path(), &e).exists());
        assert_eq!(last.map(|p| (p.received, p.total)), Some((300_000, 300_000)));
    }

    #[test]
    fn resumes_after_a_dropped_connection() {
        // FR-MDL-005, Review Focus 2
        let data = body();
        let (url, seen) = serve(data.clone(), Mode::DropFirstAfter(100_000));
        let tmp = tempfile::tempdir().unwrap();
        let e = entry(&url, &data);
        let cancel = AtomicBool::new(false);
        assert!(matches!(run(&e, tmp.path(), &cancel, &mut |_| {}), Err(DownloadError::Network(_))));
        assert_eq!(std::fs::metadata(part_path(tmp.path(), &e)).unwrap().len(), 100_000, "the part is kept");
        let path = run(&e, tmp.path(), &cancel, &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
        assert_eq!(seen.lock().unwrap().last().unwrap(), "/model from=100000", "only the rest was asked for");
    }

    #[test]
    fn starts_over_when_the_server_ignores_the_range() {
        let data = body();
        let (url, _) = serve(data.clone(), Mode::IgnoreRange);
        let tmp = tempfile::tempdir().unwrap();
        let e = entry(&url, &data);
        std::fs::write(part_path(tmp.path(), &e), &data[..1000]).unwrap();
        let path = run(&e, tmp.path(), &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
    }

    #[test]
    fn a_checksum_mismatch_deletes_the_file() {
        // FR-MDL-007
        let data = body();
        let (url, _) = serve(data.clone(), Mode::Corrupt);
        let tmp = tempfile::tempdir().unwrap();
        let e = entry(&url, &data);
        assert_eq!(run(&e, tmp.path(), &AtomicBool::new(false), &mut |_| {}), Err(DownloadError::ChecksumMismatch));
        assert!(!part_path(tmp.path(), &e).exists());
        assert!(!tmp.path().join("model.gguf").exists());
    }

    #[test]
    fn refuses_a_redirect_to_a_host_outside_the_allow_list() {
        // FR-MDL-001
        let data = body();
        let (url, seen) = serve(data.clone(), Mode::RedirectTo("http://example.invalid/model".into()));
        let tmp = tempfile::tempdir().unwrap();
        let e = entry(&url, &data);
        assert_eq!(run(&e, tmp.path(), &AtomicBool::new(false), &mut |_| {}), Err(DownloadError::HostNotAllowed("example.invalid".into())));
        assert_eq!(seen.lock().unwrap().len(), 1, "only the first, allowed request was made");
    }

    #[test]
    fn follows_an_allowed_relative_redirect() {
        let data = body();
        let (url, seen) = serve(data.clone(), Mode::RedirectTo("/real".into()));
        let tmp = tempfile::tempdir().unwrap();
        let e = entry(&url, &data);
        let path = run(&e, tmp.path(), &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
        assert_eq!(seen.lock().unwrap().clone(), vec!["/model from=0".to_owned(), "/real from=0".to_owned()]);
    }

    #[test]
    fn checks_free_space_before_any_request() {
        // FR-MDL-003
        let data = body();
        let (url, seen) = serve(data.clone(), Mode::Normal);
        let tmp = tempfile::tempdir().unwrap();
        let e = entry(&url, &data);
        let small = |_: &Path| 10u64;
        let req = Request { entry: &e, dir: tmp.path(), policy: &LocalOnly, free_space: &small, cancel: &AtomicBool::new(false), progress_every: Duration::ZERO };
        assert_eq!(download(&req, &mut |_| {}), Err(DownloadError::InsufficientSpace { needed: 300_000 + SPARE_BYTES, available: 10 }));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn pausing_keeps_the_part_for_later() {
        let data = body();
        let (url, _) = serve(data.clone(), Mode::Normal);
        let tmp = tempfile::tempdir().unwrap();
        let e = entry(&url, &data);
        let cancel = AtomicBool::new(false);
        assert_eq!(run(&e, tmp.path(), &cancel, &mut |_| cancel.store(true, Ordering::SeqCst)), Err(DownloadError::Paused));
        assert!(std::fs::metadata(part_path(tmp.path(), &e)).unwrap().len() > 0);
        cancel.store(false, Ordering::SeqCst);
        assert_eq!(std::fs::read(run(&e, tmp.path(), &cancel, &mut |_| {}).unwrap()).unwrap(), data);
    }

    #[test]
    fn reads_the_free_space_of_a_real_disk() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(free_space(tmp.path()) > 0);
    }
}
