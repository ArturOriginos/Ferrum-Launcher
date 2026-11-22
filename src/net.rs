use anyhow::{anyhow, bail, Result};
use rayon::prelude::*;
use reqwest::blocking::Client;
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Runs work on a thread and turns a panic into a regular error so the UI doesn't hang
/// in the “loading” state.
pub fn spawn_guarded(
    on_panic: impl FnOnce(String) + Send + 'static,
    f: impl FnOnce() + Send + 'static,
) {
    std::thread::spawn(move || {
        if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown error".to_string());
            on_panic(format!("Internal launcher error: {msg}"));
        }
    });
}

pub fn http_client() -> Client {
    Client::builder()
        .user_agent(format!(
            "FerrumLauncher/{} (github.com/ArturOriginos/ferrum-launcher)",
            crate::config::VERSION
        ))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()
        .expect("failed to create the HTTP client")
}

pub fn get_json(http: &Client, url: &str) -> Result<Value> {
    Ok(http.get(url).send()?.error_for_status()?.json::<Value>()?)
}

pub fn sha1_hex(data: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

// ---------- Slot for a background thread's result ----------

pub struct Slot<T>(Arc<Mutex<Option<T>>>);

impl<T> Slot<T> {
    pub fn new() -> Self {
        Slot(Arc::new(Mutex::new(None)))
    }
    pub fn put(&self, v: T) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(v);
    }
    pub fn take(&self) -> Option<T> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        Slot(self.0.clone())
    }
}

// ---------- Launch progress ----------

#[derive(Clone, Default)]
pub struct ProgressState {
    pub label: String,
    pub done: usize,
    pub total: usize,
    pub error: Option<String>,
    pub busy: bool,
    pub running: bool,
    /// When the game started (unix seconds) and how long the session lasted (filled in on exit).
    pub play_started: Option<u64>,
    pub played_secs: u64,
}

#[derive(Clone, Default)]
pub struct Progress(Arc<Mutex<ProgressState>>);

impl Progress {
    pub fn new() -> Self {
        let p = Self::default();
        p.0.lock().unwrap_or_else(|e| e.into_inner()).busy = true;
        p
    }
    /// Start of a new stage: label and counter reset.
    pub fn label(&self, s: impl Into<String>) {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        g.label = s.into();
        g.done = 0;
        g.total = 0;
    }
    pub fn add_total(&self, n: usize) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).total += n;
    }
    pub fn tick(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).done += 1;
    }
    pub fn fail(&self, e: impl Into<String>) {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        g.error = Some(e.into());
        g.busy = false;
        g.running = false;
        if let Some(st) = g.play_started {
            g.played_secs = crate::config::now().saturating_sub(st);
        }
    }
    pub fn running(&self, s: impl Into<String>) {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        g.label = s.into();
        g.busy = false;
        g.running = true;
        g.play_started = Some(crate::config::now());
    }
    pub fn finish(&self, s: impl Into<String>) {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        g.label = s.into();
        g.busy = false;
        g.running = false;
        if let Some(st) = g.play_started {
            g.played_secs = crate::config::now().saturating_sub(st);
        }
    }
    pub fn snapshot(&self) -> ProgressState {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

// ---------- Downloads ----------

#[derive(Clone)]
pub struct Job {
    pub url: String,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

/// Downloads one file (skips it if it already exists and the size matches).
pub fn download(http: &Client, j: &Job) -> Result<()> {
    if let Ok(meta) = fs::metadata(&j.dest) {
        if meta.len() > 0 && j.size.map_or(true, |s| s == meta.len()) {
            return Ok(());
        }
    }
    if j.url.is_empty() {
        bail!("No download link for file {}", j.dest.display());
    }
    if let Some(p) = j.dest.parent() {
        fs::create_dir_all(p)?;
    }
    let mut last: Option<anyhow::Error> = None;
    for attempt in 0..3u64 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(500 * attempt));
        }
        let res = http
            .get(&j.url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.bytes());
        match res {
            Ok(bytes) => {
                if let Some(h) = &j.sha1 {
                    if !h.is_empty() && sha1_hex(&bytes) != *h {
                        last = Some(anyhow!("Checksum mismatch: {}", j.url));
                        continue;
                    }
                }
                let tmp = PathBuf::from(format!("{}.part", j.dest.display()));
                fs::write(&tmp, &bytes)?;
                fs::rename(&tmp, &j.dest)?;
                return Ok(());
            }
            Err(e) => last = Some(anyhow!("{}: {}", j.url, e)),
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("Could not download {}", j.url)))
}

/// Parallel download of a list of files with progress updates.
pub fn download_all(http: &Client, jobs: Vec<Job>, progress: &Progress) -> Result<()> {
    if jobs.is_empty() {
        return Ok(());
    }
    progress.add_total(jobs.len());
    let pool = rayon::ThreadPoolBuilder::new().num_threads(12).build()?;
    pool.install(|| {
        jobs.par_iter().try_for_each(|j| {
            let r = download(http, j);
            progress.tick();
            r
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_of_abc() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn slot_put_and_take() {
        let s: Slot<u32> = Slot::new();
        assert!(s.take().is_none());
        s.put(5);
        assert_eq!(s.take(), Some(5));
        assert!(s.take().is_none());
    }

    #[test]
    fn play_time_is_recorded_on_finish() {
        let p = Progress::new();
        p.running("Game running");
        assert!(p.snapshot().play_started.is_some());
        p.finish("Game closed");
        let s = p.snapshot();
        assert!(!s.running);
        assert!(s.played_secs < 5);
    }

    #[test]
    fn guarded_thread_reports_panic() {
        let slot: Slot<String> = Slot::new();
        let s2 = slot.clone();
        spawn_guarded(move |m| s2.put(m), || panic!("test"));
        for _ in 0..100 {
            if let Some(m) = slot.take() {
                assert!(m.contains("test"), "{m}");
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the panic did not reach the handler");
    }

    #[test]
    fn progress_flow() {
        let p = Progress::new();
        p.label("stage");
        p.add_total(3);
        p.tick();
        let s = p.snapshot();
        assert!(s.busy);
        assert_eq!((s.done, s.total), (1, 3));
        p.fail("boom");
        let s = p.snapshot();
        assert!(!s.busy);
        assert_eq!(s.error.as_deref(), Some("boom"));
    }
}
