use std::path::{Path, PathBuf};

/// Alle Verzeichnisse des Launchers. Gemeinsame Spieldaten (Libraries, Assets,
/// Java) liegen unter `meta/`, jedes Profil hat seinen eigenen Spielordner.
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
}

/// Bundle-ID aus tauri.conf.json – bestimmt den Datenordner.
pub const APP_ID: &str = "io.github.satoshisson.ablc";

/// Datenordner (Windows: %APPDATA%\<id>, macOS: ~/Library/Application Support/<id>).
/// Wird auch vom MCP-Server benutzt, der ohne Tauri läuft.
pub fn default_root() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join(APP_ID)
}

impl Paths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn meta(&self) -> PathBuf {
        self.root.join("meta")
    }
    pub fn versions(&self) -> PathBuf {
        self.meta().join("versions")
    }
    pub fn version_dir(&self, id: &str) -> PathBuf {
        self.versions().join(id)
    }
    pub fn libraries(&self) -> PathBuf {
        self.meta().join("libraries")
    }
    pub fn assets(&self) -> PathBuf {
        self.meta().join("assets")
    }
    pub fn natives(&self, version_id: &str) -> PathBuf {
        self.meta().join("natives").join(version_id)
    }
    pub fn runtimes(&self) -> PathBuf {
        self.meta().join("runtimes")
    }
    pub fn cache(&self) -> PathBuf {
        self.meta().join("cache")
    }
    pub fn instances(&self) -> PathBuf {
        self.root.join("instances")
    }
    pub fn instance(&self, profile_id: &str) -> PathBuf {
        self.instances().join(profile_id)
    }
    pub fn file(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

/// JSON-Datei laden; fehlt sie oder ist sie kaputt, gibt es den Standardwert.
pub fn read_json_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// JSON atomar schreiben (erst temporäre Datei, dann umbenennen).
pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> crate::error::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
