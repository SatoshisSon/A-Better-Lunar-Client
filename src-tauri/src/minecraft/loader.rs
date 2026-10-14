//! Mod-Loader: Fabric (ab 1.14) und Legacy Fabric (z. B. 1.8.9).

use serde::{Deserialize, Serialize};

use super::manifest::{Arguments, VersionJson};
use crate::error::{Error, Result};
use crate::net;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Loader {
    #[default]
    Vanilla,
    Fabric,
    LegacyFabric,
}

impl Loader {
    fn meta_base(self) -> Option<&'static str> {
        match self {
            Loader::Vanilla => None,
            Loader::Fabric => Some("https://meta.fabricmc.net/v2"),
            Loader::LegacyFabric => Some("https://meta.legacyfabric.net/v2"),
        }
    }

    /// Loader-Name für die Modrinth-API.
    pub fn modrinth_name(self) -> Option<&'static str> {
        match self {
            Loader::Vanilla => None,
            Loader::Fabric => Some("fabric"),
            Loader::LegacyFabric => Some("legacy-fabric"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GameVersion {
    version: String,
}

#[derive(Debug, Deserialize)]
struct LoaderEntry {
    loader: LoaderVersion,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoaderVersion {
    pub version: String,
    #[serde(default)]
    pub stable: bool,
}

/// Alle Minecraft-Versionen, die der Loader unterstützt.
pub async fn supported_game_versions(http: &reqwest::Client, loader: Loader) -> Result<Vec<String>> {
    let Some(base) = loader.meta_base() else { return Ok(vec![]) };
    let list: Vec<GameVersion> = net::get_json(http, &format!("{base}/versions/game")).await?;
    Ok(list.into_iter().map(|g| g.version).collect())
}

/// Loader-Versionen für eine Minecraft-Version (neueste zuerst).
pub async fn loader_versions(
    http: &reqwest::Client,
    loader: Loader,
    game_version: &str,
) -> Result<Vec<LoaderVersion>> {
    let Some(base) = loader.meta_base() else { return Ok(vec![]) };
    let list: Vec<LoaderEntry> =
        net::get_json(http, &format!("{base}/versions/loader/{game_version}")).await?;
    Ok(list.into_iter().map(|e| e.loader).collect())
}

/// Lädt das Loader-Profil (ein Versions-JSON, das von Vanilla erbt).
pub async fn loader_profile(
    http: &reqwest::Client,
    loader: Loader,
    game_version: &str,
    loader_version: Option<&str>,
) -> Result<VersionJson> {
    let base = loader.meta_base().ok_or_else(|| Error::msg("Kein Loader"))?;
    let version = match loader_version {
        Some(v) if !v.is_empty() => v.to_string(),
        _ => {
            let versions = loader_versions(http, loader, game_version).await?;
            versions
                .iter()
                .find(|v| v.stable)
                .or(versions.first())
                .map(|v| v.version.clone())
                .ok_or_else(|| {
                    Error::msg(format!("Kein Loader für Minecraft {game_version} verfügbar"))
                })?
        }
    };
    net::get_json(
        http,
        &format!("{base}/versions/loader/{game_version}/{version}/profile/json"),
    )
    .await
}

/// Führt ein Loader-Profil mit seinem Vanilla-Elternteil zusammen.
pub fn merge(parent: VersionJson, child: VersionJson) -> VersionJson {
    let mut seen = std::collections::HashSet::new();
    let libraries = child
        .libraries
        .into_iter()
        .chain(parent.libraries)
        .filter(|l| seen.insert(l.dedupe_key()))
        .collect();

    let arguments = match (parent.arguments, child.arguments) {
        (None, None) => None,
        (p, c) => {
            let p = p.unwrap_or_default();
            let c = c.unwrap_or_default();
            Some(Arguments {
                game: p.game.into_iter().chain(c.game).collect(),
                jvm: p.jvm.into_iter().chain(c.jvm).collect(),
            })
        }
    };

    VersionJson {
        id: child.id,
        inherits_from: None,
        main_class: if child.main_class.is_empty() { parent.main_class } else { child.main_class },
        minecraft_arguments: child.minecraft_arguments.or(parent.minecraft_arguments),
        arguments,
        libraries,
        asset_index: child.asset_index.or(parent.asset_index),
        assets: child.assets.or(parent.assets),
        downloads: parent.downloads,
        java_version: child.java_version.or(parent.java_version),
        logging: parent.logging,
        kind: parent.kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minecraft::manifest::Library;

    fn lib(name: &str) -> Library {
        serde_json::from_value(serde_json::json!({ "name": name })).unwrap()
    }

    #[test]
    fn child_libraries_win() {
        let parent = VersionJson {
            id: "1.21.1".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            libraries: vec![lib("org.ow2.asm:asm:9.1"), lib("com.google.code.gson:gson:2.10")],
            ..Default::default()
        };
        let child = VersionJson {
            id: "fabric-loader-0.16.5-1.21.1".into(),
            main_class: "net.fabricmc.loader.impl.launch.knot.KnotClient".into(),
            libraries: vec![lib("org.ow2.asm:asm:9.7"), lib("net.fabricmc:fabric-loader:0.16.5")],
            ..Default::default()
        };
        let merged = merge(parent, child);
        let names: Vec<_> = merged.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            ["org.ow2.asm:asm:9.7", "net.fabricmc:fabric-loader:0.16.5", "com.google.code.gson:gson:2.10"]
        );
        assert_eq!(merged.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient");
        assert_eq!(merged.id, "fabric-loader-0.16.5-1.21.1");
    }
}
