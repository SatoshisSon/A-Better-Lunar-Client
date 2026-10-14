//! MCP-Server („Model Context Protocol“) für die Claude-Desktop-App.
//! Start: `a-better-lunar-client --mcp` – Kommunikation über stdin/stdout (JSON-RPC).
//! Claude kann damit deine Welten selbst auslesen und Bauideen entwickeln.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::paths::Paths;
use crate::world::{self, analysis, anvil::Dimension};

const SERVER_NAME: &str = "a-better-lunar-client";
const SUPPORTED_PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Dieser Server liest Minecraft-Welten des Nutzers aus (Einzelspieler-Welten und Welt-Ordner seines eigenen Servers), damit du Bauideen passend zum Gelände entwickeln kannst. \
Vorgehen: 1) list_worlds, 2) world_info für Spawn und Spielerpositionen, 3) area_overview für Gelände, Höhen, Biome und vorhandene Bauten, 4) get_blocks, um bestehende Bauten genau anzusehen. \
Koordinaten: x = Osten, z = Süden, y = Höhe. Die Daten zeigen den zuletzt gespeicherten Stand. \
Antworte auf Deutsch und gib bei Bauvorschlägen konkrete Koordinaten, Maße, Blocknamen und Materialmengen an.";

fn world_param() -> Value {
    json!({ "type": "string", "description": "Name oder ID der Welt (aus list_worlds)" })
}

fn dim_param() -> Value {
    json!({ "type": "string", "enum": ["overworld", "nether", "end"], "description": "Dimension, Standard: overworld" })
}

fn tools() -> Value {
    json!([
        {
            "name": "list_worlds",
            "description": "Listet alle verfügbaren Minecraft-Welten (Einzelspieler-Welten aller Launcher-Profile und eingetragene Server-Welten) mit Version, Spielmodus und Spawn.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "world_info",
            "description": "Details zu einer Welt: Version, Spielmodus, Spawn, Tageszeit, erkundete Bereiche je Dimension und letzte Spielerpositionen.",
            "inputSchema": { "type": "object", "properties": { "world": world_param() }, "required": ["world"] },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "area_overview",
            "description": "Geländeübersicht eines Gebiets (max. 512×512): Höhen, Flachheit, Wasser, häufigste Oberflächenblöcke, Biome, Hinweise auf vorhandene Bauten sowie eine Material- und eine Höhenkarte als Text. Entweder Mittelpunkt (x, z) + radius oder Ecken x1,z1,x2,z2 angeben.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "world": world_param(),
                    "dimension": dim_param(),
                    "x": { "type": "integer", "description": "Mittelpunkt x" },
                    "z": { "type": "integer", "description": "Mittelpunkt z" },
                    "radius": { "type": "integer", "description": "Radius in Blöcken (Standard 64, max. 256)" },
                    "x1": { "type": "integer" }, "z1": { "type": "integer" },
                    "x2": { "type": "integer" }, "z2": { "type": "integer" }
                },
                "required": ["world"]
            },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "get_blocks",
            "description": "Exakter Block-Ausschnitt (max. 110.592 Blöcke, z. B. 48×48×48), Schicht für Schicht als Zeichenraster mit Legende. Ideal, um bestehende Bauten oder das Gelände an einem Bauplatz genau zu sehen.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "world": world_param(),
                    "dimension": dim_param(),
                    "x1": { "type": "integer" }, "y1": { "type": "integer" }, "z1": { "type": "integer" },
                    "x2": { "type": "integer" }, "y2": { "type": "integer" }, "z2": { "type": "integer" }
                },
                "required": ["world", "x1", "y1", "z1", "x2", "y2", "z2"]
            },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "find_blocks",
            "description": "Sucht Blöcke nach (Teil-)Namen im Umkreis (max. 128) um einen Punkt, z. B. „chest“, „bed“, „beacon“, „spawner“. Gibt Koordinaten zurück, nächste zuerst.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "world": world_param(),
                    "dimension": dim_param(),
                    "block": { "type": "string", "description": "Blockname oder Teil davon" },
                    "x": { "type": "integer" }, "z": { "type": "integer" },
                    "radius": { "type": "integer", "description": "Standard 48, max. 128" }
                },
                "required": ["world", "block", "x", "z"]
            },
            "annotations": { "readOnlyHint": true }
        }
    ])
}

fn int(args: &Value, key: &str) -> Option<i32> {
    args.get(key).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f.floor() as i64))).map(|v| v as i32)
}

fn req_int(args: &Value, key: &str) -> Result<i32> {
    int(args, key).ok_or_else(|| Error::msg(format!("Parameter „{key}“ fehlt")))
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| Error::msg(format!("Parameter „{key}“ fehlt")))
}

fn dimension(args: &Value) -> Result<Dimension> {
    Dimension::parse(args.get("dimension").and_then(Value::as_str).unwrap_or("overworld"))
}

