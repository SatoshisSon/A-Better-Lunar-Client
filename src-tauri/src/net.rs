use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::{stream, StreamExt};
use serde::de::DeserializeOwned;
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;

use crate::error::{Error, Result};
use crate::progress::Progress;

pub const USER_AGENT: &str = concat!(
    "SatoshisSon/A-Better-Lunar-Client/",
    env!("CARGO_PKG_VERSION"),
    " (github.com/SatoshisSon/A-Better-Lunar-Client)"
);

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()
        .expect("HTTP-Client konnte nicht erstellt werden")
}

pub async fn get_json<T: DeserializeOwned>(http: &reqwest::Client, url: &str) -> Result<T> {
    Ok(http.get(url).send().await?.error_for_status()?.json().await?)
}

/// JSON laden und lokal zwischenspeichern; ohne Internet wird der Cache benutzt.
pub async fn get_json_cached<T: DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
    cache_file: &Path,
) -> Result<T> {
    match http.get(url).send().await.and_then(|r| r.error_for_status()) {
        Ok(resp) => {
            let bytes = resp.bytes().await?;
            let value = serde_json::from_slice(&bytes)?;
            if let Some(parent) = cache_file.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(cache_file, &bytes).await?;
            Ok(value)
        }
        Err(e) => match tokio::fs::read(cache_file).await {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(_) => Err(e.into()),
        },
    }
}

#[derive(Debug, Clone)]
pub struct Download {
    pub url: String,
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub executable: bool,
}

impl Download {
    pub fn new(url: impl Into<String>, path: PathBuf) -> Self {
        Self { url: url.into(), path, sha1: None, size: None, executable: false }
    }
    pub fn sha1(mut self, sha1: Option<String>) -> Self {
        self.sha1 = sha1;
        self
    }
    pub fn size(mut self, size: Option<u64>) -> Self {
        self.size = size;
        self
    }
}

pub fn sha1_file(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(hex::encode(Sha1::digest(&bytes)))
}

/// Schnelle Prüfung: Existiert die Datei mit der richtigen Größe, gilt sie als
/// fertig. Nur wenn keine Größe bekannt ist, wird der Hash berechnet.
fn is_complete(d: &Download) -> bool {
    let Ok(meta) = std::fs::metadata(&d.path) else { return false };
    match (d.size, &d.sha1) {
        (Some(size), _) => meta.len() == size,
        (None, Some(sha1)) => sha1_file(&d.path).is_some_and(|h| h.eq_ignore_ascii_case(sha1)),
        (None, None) => true,
    }
}

async fn fetch(http: &reqwest::Client, d: &Download) -> Result<()> {
    if let Some(parent) = d.path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = d.path.with_extension("part");
    let mut resp = http.get(&d.url).send().await?.error_for_status()?;
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut hasher = Sha1::new();
    while let Some(chunk) = resp.chunk().await? {
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);

    if let Some(expected) = &d.sha1 {
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(Error::msg(format!("Prüfsumme falsch: {}", d.url)));
        }
    }
    tokio::fs::rename(&tmp, &d.path).await?;

    #[cfg(unix)]
    if d.executable {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&d.path, std::fs::Permissions::from_mode(0o755)).await?;
    }
    Ok(())
}

/// Lädt alle fehlenden Dateien parallel herunter (mit bis zu 3 Versuchen).
pub async fn download_all(
    http: &reqwest::Client,
    downloads: Vec<Download>,
    concurrency: usize,
    progress: &Progress,
    stage: &str,
) -> Result<()> {
    let pending: Vec<Download> = tokio::task::spawn_blocking(move || {
        downloads.into_iter().filter(|d| !is_complete(d)).collect()
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))?;

    let total = pending.len();
    if total == 0 {
        return Ok(());
    }
    let done = Arc::new(AtomicUsize::new(0));
    progress.update(stage, 0, total);

    let results: Vec<Result<()>> = stream::iter(pending)
        .map(|d| {
            let done = done.clone();
            async move {
                let mut last_err = None;
                for attempt in 0..3 {
                    if attempt > 0 {
                        tokio::time::sleep(Duration::from_millis(500 * attempt)).await;
                    }
                    match fetch(http, &d).await {
                        Ok(()) => {
                            last_err = None;
                            break;
                        }
                        Err(e) => last_err = Some(e),
                    }
                }
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                progress.update(stage, n, total);
                match last_err {
                    Some(e) => Err(e),
                    None => Ok(()),
                }
            }
        })
        .buffer_unordered(concurrency.max(1))
        .collect()
        .await;

    results.into_iter().collect()
}
