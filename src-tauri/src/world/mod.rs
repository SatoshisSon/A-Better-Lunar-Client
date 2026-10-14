//! Welten finden und auslesen: Einzelspieler-Welten aller Profile und
//! eingetragene Server-Welten (Welt-Ordner deines eigenen Servers).

pub mod analysis;
pub mod anvil;
pub mod nbt;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::paths::{read_json_or_default, write_json, Paths};
use crate::profiles::ProfileStore;
use nbt::Tag;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerWorld {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct WorldStore {
    #[serde(default)]
    servers: Vec<ServerWorld>,
}

fn store_path(paths: &Paths) -> PathBuf {
    paths.file("worlds.json")
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldEntry {
    pub id: String,
    pub name: String,
    /// „singleplayer“ oder „server“
    pub source: &'static str,
    pub profile: Option<String>,
    pub path: String,
    pub version: Option<String>,
    pub last_played: Option<i64>,
    pub game_mode: Option<String>,
    pub spawn: Option<[i64; 3]>,
    pub available: bool,
}

/// Grunddaten aus `level.dat`.
#[derive(Debug, Default, Clone)]
pub struct LevelInfo {
    pub name: Option<String>,
    pub version: Option<String>,
    pub data_version: Option<i64>,
    pub last_played: Option<i64>,
    pub game_mode: Option<String>,
    pub spawn: Option<[i64; 3]>,
    pub day_time: Option<i64>,
    pub raining: bool,
    pub player: Option<Tag>,
}

pub fn read_level(world: &Path) -> Option<LevelInfo> {
    let bytes = std::fs::read(world.join("level.dat")).ok()?;
    let root = nbt::parse_compressed(&bytes).ok()?;
    let d = root.get("Data")?;
    let int = |k: &str| d.get(k).and_then(Tag::as_i64);
    let spawn = match (int("SpawnX"), int("SpawnY"), int("SpawnZ")) {
        (Some(x), Some(y), Some(z)) => Some([x, y, z]),
        // 1.21.5+: spawn = { pos: [I; x, y, z] }
        _ => match d.path(&["spawn", "pos"]) {
            Some(Tag::IntArray(p)) if p.len() == 3 => Some([p[0] as i64, p[1] as i64, p[2] as i64]),
            _ => None,
        },
    };
    let game_mode = int("GameType").map(|g| {
        match g {
            0 => "Überleben",
            1 => "Kreativ",
            2 => "Abenteuer",
            3 => "Zuschauer",
            _ => "?",
        }
        .to_string()
    });
    Some(LevelInfo {
        name: d.get("LevelName").and_then(Tag::as_str).map(String::from),
        version: d.path(&["Version", "Name"]).and_then(Tag::as_str).map(String::from),
        data_version: int("DataVersion"),
        last_played: int("LastPlayed"),
        game_mode,
        spawn,
        day_time: int("DayTime"),
        raining: int("raining").unwrap_or(0) != 0,
        player: d.get("Player").cloned(),
    })
}

fn entry(id: String, name: String, source: &'static str, profile: Option<String>, path: &Path) -> WorldEntry {
    let info = read_level(path);
    WorldEntry {
        id,
        name: info.as_ref().and_then(|i| i.name.clone()).filter(|n| !n.is_empty()).unwrap_or(name),
        source,
        profile,
        path: path.to_string_lossy().into_owned(),
        version: info.as_ref().and_then(|i| i.version.clone()),
        last_played: info.as_ref().and_then(|i| i.last_played),
        game_mode: info.as_ref().and_then(|i| i.game_mode.clone()),
        spawn: info.as_ref().and_then(|i| i.spawn),
        available: path.join("region").is_dir() || path.join("level.dat").is_file(),
    }
}

pub fn list_all(paths: &Paths) -> Vec<WorldEntry> {
    let mut out = Vec::new();
    for p in ProfileStore::load(paths).profiles {
        let saves = paths.instance(&p.id).join("saves");
        let Ok(dirs) = std::fs::read_dir(&saves) else { continue };
        for d in dirs.flatten() {
            let path = d.path();
            if !path.join("level.dat").is_file() {
                continue;
            }
            let folder = d.file_name().to_string_lossy().into_owned();
            out.push(entry(format!("{}/{folder}", p.id), folder, "singleplayer", Some(p.name.clone()), &path));
        }
    }
    let store: WorldStore = read_json_or_default(&store_path(paths));
    for s in store.servers {
        out.push(entry(s.id.clone(), s.name.clone(), "server", None, Path::new(&s.path)));
    }
    out.sort_by_key(|w| std::cmp::Reverse(w.last_played));
    out
}

/// Welt anhand von ID oder Name finden (Groß-/Kleinschreibung egal).
pub fn find(paths: &Paths, query: &str) -> Result<WorldEntry> {
    let all = list_all(paths);
    let q = query.trim().to_lowercase();
    if let Some(w) = all.iter().find(|w| w.id == query.trim()) {
        return Ok(w.clone());
    }
    let exact: Vec<_> = all.iter().filter(|w| w.name.to_lowercase() == q).collect();
    if exact.len() == 1 {
        return Ok(exact[0].clone());
    }
    let partial: Vec<_> = all.iter().filter(|w| w.name.to_lowercase().contains(&q)).collect();
    match (exact.len(), partial.len()) {
        (0, 1) => Ok(partial[0].clone()),
        (0, 0) => Err(Error::msg(format!("Keine Welt „{query}“ gefunden. Nutze list_worlds."))),
        _ => Err(Error::msg(format!(
            "„{query}“ ist mehrdeutig. Bitte die ID angeben: {}",
            exact.iter().chain(partial.iter()).map(|w| format!("{} ({})", w.name, w.id)).collect::<Vec<_>>().join(", ")
        ))),
    }
}

pub fn add_server(paths: &Paths, name: &str, path: &str) -> Result<ServerWorld> {
    let dir = Path::new(path.trim());
    if !dir.join("level.dat").is_file() && !dir.join("region").is_dir() {
        return Err(Error::msg(
            "Das ist kein Welt-Ordner. Wähle den Ordner, der „level.dat“ und „region“ enthält (meist „world“).",
        ));
    }
    let mut store: WorldStore = read_json_or_default(&store_path(paths));
    let world = ServerWorld {
        id: format!("server/{}", uuid::Uuid::new_v4().simple()),
        name: if name.trim().is_empty() { "Server-Welt".into() } else { name.trim().to_string() },
        path: dir.to_string_lossy().into_owned(),
    };
    store.servers.push(world.clone());
    write_json(&store_path(paths), &store)?;
    Ok(world)
}

pub fn remove_server(paths: &Paths, id: &str) -> Result<()> {
    let mut store: WorldStore = read_json_or_default(&store_path(paths));
    store.servers.retain(|s| s.id != id);
    write_json(&store_path(paths), &store)
}

#[derive(Debug, Clone)]
pub struct PlayerPos {
    pub name: String,
    pub pos: [f64; 3],
    pub dimension: String,
}

fn player_from_tag(name: String, t: &Tag) -> Option<PlayerPos> {
    let pos = t.get("Pos")?.as_list()?;
    let p: Vec<f64> = pos.iter().filter_map(Tag::as_f64).collect();
    if p.len() != 3 {
        return None;
    }
    let dimension = match t.get("Dimension") {
        Some(Tag::String(s)) => s.trim_start_matches("minecraft:").to_string(),
        Some(other) => match other.as_i64() {
            Some(-1) => "the_nether".into(),
            Some(1) => "the_end".into(),
            _ => "overworld".into(),
        },
        None => "overworld".into(),
    };
    Some(PlayerPos { name, pos: [p[0], p[1], p[2]], dimension })
}

/// Spielerpositionen: Einzelspieler aus `level.dat`, Server aus `playerdata/`.
pub fn players(world: &Path, level: Option<&LevelInfo>) -> Vec<PlayerPos> {
    let mut out = Vec::new();
    if let Some(p) = level.and_then(|l| l.player.as_ref()).and_then(|t| player_from_tag("Du".into(), t)) {
        out.push(p);
    }
    // Namen aus der usercache.json des Servers (liegt neben dem Welt-Ordner).
    let names: Vec<serde_json::Value> = world
        .parent()
        .and_then(|p| std::fs::read(p.join("usercache.json")).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let name_of = |uuid: &str| {
        names
            .iter()
            .find(|e| e["uuid"].as_str().is_some_and(|u| u.eq_ignore_ascii_case(uuid)))
            .and_then(|e| e["name"].as_str())
            .map(String::from)
    };
    if let Ok(files) = std::fs::read_dir(world.join("playerdata")) {
        for f in files.flatten().take(200) {
            let fname = f.file_name().to_string_lossy().into_owned();
            let Some(uuid) = fname.strip_suffix(".dat") else { continue };
            let Some(tag) = std::fs::read(f.path()).ok().and_then(|b| nbt::parse_compressed(&b).ok()) else { continue };
            let name = name_of(uuid).unwrap_or_else(|| uuid.to_string());
            if let Some(p) = player_from_tag(name, &tag) {
                if !out.iter().any(|o| o.pos == p.pos) {
                    out.push(p);
                }
            }
        }
    }
    out
}
