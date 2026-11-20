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

pub fn http_client() -> Client {
    Client::builder()
        .user_agent(format!(
            "FerrumLauncher/{} (github.com/ArturOriginos/ferrum-launcher)",
            crate::config::VERSION
        ))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()
        .expect("не удалось создать HTTP-клиент")
}

pub fn get_json(http: &Client, url: &str) -> Result<Value> {
    Ok(http.get(url).send()?.error_for_status()?.json::<Value>()?)
}

pub fn sha1_hex(data: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

// ---------- Слот для результата фонового потока ----------

pub struct Slot<T>(Arc<Mutex<Option<T>>>);

impl<T> Slot<T> {
    pub fn new() -> Self {
        Slot(Arc::new(Mutex::new(None)))
    }
    pub fn put(&self, v: T) {
        *self.0.lock().unwrap() = Some(v);
    }
    pub fn take(&self) -> Option<T> {
        self.0.lock().unwrap().take()
    }
}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        Slot(self.0.clone())
    }
}

// ---------- Прогресс запуска ----------

#[derive(Clone, Default)]
pub struct ProgressState {
    pub label: String,
    pub done: usize,
    pub total: usize,
    pub error: Option<String>,
    pub busy: bool,
    pub running: bool,
}

#[derive(Clone, Default)]
pub struct Progress(Arc<Mutex<ProgressState>>);

impl Progress {
    pub fn new() -> Self {
        let p = Self::default();
        p.0.lock().unwrap().busy = true;
        p
    }
    /// Начало нового этапа: подпись и сброс счётчиков.
    pub fn label(&self, s: impl Into<String>) {
        let mut g = self.0.lock().unwrap();
        g.label = s.into();
        g.done = 0;
        g.total = 0;
    }
    pub fn add_total(&self, n: usize) {
        self.0.lock().unwrap().total += n;
    }
    pub fn tick(&self) {
        self.0.lock().unwrap().done += 1;
    }
    pub fn fail(&self, e: impl Into<String>) {
        let mut g = self.0.lock().unwrap();
        g.error = Some(e.into());
        g.busy = false;
        g.running = false;
    }
    pub fn running(&self, s: impl Into<String>) {
        let mut g = self.0.lock().unwrap();
        g.label = s.into();
        g.busy = false;
        g.running = true;
    }
    pub fn finish(&self, s: impl Into<String>) {
        let mut g = self.0.lock().unwrap();
        g.label = s.into();
        g.busy = false;
        g.running = false;
    }
    pub fn snapshot(&self) -> ProgressState {
        self.0.lock().unwrap().clone()
    }
}

// ---------- Загрузки ----------

#[derive(Clone)]
pub struct Job {
    pub url: String,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

/// Скачивает один файл (пропускает, если уже есть и размер совпал).
pub fn download(http: &Client, j: &Job) -> Result<()> {
    if let Ok(meta) = fs::metadata(&j.dest) {
        if meta.len() > 0 && j.size.map_or(true, |s| s == meta.len()) {
            return Ok(());
        }
    }
    if j.url.is_empty() {
        bail!("Нет ссылки для файла {}", j.dest.display());
    }
    if let Some(p) = j.dest.parent() {
        fs::create_dir_all(p)?;
    }
    let mut last: Option<anyhow::Error> = None;
    for _ in 0..3 {
        let res = http
            .get(&j.url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.bytes());
        match res {
            Ok(bytes) => {
                if let Some(h) = &j.sha1 {
                    if !h.is_empty() && sha1_hex(&bytes) != *h {
                        last = Some(anyhow!("Не совпала контрольная сумма: {}", j.url));
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
    Err(last.unwrap_or_else(|| anyhow!("Не удалось скачать {}", j.url)))
}

/// Параллельная загрузка списка файлов с обновлением прогресса.
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
    fn progress_flow() {
        let p = Progress::new();
        p.label("этап");
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
