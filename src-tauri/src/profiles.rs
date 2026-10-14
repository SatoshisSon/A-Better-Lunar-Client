//! Profile (Instanzen): eigene Version, eigener Loader, eigene Mods und Welten.

use serde::{Deserialize, Serialize};

use crate::auth::now;
use crate::error::{Error, Result};
use crate::minecraft::loader::Loader;
use crate::paths::{read_json_or_default, write_json, Paths};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub game_version: String,
    #[serde(default)]
    pub loader: Loader,
    /// `None` = immer die neueste stabile Loader-Version.
    #[serde(default)]
    pub loader_version: Option<String>,
    /// `None` = Standard aus den Einstellungen.
    #[serde(default)]
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub jvm_args: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub last_played: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewProfile {
    pub name: String,
    pub game_version: String,
    pub loader: Loader,
    #[serde(default)]
    pub loader_version: Option<String>,
    #[serde(default)]
    pub memory_mb: Option<u32>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProfileStore {
    #[serde(default)]
    pub selected: Option<String>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

impl ProfileStore {
    pub fn load(paths: &Paths) -> Self {
        read_json_or_default(&paths.file("profiles.json"))
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_json(&paths.file("profiles.json"), self)
    }

    pub fn get(&self, id: &str) -> Result<&Profile> {
        self.profiles.iter().find(|p| p.id == id).ok_or_else(|| Error::msg("Profil nicht gefunden"))
    }

    pub fn get_mut(&mut self, id: &str) -> Result<&mut Profile> {
        self.profiles.iter_mut().find(|p| p.id == id).ok_or_else(|| Error::msg("Profil nicht gefunden"))
    }

    pub fn create(&mut self, paths: &Paths, new: NewProfile) -> Result<Profile> {
        let name = new.name.trim();
        if name.is_empty() {
            return Err(Error::msg("Bitte einen Profilnamen angeben"));
        }
        let profile = Profile {
            id: uuid::Uuid::new_v4().simple().to_string(),
            name: name.to_string(),
            game_version: new.game_version,
            loader: new.loader,
            loader_version: new.loader_version.filter(|v| !v.is_empty()),
            memory_mb: new.memory_mb,
            jvm_args: String::new(),
            width: None,
            height: None,
            created_at: now(),
            last_played: None,
        };
        let dir = paths.instance(&profile.id);
        for sub in ["mods", "resourcepacks", "shaderpacks", "saves"] {
            std::fs::create_dir_all(dir.join(sub))?;
        }
        self.selected = Some(profile.id.clone());
        self.profiles.push(profile.clone());
        Ok(profile)
    }

    pub fn update(&mut self, profile: Profile) -> Result<()> {
        let existing = self.get_mut(&profile.id)?;
        *existing = Profile {
            created_at: existing.created_at,
            last_played: existing.last_played,
            ..profile
        };
        Ok(())
    }

    pub fn delete(&mut self, paths: &Paths, id: &str, delete_files: bool) -> Result<()> {
        self.profiles.retain(|p| p.id != id);
        if self.selected.as_deref() == Some(id) {
            self.selected = self.profiles.first().map(|p| p.id.clone());
        }
        if delete_files {
            let dir = paths.instance(id);
            if dir.exists() {
                std::fs::remove_dir_all(dir)?;
            }
        }
        Ok(())
    }
}
