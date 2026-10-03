//! One model download at a time, on its own thread (model-manager spec § 5).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pla_core::models::download::{DownloadError, Progress};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Failure {
    pub kind: &'static str,
    pub needed: Option<u64>,
    pub available: Option<u64>,
    pub detail: String,
}

impl From<&DownloadError> for Failure {
    fn from(e: &DownloadError) -> Self {
        let (kind, needed, available) = match e {
            DownloadError::Network(_) | DownloadError::Paused => ("network", None, None),
            DownloadError::HostNotAllowed(_) => ("host", None, None),
            DownloadError::NotHttps => ("https", None, None),
            DownloadError::InsufficientSpace { needed, available } => ("space", Some(*needed), Some(*available)),
            DownloadError::ChecksumMismatch => ("checksum", None, None),
            DownloadError::Io(_) => ("io", None, None),
        };
        Self { kind, needed, available, detail: e.to_string() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DownloadState {
    Running { progress: Progress },
    Paused { received: u64, total: u64 },
    Failed { failure: Failure, received: u64, total: u64 },
    Done { path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlreadyRunning;

type Run = dyn FnOnce(&AtomicBool, &mut dyn FnMut(Progress)) -> Result<PathBuf, DownloadError> + Send;

#[derive(Default)]
pub struct ModelDownloads {
    state: Arc<Mutex<Option<DownloadState>>>,
    cancel: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl ModelDownloads {
    /// Starts `run` on a thread unless one is still running. `on_event` sees every state change,
    /// `on_done` the verified file.
    pub fn start(
        &self,
        run: impl FnOnce(&AtomicBool, &mut dyn FnMut(Progress)) -> Result<PathBuf, DownloadError> + Send + 'static,
        on_event: impl Fn(&DownloadState) + Send + 'static,
        on_done: impl FnOnce(PathBuf) + Send + 'static,
    ) -> Result<(), AlreadyRunning> {
        let mut thread = self.thread.lock().expect("download thread lock");
        if thread.as_ref().is_some_and(|t| !t.is_finished()) {
            return Err(AlreadyRunning);
        }
        self.cancel.store(false, Ordering::SeqCst);
        let (state, cancel) = (Arc::clone(&self.state), Arc::clone(&self.cancel));
        let run: Box<Run> = Box::new(run);
        *thread = Some(
            std::thread::Builder::new()
                .name("pla-model-download".into())
                .spawn(move || {
                    let set = |s: DownloadState| {
                        *state.lock().expect("download state lock") = Some(s.clone());
                        on_event(&s);
                    };
                    let mut last = (0u64, 0u64);
                    let result = run(&cancel, &mut |p: Progress| {
                        last = (p.received, p.total);
                        set(DownloadState::Running { progress: p });
                    });
                    match result {
                        Ok(path) => {
                            set(DownloadState::Done { path: path.to_string_lossy().into_owned() });
                            on_done(path);
                        }
                        Err(DownloadError::Paused) => set(DownloadState::Paused { received: last.0, total: last.1 }),
                        Err(e) => set(DownloadState::Failed { failure: Failure::from(&e), received: last.0, total: last.1 }),
                    }
                })
                .expect("spawn model download"),
        );
        Ok(())
    }

    pub fn pause(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn current(&self) -> Option<DownloadState> {
        self.state.lock().expect("download state lock").clone()
    }

    /// On exit: ask the run to stop and wait at most `wait` (a read stuck on a dead connection
    /// must not hold Quit; the `.part` stays for the next start).
    pub fn shutdown(&self, wait: Duration) {
        self.pause();
        let deadline = Instant::now() + wait;
        let mut thread = self.thread.lock().expect("download thread lock");
        while thread.as_ref().is_some_and(|t| !t.is_finished()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if thread.as_ref().is_some_and(|t| t.is_finished()) {
            let _ = thread.take().map(|t| t.join());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn progress(received: u64) -> Progress {
        Progress { received, total: 100, bytes_per_sec: 10, eta_secs: Some(1) }
    }

    #[test]
    fn runs_reports_and_finishes() {
        let d = ModelDownloads::default();
        let (tx, rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        d.start(
            |_cancel, report| {
                report(progress(50));
                Ok(PathBuf::from("C:/m.gguf"))
            },
            move |s| tx.send(s.clone()).unwrap(),
            move |p| done_tx.send(p).unwrap(),
        )
        .unwrap();
        assert_eq!(rx.recv().unwrap(), DownloadState::Running { progress: progress(50) });
        assert_eq!(rx.recv().unwrap(), DownloadState::Done { path: "C:/m.gguf".into() });
        assert_eq!(done_rx.recv().unwrap(), PathBuf::from("C:/m.gguf"));
    }

    #[test]
    fn a_second_start_is_refused_while_one_runs() {
        // Review Focus 3
        let d = ModelDownloads::default();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        d.start(move |_, _| { go_rx.recv().unwrap(); Ok(PathBuf::from("m")) }, |_| {}, |_| {}).unwrap();
        assert_eq!(d.start(|_, _| Ok(PathBuf::from("n")), |_| {}, |_| {}), Err(AlreadyRunning));
        go_tx.send(()).unwrap();
        d.shutdown(Duration::from_secs(2));
    }

    #[test]
    fn pause_stops_the_run_and_remembers_the_bytes() {
        let d = ModelDownloads::default();
        let (tx, rx) = mpsc::channel();
        d.start(
            |cancel, report| {
                report(progress(40));
                while !cancel.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(DownloadError::Paused)
            },
            move |s| tx.send(s.clone()).unwrap(),
            |_| {},
        )
        .unwrap();
        assert!(matches!(rx.recv().unwrap(), DownloadState::Running { .. }));
        d.pause();
        assert_eq!(rx.recv().unwrap(), DownloadState::Paused { received: 40, total: 100 });
        assert_eq!(d.current(), Some(DownloadState::Paused { received: 40, total: 100 }));
        d.start(|_, _| Ok(PathBuf::from("m")), |_| {}, |_| {}).expect("a paused download can start again");
        d.shutdown(Duration::from_secs(2));
    }

    #[test]
    fn failures_carry_a_kind_the_ui_can_explain() {
        let d = ModelDownloads::default();
        let (tx, rx) = mpsc::channel();
        d.start(|_, _| Err(DownloadError::InsufficientSpace { needed: 9, available: 3 }), move |s| tx.send(s.clone()).unwrap(), |_| {}).unwrap();
        assert_eq!(
            rx.recv().unwrap(),
            DownloadState::Failed { failure: Failure { kind: "space", needed: Some(9), available: Some(3), detail: "not enough disk space: 9 bytes needed, 3 available".into() }, received: 0, total: 0 }
        );
        for (e, kind) in [
            (DownloadError::Network("x".into()), "network"),
            (DownloadError::HostNotAllowed("h".into()), "host"),
            (DownloadError::NotHttps, "https"),
            (DownloadError::ChecksumMismatch, "checksum"),
            (DownloadError::Io("disk".into()), "io"),
        ] {
            assert_eq!(Failure::from(&e).kind, kind);
        }
    }

    #[test]
    fn shutdown_returns_quickly_even_if_the_run_hangs() {
        // Review Focus 1: Quit stays under 5 s (FR-SCH-003)
        let d = ModelDownloads::default();
        d.start(|_, _| { std::thread::sleep(Duration::from_secs(30)); Ok(PathBuf::from("m")) }, |_| {}, |_| {}).unwrap();
        let t = std::time::Instant::now();
        d.shutdown(Duration::from_millis(300));
        assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
    }
}
