//! Mod-Manager auf Basis von Modrinth: Presets, Suche, Installation, Updates.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::minecraft::loader::Loader;
use crate::net::{self, Download};
use crate::paths::{read_json_or_default, write_json, Paths};
use crate::profiles::Profile;
use crate::progress::Progress;

const API: &str = "https://api.modrinth.com/v2";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Mod,
    Shader,
    ResourcePack,
}

impl ContentKind {
    fn folder(self) -> &'static str {
        match self {
            ContentKind::Mod => "mods",
            ContentKind::Shader => "shaderpacks",
            ContentKind::ResourcePack => "resourcepacks",
        }
    }
    fn project_type(self) -> &'static str {
        match self {
            ContentKind::Mod => "mod",
            ContentKind::Shader => "shader",
            ContentKind::ResourcePack => "resourcepack",
        }
    }
    fn loaders(self, loader: Loader) -> Option<Vec<&'static str>> {
        match self {
            ContentKind::Mod => loader.modrinth_name().map(|l| vec![l]),
            ContentKind::Shader => Some(vec!["iris"]),
            ContentKind::ResourcePack => Some(vec!["minecraft"]),
        }
    }
}

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub projects: &'static [&'static str],
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "performance",
        name: "Performance",
        description: "Deutlich mehr FPS, weniger RAM und kürzere Ladezeiten.",
        projects: &[
            "sodium",
            "lithium",
            "ferrite-core",
            "entityculling",
            "immediatelyfast",
            "modernfix",
            "dynamic-fps",
        ],
    },
    Preset {
        id: "visuals",
        name: "Shader & Optik",
        description: "Shader-Unterstützung, Zoom, dynamisches Licht, verbundene Texturen.",
        projects: &[
            "iris",
            "sodium-extra",
            "reeses-sodium-options",
            "zoomify",
            "lambdynamiclights",
            "continuity",
        ],
    },
    Preset {
        id: "comfort",
        name: "Komfort",
        description: "Mod-Menü im Spiel und bessere Inventar-Bedienung.",
        projects: &["modmenu", "mouse-tweaks"],
    },
];

// ---------------------------------------------------------------------------
// Modrinth-API
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct MrVersion {
    id: String,
    project_id: String,
    version_number: String,
    #[serde(default)]
    version_type: String,
    files: Vec<MrFile>,
    #[serde(default)]
    dependencies: Vec<MrDependency>,
}

