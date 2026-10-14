//! Lädt automatisch die passende Java-Laufzeit von Mojang herunter.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use super::manifest::VersionJson;
use crate::error::{Error, Result};
use crate::net::{self, Download};
use crate::paths::Paths;
use crate::progress::Progress;

const RUNTIME_INDEX: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Deserialize)]
struct RuntimeEntry {
    manifest: ManifestRef,
}

#[derive(Deserialize)]
struct ManifestRef {
    url: String,
    sha1: String,
}

#[derive(Deserialize)]
struct RuntimeManifest {
    files: HashMap<String, RuntimeFile>,
}

#[derive(Deserialize)]
struct RuntimeFile {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    executable: bool,
    #[serde(default)]
    downloads: Option<RuntimeDownloads>,
    #[serde(default)]
    target: Option<String>,
}

#[derive(Deserialize)]
struct RuntimeDownloads {
    raw: RawDownload,
}

#[derive(Deserialize)]
struct RawDownload {
    url: String,
    sha1: String,
    size: u64,
}

type RuntimeIndex = HashMap<String, HashMap<String, Vec<RuntimeEntry>>>;

/// Plattform-Schlüssel in Mojangs Index, bevorzugte zuerst.
fn platforms(prefer_x64_on_arm_mac: bool) -> Vec<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => vec!["windows-arm64", "windows-x64"],
        ("windows", "x86") => vec!["windows-x86"],
        ("windows", _) => vec!["windows-x64"],
        ("macos", "aarch64") if prefer_x64_on_arm_mac => vec!["mac-os"],
        ("macos", "aarch64") => vec!["mac-os-arm64", "mac-os"],
        ("macos", _) => vec!["mac-os"],
        ("linux", "x86") => vec!["linux-i386"],
        _ => vec!["linux"],
    }
}

fn java_binary(home: &std::path::Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        home.join("bin").join("javaw.exe")
    } else if cfg!(target_os = "macos") {
        home.join("jre.bundle/Contents/Home/bin/java")
    } else {
        home.join("bin").join("java")
    }
}

/// Stellt sicher, dass die passende Java-Version installiert ist, und gibt den Pfad zurück.
pub async fn ensure_java(
    http: &reqwest::Client,
    paths: &Paths,
    version: &VersionJson,
    concurrency: usize,
    progress: &Progress,
) -> Result<PathBuf> {
    let component = version
        .java_version
        .as_ref()
        .map(|j| j.component.clone())
        .unwrap_or_else(|| "jre-legacy".into());

    // Alte Versionen mit LWJGL 2 haben nur x64-Natives → auf Apple Silicon x64-Java (Rosetta).
    let uses_legacy_natives = version.libraries.iter().any(|l| l.natives.is_some());

    progress.stage("Java wird geprüft");
    let index: RuntimeIndex =
        net::get_json_cached(http, RUNTIME_INDEX, &paths.cache().join("java_runtimes.json")).await?;

    let (platform, entry) = platforms(uses_legacy_natives)
        .into_iter()
        .find_map(|p| {
            index
                .get(p)
                .and_then(|m| m.get(&component))
                .and_then(|list| list.first())
                .map(|e| (p, e))
        })
        .ok_or_else(|| {
            Error::msg(format!("Keine Java-Laufzeit ({component}) für dieses System verfügbar"))
        })?;

    let home = paths.runtimes().join(&component).join(platform);
    let marker = home.join(".ablc-version");
    let binary = java_binary(&home);
    if std::fs::read_to_string(&marker).ok().as_deref() == Some(entry.manifest.sha1.as_str())
        && binary.exists()
    {
        return Ok(binary);
    }

    progress.stage("Java wird heruntergeladen");
    let manifest: RuntimeManifest = net::get_json(http, &entry.manifest.url).await?;
    let mut downloads = Vec::new();
    let mut links = Vec::new();
    for (rel, file) in &manifest.files {
        let path = home.join(rel);
        match file.kind.as_str() {
            "directory" => std::fs::create_dir_all(&path)?,
            "file" => {
                if let Some(d) = &file.downloads {
                    let mut dl = Download::new(d.raw.url.clone(), path)
                        .sha1(Some(d.raw.sha1.clone()))
                        .size(Some(d.raw.size));
                    dl.executable = file.executable;
                    downloads.push(dl);
                }
            }
            "link" => {
                if let Some(target) = &file.target {
                    links.push((path, target.clone()));
                }
            }
            _ => {}
        }
    }
    net::download_all(http, downloads, concurrency, progress, "Java").await?;

    #[cfg(unix)]
    for (path, target) in links {
        if path.symlink_metadata().is_err() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::os::unix::fs::symlink(target, &path)?;
        }
    }
    #[cfg(not(unix))]
    drop(links);

    std::fs::write(&marker, &entry.manifest.sha1)?;
    if !binary.exists() {
        return Err(Error::msg("Java wurde installiert, aber die Programmdatei fehlt"));
    }
    Ok(binary)
}
