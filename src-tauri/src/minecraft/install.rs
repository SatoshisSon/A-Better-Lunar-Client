//! Lädt alles herunter, was eine Minecraft-Version zum Starten braucht.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::loader::{self, Loader};
use super::manifest::*;
use super::rules;
use crate::error::{Error, Result};
use crate::net::{self, Download};
use crate::paths::Paths;
use crate::progress::Progress;

const RESOURCES_URL: &str = "https://resources.download.minecraft.net";

pub async fn version_manifest(http: &reqwest::Client, paths: &Paths) -> Result<VersionManifest> {
    net::get_json_cached(http, VERSION_MANIFEST_URL, &paths.cache().join("version_manifest_v2.json"))
        .await
}

/// Vanilla-Versions-JSON laden (aus dem Cache, wenn die Prüfsumme passt).
async fn vanilla_json(http: &reqwest::Client, paths: &Paths, id: &str) -> Result<VersionJson> {
    let file = paths.version_dir(id).join(format!("{id}.json"));
    let manifest = version_manifest(http, paths).await;
    let entry = manifest.ok().and_then(|m| m.versions.into_iter().find(|v| v.id == id));

    if let Some(entry) = &entry {
        let cached_ok = entry
            .sha1
            .as_ref()
            .is_some_and(|sha| net::sha1_file(&file).is_some_and(|h| &h == sha));
        if !cached_ok {
            return net::get_json_cached(http, &entry.url, &file).await;
        }
    }
    let bytes = std::fs::read(&file)
        .map_err(|_| Error::msg(format!("Minecraft {id} ist unbekannt oder nicht offline verfügbar")))?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Versions-JSON inkl. Mod-Loader zusammenbauen. Gibt (zusammengeführt, Vanilla-ID) zurück.
pub async fn resolve_version(
    http: &reqwest::Client,
    paths: &Paths,
    game_version: &str,
    loader: Loader,
    loader_version: Option<&str>,
) -> Result<VersionJson> {
    let vanilla = vanilla_json(http, paths, game_version).await?;
    if loader == Loader::Vanilla {
        return Ok(vanilla);
    }
    let cache = paths
        .cache()
        .join("loaders")
        .join(format!("{loader:?}-{game_version}-{}.json", loader_version.unwrap_or("latest")));
    let profile = match loader::loader_profile(http, loader, game_version, loader_version).await {
        Ok(p) => {
            let _ = crate::paths::write_json(&cache, &p);
            p
        }
        Err(e) => match std::fs::read(&cache) {
            Ok(b) => serde_json::from_slice(&b)?,
            Err(_) => return Err(e),
        },
    };
    Ok(loader::merge(vanilla, profile))
}

/// Ergebnis der Installation – alles, was zum Bauen der Startbefehle nötig ist.
pub struct Installed {
    pub classpath: Vec<PathBuf>,
    pub libraries_dir: PathBuf,
    pub natives_dir: PathBuf,
    pub assets_root: PathBuf,
    pub game_assets: PathBuf,
    pub assets_index_name: String,
    pub logging_arg: Option<String>,
}

pub async fn install(
    http: &reqwest::Client,
    paths: &Paths,
    game_version: &str,
    version: &VersionJson,
    game_dir: &Path,
    concurrency: usize,
    progress: &Progress,
) -> Result<Installed> {
    let libs_dir = paths.libraries();
    let mut downloads = Vec::new();
    let mut classpath = Vec::new();
    let mut native_jars: Vec<(PathBuf, Vec<String>)> = Vec::new();
    let mut seen = HashSet::new();

    // --- Libraries ---
    for lib in version.libraries.iter().filter(|l| rules::library_allowed(l)) {
        if let Some(art) = lib.main_artifact() {
            let Some(rel) = art.path.clone() else { continue };
            let path = libs_dir.join(&rel);
            if seen.insert(path.clone()) {
                downloads.push(Download::new(art.url, path.clone()).sha1(art.sha1).size(art.size));
                // Neue Natives-Jars (1.19+) bleiben im Classpath; LWJGL wählt die
                // passende Architektur selbst aus und entpackt sie.
                classpath.push(path);
            }
        }
        // Alte Natives (bis 1.18) über Klassifizierer.
        if let Some(classifier) = rules::native_classifier(lib) {
            let art = lib
                .downloads
                .as_ref()
                .and_then(|d| d.classifiers.as_ref())
                .and_then(|c| c.get(&classifier))
                .cloned();
            if let Some(art) = art {
                let rel = art
                    .path
                    .clone()
                    .or_else(|| Library::maven_path(&format!("{}:{classifier}", lib.name)))
                    .ok_or_else(|| Error::msg("Ungültiger Library-Pfad"))?;
                let path = libs_dir.join(rel);
                downloads.push(Download::new(art.url, path.clone()).sha1(art.sha1).size(art.size));
                let exclude = lib.extract.as_ref().map(|e| e.exclude.clone()).unwrap_or_default();
                native_jars.push((path, exclude));
            }
        }
    }

    // --- Client-Jar ---
    let client = version
        .downloads
        .as_ref()
        .and_then(|d| d.client.clone())
        .ok_or_else(|| Error::msg("Versions-JSON enthält keinen Client-Download"))?;
    let client_jar = paths.version_dir(game_version).join(format!("{game_version}.jar"));
    downloads.push(Download::new(client.url, client_jar.clone()).sha1(client.sha1).size(client.size));
    classpath.push(client_jar);

    // --- Logging-Konfiguration (behebt u. a. Log4Shell bei alten Versionen) ---
    let mut logging_arg = None;
    if let Some(lc) = version.logging.as_ref().and_then(|l| l.client.as_ref()) {
        let path = paths.assets().join("log_configs").join(&lc.file.id);
        downloads.push(
            Download::new(lc.file.url.clone(), path.clone()).sha1(lc.file.sha1.clone()).size(lc.file.size),
        );
        logging_arg = Some(lc.argument.replace("${path}", &path.to_string_lossy()));
    }

    // --- Asset-Index ---
    let index_ref = version
        .asset_index
        .clone()
        .ok_or_else(|| Error::msg("Versions-JSON enthält keinen Asset-Index"))?;
    let index_path = paths.assets().join("indexes").join(format!("{}.json", index_ref.id));
    progress.stage("Asset-Index wird geladen");
    net::download_all(
        http,
        vec![Download::new(index_ref.url.clone(), index_path.clone())
            .sha1(index_ref.sha1.clone())
            .size(index_ref.size)],
        1,
        progress,
        "Asset-Index",
    )
    .await?;
    let index: AssetIndex = serde_json::from_slice(&std::fs::read(&index_path)?)?;

    let objects_dir = paths.assets().join("objects");
    let mut seen_hashes = HashSet::new();
    for obj in index.objects.values() {
        if !seen_hashes.insert(obj.hash.clone()) {
            continue;
        }
        let prefix = &obj.hash[..2];
        downloads.push(
            Download::new(
                format!("{RESOURCES_URL}/{prefix}/{}", obj.hash),
                objects_dir.join(prefix).join(&obj.hash),
            )
            .sha1(Some(obj.hash.clone()))
            .size(Some(obj.size)),
        );
    }

    net::download_all(http, downloads, concurrency, progress, "Spieldateien").await?;

    // --- Alte Asset-Layouts (vor 1.7) ---
    let assets_root = paths.assets();
    let mut game_assets = assets_root.clone();
    if index.is_virtual || index.map_to_resources {
        let target = if index.map_to_resources {
            game_dir.join("resources")
        } else {
            assets_root.join("virtual").join(&index_ref.id)
        };
        progress.stage("Alte Asset-Struktur wird erstellt");
        let objects = index.objects.clone();
        let objects_dir = objects_dir.clone();
        let target2 = target.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            for (name, obj) in objects {
                let dest = target2.join(&name);
                if dest.exists() {
                    continue;
                }
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(objects_dir.join(&obj.hash[..2]).join(&obj.hash), dest)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| Error::msg(e.to_string()))??;
        game_assets = target;
    }

    // --- Natives entpacken ---
    progress.stage("Natives werden entpackt");
    let natives_dir = paths.natives(&version.id);
    let nd = natives_dir.clone();
    tokio::task::spawn_blocking(move || extract_natives(&native_jars, &nd))
        .await
        .map_err(|e| Error::msg(e.to_string()))??;

    Ok(Installed {
        classpath,
        libraries_dir: libs_dir,
        natives_dir,
        assets_root,
        game_assets,
        assets_index_name: index_ref.id,
        logging_arg,
    })
}

fn is_native_file(name: &str) -> bool {
    [".dll", ".so", ".dylib", ".jnilib"].iter().any(|ext| name.ends_with(ext))
}

fn extract_natives(jars: &[(PathBuf, Vec<String>)], dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    for (jar, exclude) in jars {
        let file = std::fs::File::open(jar)?;
        let mut archive = zip::ZipArchive::new(file)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            if entry.is_dir()
                || name.starts_with("META-INF/")
                || exclude.iter().any(|e| name.starts_with(e.as_str()))
                || !is_native_file(&name)
            {
                continue;
            }
            // Flach entpacken: LWJGL erwartet die Bibliotheken direkt im Ordner.
            let Some(file_name) = Path::new(&name).file_name() else { continue };
            let dest = dir.join(file_name);
            if dest.exists() && std::fs::metadata(&dest)?.len() == entry.size() {
                continue;
            }
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf)?;
            std::fs::write(dest, buf)?;
        }
    }
    Ok(())
}