#[derive(Debug, Clone, Deserialize)]
struct MrFile {
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    hashes: MrHashes,
    size: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct MrHashes {
    sha1: String,
}

#[derive(Debug, Clone, Deserialize)]
struct MrDependency {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    version_id: Option<String>,
    dependency_type: String,
}

#[derive(Debug, Clone, Deserialize)]
struct MrProject {
    id: String,
    slug: String,
    title: String,
    #[serde(default)]
    icon_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub author: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub total_hits: u64,
}

pub async fn search(
    http: &reqwest::Client,
    profile: &Profile,
    kind: ContentKind,
    query: &str,
    offset: u32,
) -> Result<SearchResult> {
    let mut facets = vec![
        vec![format!("project_type:{}", kind.project_type())],
        vec![format!("versions:{}", profile.game_version)],
    ];
    if kind == ContentKind::Mod {
        let loader = profile
            .loader
            .modrinth_name()
            .ok_or_else(|| Error::msg("Vanilla-Profile unterstützen keine Mods. Wähle Fabric."))?;
        facets.push(vec![format!("categories:{loader}")]);
    }
    let facets = serde_json::to_string(&facets)?;
    let index = if query.trim().is_empty() { "downloads" } else { "relevance" };
    Ok(http
        .get(format!("{API}/search"))
        .query(&[
            ("query", query),
            ("facets", facets.as_str()),
            ("index", index),
            ("limit", "20"),
            ("offset", &offset.to_string()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

async fn project(http: &reqwest::Client, id: &str) -> Result<MrProject> {
    net::get_json(http, &format!("{API}/project/{id}")).await
}

/// Passende (neueste, bevorzugt stabile) Version eines Projekts finden.
async fn best_version(
    http: &reqwest::Client,
    project_id: &str,
    profile: &Profile,
    kind: ContentKind,
) -> Result<Option<MrVersion>> {
    let loaders = kind.loaders(profile.loader);
    let query = |with_game_version: bool| {
        let mut q: Vec<(&str, String)> = Vec::new();
        if let Some(l) = &loaders {
            q.push(("loaders", serde_json::to_string(l).unwrap()));
        }
        if with_game_version {
            q.push(("game_versions", serde_json::to_string(&[&profile.game_version]).unwrap()));
        }
        q
    };
    let url = format!("{API}/project/{project_id}/version");
    let mut versions: Vec<MrVersion> =
        http.get(&url).query(&query(true)).send().await?.error_for_status()?.json().await?;
    // Shader und Ressourcenpakete funktionieren meist versionsübergreifend.
    if versions.is_empty() && kind != ContentKind::Mod {
        versions = http.get(&url).query(&query(false)).send().await?.error_for_status()?.json().await?;
    }
    let pick = versions.iter().position(|v| v.version_type == "release").unwrap_or(0);
    Ok(if versions.is_empty() { None } else { Some(versions.swap_remove(pick)) })
}

// ---------------------------------------------------------------------------
// Installierte Inhalte
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledProject {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub kind: ContentKind,
    pub version_id: String,
    pub version_number: String,
    pub filename: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    /// Automatisch als Abhängigkeit installiert.
    #[serde(default)]
    pub dependency: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ContentIndex {
    #[serde(default)]
    projects: Vec<InstalledProject>,
}

fn index_path(game_dir: &Path) -> PathBuf {
    game_dir.join("ablc-content.json")
}

fn load_index(game_dir: &Path) -> ContentIndex {
    read_json_or_default(&index_path(game_dir))
}

fn save_index(game_dir: &Path, index: &ContentIndex) -> Result<()> {
    write_json(&index_path(game_dir), index)
}

/// Eintrag in der Mod-Liste der Oberfläche.
#[derive(Debug, Clone, Serialize)]
pub struct ContentItem {
    pub filename: String,
    pub enabled: bool,
    pub title: String,
    pub version: Option<String>,
    pub project_id: Option<String>,
    pub icon_url: Option<String>,
    pub dependency: bool,
}

const DISABLED: &str = ".disabled";

pub fn list(paths: &Paths, profile_id: &str, kind: ContentKind) -> Result<Vec<ContentItem>> {
    let game_dir = paths.instance(profile_id);
    let dir = game_dir.join(kind.folder());
    let index = load_index(&game_dir);
    let mut items = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(items) };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with('.') {
            continue;
        }
        let enabled = !file_name.ends_with(DISABLED);
        let base = file_name.trim_end_matches(DISABLED).to_string();
        let meta = index.projects.iter().find(|p| p.filename == base);
        items.push(ContentItem {
            title: meta.map(|m| m.title.clone()).unwrap_or_else(|| base.clone()),
            version: meta.map(|m| m.version_number.clone()),
            project_id: meta.map(|m| m.project_id.clone()),
            icon_url: meta.and_then(|m| m.icon_url.clone()),
            dependency: meta.is_some_and(|m| m.dependency),
            filename: base,
            enabled,
        });
    }
    items.sort_by_key(|i| (i.dependency, i.title.to_lowercase()));
    Ok(items)
}

pub fn set_enabled(paths: &Paths, profile_id: &str, kind: ContentKind, filename: &str, enabled: bool) -> Result<()> {
    let dir = paths.instance(profile_id).join(kind.folder());
    let on = dir.join(filename);
    let off = dir.join(format!("{filename}{DISABLED}"));
    match (enabled, on.exists(), off.exists()) {
        (true, false, true) => std::fs::rename(off, on)?,
        (false, true, false) => std::fs::rename(on, off)?,
        _ => {}
    }
    Ok(())
}

pub fn remove(paths: &Paths, profile_id: &str, kind: ContentKind, filename: &str) -> Result<()> {
    let game_dir = paths.instance(profile_id);
    let dir = game_dir.join(kind.folder());
    for p in [dir.join(filename), dir.join(format!("{filename}{DISABLED}"))] {
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    let mut index = load_index(&game_dir);
    index.projects.retain(|p| p.filename != filename);
    save_index(&game_dir, &index)
}

/// Installiert ein Projekt (inkl. benötigter Abhängigkeiten). Gibt `false`
/// zurück, wenn es keine passende Version für das Profil gibt.
pub async fn install(
    http: &reqwest::Client,
    paths: &Paths,
    profile: &Profile,
    project_id: &str,
    kind: ContentKind,
) -> Result<bool> {
    let game_dir = paths.instance(&profile.id);
    let mut index = load_index(&game_dir);
    let mut visiting = HashSet::new();
    let ok = install_rec(http, &game_dir, profile, project_id, None, kind, Mode::User, &mut index, &mut visiting).await?;
    save_index(&game_dir, &index)?;
    Ok(ok)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Vom Nutzer gewählt.
    User,
    /// Automatisch als Abhängigkeit.
    Dependency,
    /// Aktualisierung eines vorhandenen Eintrags (Markierungen bleiben).
    Update,
}

#[allow(clippy::too_many_arguments)]
async fn install_rec(
    http: &reqwest::Client,
    game_dir: &Path,
    profile: &Profile,
    project_id: &str,
    version_id: Option<&str>,
    kind: ContentKind,
    mode: Mode,
    index: &mut ContentIndex,
    visiting: &mut HashSet<String>,
) -> Result<bool> {
    let info = project(http, project_id).await?;
    if !visiting.insert(info.id.clone()) {
        return Ok(true);
    }
    // Abhängigkeiten, die schon installiert sind, nicht anfassen.
    if mode == Mode::Dependency && index.projects.iter().any(|p| p.project_id == info.id) {
        return Ok(true);
    }

    let version = match version_id {
        Some(vid) => Some(net::get_json::<MrVersion>(http, &format!("{API}/version/{vid}")).await?),
        None => best_version(http, &info.id, profile, kind).await?,
    };
    let Some(version) = version else { return Ok(false) };

    // Benötigte Abhängigkeiten zuerst.
    if kind == ContentKind::Mod {
        for dep in version.dependencies.iter().filter(|d| d.dependency_type == "required") {
            let dep_project = match (&dep.project_id, &dep.version_id) {
                (Some(p), _) => p.clone(),
                (None, Some(v)) => {
                    net::get_json::<MrVersion>(http, &format!("{API}/version/{v}")).await?.project_id
                }
                _ => continue,
            };
            Box::pin(install_rec(
                http,
                game_dir,
                profile,
                &dep_project,
                dep.version_id.as_deref(),
                kind,
                Mode::Dependency,
                index,
                visiting,
            ))
            .await?;
        }
    }

    let file = version
        .files
        .iter()
        .find(|f| f.primary)
        .or(version.files.first())
        .ok_or_else(|| Error::msg(format!("{} hat keine Datei", info.title)))?;
    let dir = game_dir.join(kind.folder());

    // Alte Version desselben Projekts entfernen.
    let old = index.projects.iter().position(|p| p.project_id == info.id);
    let mut was_enabled = true;
    let mut was_dependency = mode == Mode::Dependency;
    if let Some(i) = old {
        let prev = index.projects.remove(i);
        was_dependency = prev.dependency && mode != Mode::User;
        if prev.filename != file.filename {
            let _ = std::fs::remove_file(dir.join(&prev.filename));
            let disabled = dir.join(format!("{}{DISABLED}", prev.filename));
            if disabled.exists() {
                was_enabled = false;
                let _ = std::fs::remove_file(disabled);
            }
        }
    }

    let target = dir.join(&file.filename);
    net::download_all(
        http,
        vec![Download::new(file.url.clone(), target.clone())
            .sha1(Some(file.hashes.sha1.clone()))
            .size(Some(file.size))],
        1,
        &Progress::silent(),
        "",
    )
    .await?;
    if !was_enabled {
        std::fs::rename(&target, dir.join(format!("{}{DISABLED}", file.filename)))?;
    }

    index.projects.push(InstalledProject {
        project_id: info.id,
        slug: info.slug,
        title: info.title,
        kind,
        version_id: version.id,
        version_number: version.version_number,
        filename: file.filename.clone(),
        icon_url: info.icon_url,
        dependency: was_dependency,
    });
    Ok(true)
}

#[derive(Debug, Default, Serialize)]
pub struct PresetReport {
    pub installed: Vec<String>,
    pub unavailable: Vec<String>,
}

pub async fn install_presets(
    http: &reqwest::Client,
    paths: &Paths,
    profile: &Profile,
    preset_ids: &[String],
    progress: &Progress,
) -> Result<PresetReport> {
    if profile.loader == Loader::Vanilla {
        return Err(Error::msg("Mod-Presets brauchen einen Mod-Loader (Fabric)."));
    }
    let slugs: Vec<&str> = PRESETS
        .iter()
        .filter(|p| preset_ids.iter().any(|id| id == p.id))
        .flat_map(|p| p.projects.iter().copied())
        .collect();
    let mut report = PresetReport::default();
    for (i, slug) in slugs.iter().enumerate() {
        progress.update(&format!("Mods werden installiert: {slug}"), i, slugs.len());
        match install(http, paths, profile, slug, ContentKind::Mod).await {
            Ok(true) => report.installed.push(slug.to_string()),
            Ok(false) => report.unavailable.push(slug.to_string()),
            Err(Error::Http(e)) if e.status().map(|s| s.as_u16()) == Some(404) => {
                report.unavailable.push(slug.to_string())
            }
            Err(e) => return Err(e),
        }
    }
    progress.update("Mods installiert", slugs.len(), slugs.len());
    Ok(report)
}

/// Aktualisiert alle über Modrinth installierten Inhalte auf die neueste
/// passende Version (z. B. nach einem Versionswechsel des Profils).
pub async fn update_all(
    http: &reqwest::Client,
    paths: &Paths,
    profile: &Profile,
    progress: &Progress,
) -> Result<PresetReport> {
    let game_dir = paths.instance(&profile.id);
    let index = load_index(&game_dir);
    let mut report = PresetReport::default();
    let total = index.projects.len();
    for (i, p) in index.projects.iter().enumerate() {
        progress.update(&format!("Prüfe {}", p.title), i, total);
        match best_version(http, &p.project_id, profile, p.kind).await? {
            Some(v) if v.id != p.version_id => {
                let mut idx = load_index(&game_dir);
                let mut visiting = HashSet::new();
                install_rec(http, &game_dir, profile, &p.project_id, Some(&v.id), p.kind, Mode::Update, &mut idx, &mut visiting)
                    .await?;
                save_index(&game_dir, &idx)?;
                report.installed.push(p.title.clone());
            }
            Some(_) => {}
            None => report.unavailable.push(p.title.clone()),
        }
    }
    progress.update("Fertig", total, total);
    Ok(report)
}