fn call_tool(paths: &Paths, name: &str, args: &Value) -> Result<String> {
    match name {
        "list_worlds" => {
            let worlds = world::list_all(paths);
            if worlds.is_empty() {
                return Ok("Keine Welten gefunden. Spiele eine Einzelspieler-Welt oder trage im Launcher unter „Welten“ einen Server-Welt-Ordner ein.".into());
            }
            let mut s = String::new();
            for w in worlds {
                s += &format!(
                    "- {} [ID: {}] – {}{}{}{}{}\n",
                    w.name,
                    w.id,
                    if w.source == "server" { "Server" } else { "Einzelspieler" },
                    w.profile.map(|p| format!(", Profil {p}")).unwrap_or_default(),
                    w.version.map(|v| format!(", Version {v}")).unwrap_or_default(),
                    w.game_mode.map(|g| format!(", {g}")).unwrap_or_default(),
                    w.spawn.map(|[x, y, z]| format!(", Spawn {x} {y} {z}")).unwrap_or_default(),
                );
            }
            Ok(s)
        }
        "world_info" => analysis::world_info(&world::find(paths, str_arg(args, "world")?)?),
        "area_overview" => {
            let w = world::find(paths, str_arg(args, "world")?)?;
            let (x1, z1, x2, z2) = match (int(args, "x1"), int(args, "z1"), int(args, "x2"), int(args, "z2")) {
                (Some(a), Some(b), Some(c), Some(d)) => (a, b, c, d),
                _ => {
                    let (x, z) = match (int(args, "x"), int(args, "z")) {
                        (Some(x), Some(z)) => (x, z),
                        _ => {
                            let s = world::read_level(Path::new(&w.path)).and_then(|l| l.spawn).ok_or_else(|| {
                                Error::msg("Bitte x und z (Mittelpunkt) oder x1,z1,x2,z2 angeben.")
                            })?;
                            (s[0] as i32, s[2] as i32)
                        }
                    };
                    let r = int(args, "radius").unwrap_or(64).clamp(4, analysis::MAX_AREA / 2);
                    (x - r, z - r, x + r - 1, z + r - 1)
                }
            };
            analysis::area_summary(Path::new(&w.path), dimension(args)?, x1, z1, x2, z2)
        }
        "get_blocks" => {
            let w = world::find(paths, str_arg(args, "world")?)?;
            analysis::blocks(
                Path::new(&w.path),
                dimension(args)?,
                req_int(args, "x1")?,
                req_int(args, "y1")?,
                req_int(args, "z1")?,
                req_int(args, "x2")?,
                req_int(args, "y2")?,
                req_int(args, "z2")?,
            )
        }
        "find_blocks" => {
            let w = world::find(paths, str_arg(args, "world")?)?;
            analysis::find_blocks(
                Path::new(&w.path),
                dimension(args)?,
                str_arg(args, "block")?,
                req_int(args, "x")?,
                req_int(args, "z")?,
                int(args, "radius").unwrap_or(48),
            )
        }
        other => Err(Error::msg(format!("Unbekanntes Werkzeug: {other}"))),
    }
}

/// Verarbeitet eine JSON-RPC-Nachricht; `None` bei Benachrichtigungen.
pub fn handle(paths: &Paths, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
            let version = if SUPPORTED_PROTOCOLS.contains(&requested) { requested } else { SUPPORTED_PROTOCOLS[0] };
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": SERVER_NAME, "title": "A Better Lunar Client – Welten", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match call_tool(paths, name, &args) {
                Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e.to_string() }], "isError": true }),
            }
        }
        _ => {
            return Some(json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": format!("Methode nicht unterstützt: {method}") }
            }))
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

pub fn run() {
    let paths = Paths::new(crate::paths::default_root());
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&paths, &msg),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } })),
        };
        if let Some(reply) = reply {
            if writeln!(stdout, "{reply}").and_then(|_| stdout.flush()).is_err() {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Einrichtung in der Claude-Desktop-App
// ---------------------------------------------------------------------------

pub const CONFIG_KEY: &str = "minecraft-welten";

pub fn claude_config_path() -> Option<PathBuf> {
    // Windows: %APPDATA%\Claude, macOS: ~/Library/Application Support/Claude
    dirs::config_dir().map(|d| d.join("Claude").join("claude_desktop_config.json"))
}

pub fn server_entry() -> Result<Value> {
    let exe = std::env::current_exe()?;
    Ok(json!({ "command": exe.to_string_lossy(), "args": ["--mcp"] }))
}

/// Trägt den Server in die Claude-Desktop-Konfiguration ein (mit Sicherungskopie).
pub fn install_into_claude() -> Result<PathBuf> {
    let path = claude_config_path().ok_or_else(|| Error::msg("Konfigurationsordner nicht gefunden"))?;
    let mut config: Value = match std::fs::read(&path) {
        Ok(bytes) => {
            std::fs::write(path.with_extension("json.bak"), &bytes)?;
            serde_json::from_slice(&bytes)
                .map_err(|_| Error::msg("Die Claude-Konfiguration ist kein gültiges JSON. Bitte manuell prüfen."))?
        }
        Err(_) => json!({}),
    };
    let obj = config.as_object_mut().ok_or_else(|| Error::msg("Unerwartetes Format der Claude-Konfiguration"))?;
    let servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
    let servers = servers.as_object_mut().ok_or_else(|| Error::msg("„mcpServers“ hat ein unerwartetes Format"))?;
    servers.insert(CONFIG_KEY.into(), server_entry()?);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&config)?)?;
    Ok(path)
}

pub fn is_installed_in_claude() -> bool {
    claude_config_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_some_and(|c| c.get("mcpServers").and_then(|s| s.get(CONFIG_KEY)).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_and_list() {
        let paths = Paths::new(std::env::temp_dir().join("ablc-mcp-test"));
        let init = handle(&paths, &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}})).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert!(handle(&paths, &json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
        let list = handle(&paths, &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 5);
        let err = handle(&paths, &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"world_info","arguments":{"world":"gibtsnicht"}}})).unwrap();
        assert_eq!(err["result"]["isError"], true);
    }
}
