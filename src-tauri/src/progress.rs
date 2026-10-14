use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
struct ProgressEvent<'a> {
    task: &'a str,
    stage: &'a str,
    done: usize,
    total: usize,
}

/// Schickt Fortschrittsmeldungen an die Oberfläche, gedrosselt auf ~20/s.
pub struct Progress {
    app: Option<AppHandle>,
    task: String,
    last: Mutex<Instant>,
}

impl Progress {
    pub fn new(app: &AppHandle, task: impl Into<String>) -> Self {
        Self {
            app: Some(app.clone()),
            task: task.into(),
            last: Mutex::new(Instant::now() - Duration::from_secs(1)),
        }
    }

    /// Ohne Oberfläche (z. B. in Tests).
    #[allow(dead_code)]
    pub fn silent() -> Self {
        Self { app: None, task: String::new(), last: Mutex::new(Instant::now()) }
    }

    pub fn update(&self, stage: &str, done: usize, total: usize) {
        let Some(app) = &self.app else { return };
        let finished = done == 0 || done >= total;
        {
            let mut last = self.last.lock().unwrap();
            if !finished && last.elapsed() < Duration::from_millis(50) {
                return;
            }
            *last = Instant::now();
        }
        let _ = app.emit(
            "progress",
            ProgressEvent { task: &self.task, stage, done, total },
        );
    }

    pub fn stage(&self, stage: &str) {
        self.update(stage, 0, 0);
    }
}
