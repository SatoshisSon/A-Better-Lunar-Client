use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::paths::{read_json_or_default, write_json, Paths};

/// Beim Bauen per Umgebungsvariable vorbelegbar (z. B. in GitHub Actions).
const DEFAULT_MS_CLIENT_ID: &str = match option_env!("ABLC_MS_CLIENT_ID") {
    Some(id) => id,
    None => "",
};
const DEFAULT_DISCORD_APP_ID: &str = match option_env!("ABLC_DISCORD_APP_ID") {
    Some(id) => id,
    None => "",
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub ms_client_id: String,
    pub discord_enabled: bool,
    pub discord_app_id: String,
    pub default_memory_mb: u32,
    pub optimized_jvm_flags: bool,
    pub java_path: String,
    pub hide_launcher_on_launch: bool,
    pub show_snapshots: bool,
    pub concurrent_downloads: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ms_client_id: DEFAULT_MS_CLIENT_ID.into(),
            discord_enabled: false,
            discord_app_id: DEFAULT_DISCORD_APP_ID.into(),
            default_memory_mb: 4096,
            optimized_jvm_flags: true,
            java_path: String::new(),
            hide_launcher_on_launch: false,
            show_snapshots: false,
            concurrent_downloads: 32,
        }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Self {
        let mut s: Settings = read_json_or_default(&paths.file("settings.json"));
        if s.ms_client_id.is_empty() {
            s.ms_client_id = DEFAULT_MS_CLIENT_ID.into();
        }
        if s.discord_app_id.is_empty() {
            s.discord_app_id = DEFAULT_DISCORD_APP_ID.into();
        }
        s
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_json(&paths.file("settings.json"), self)
    }
}
