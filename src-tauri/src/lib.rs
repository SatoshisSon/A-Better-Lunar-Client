mod accounts;
mod auth;
mod discord;
mod error;
mod mcp;
mod minecraft;
mod mods;
mod net;
mod paths;
mod profiles;
mod progress;
mod settings;
mod world;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{oneshot, Mutex};

use accounts::{AccountInfo, AccountKind, AccountStore};
use error::{Error, Result};
use minecraft::launch::{Auth, LaunchOptions};
use minecraft::loader::{self, Loader, LoaderVersion};
use minecraft::{install, java, launch};
use mods::{ContentItem, ContentKind, Preset, PresetReport, SearchResult};
use paths::Paths;
use profiles::{NewProfile, Profile, ProfileStore};
use progress::Progress;
use settings::Settings;

pub struct AppState {
    paths: Paths,
    http: reqwest::Client,
    discord: std::sync::Mutex<discord::Discord>,
    /// Laufende Spiele: Profil-ID → Signal zum Beenden.
    games: Mutex<HashMap<String, oneshot::Sender<()>>>,
    login_cancelled: AtomicBool,
}

// ---------------------------------------------------------------------------
// Einstellungen & System
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    Settings::load(&state.paths)
}

#[tauri::command]
fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<()> {
    settings.save(&state.paths)
}

#[derive(Serialize)]
struct SystemInfo {
    total_memory_mb: u64,
    data_dir: String,
    version: &'static str,
}

#[tauri::command]
fn system_info(state: State<'_, AppState>) -> SystemInfo {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    SystemInfo {
        total_memory_mb: sys.total_memory() / 1024 / 1024,
        data_dir: state.paths.root.to_string_lossy().into_owned(),
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<()> {
    if !url.starts_with("https://") {
        return Err(Error::msg("Nur https-Links erlaubt"));
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| Error::msg(e.to_string()))
}

#[tauri::command]
fn open_folder(app: AppHandle, state: State<'_, AppState>, profile_id: Option<String>, sub: Option<String>) -> Result<()> {
    let mut dir = match profile_id {
        Some(id) => state.paths.instance(&id),
        None => state.paths.root.clone(),
    };
    if let Some(sub) = sub.filter(|s| ["mods", "shaderpacks", "resourcepacks", "saves", "screenshots", "logs"].contains(&s.as_str())) {
        dir = dir.join(sub);
    }
    std::fs::create_dir_all(&dir)?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| Error::msg(e.to_string()))
}

// ---------------------------------------------------------------------------
// Versionen
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct VersionInfo {
    id: String,
    kind: String,
    release_time: String,
}

#[tauri::command]
async fn list_versions(state: State<'_, AppState>, loader: Loader, snapshots: bool) -> Result<Vec<VersionInfo>> {
    let manifest = install::version_manifest(&state.http, &state.paths).await?;
    let supported: Option<std::collections::HashSet<String>> = match loader {
        Loader::Vanilla => None,
        l => Some(loader::supported_game_versions(&state.http, l).await?.into_iter().collect()),
    };
    Ok(manifest
        .versions
        .into_iter()
        .filter(|v| v.kind == "release" || (snapshots && v.kind == "snapshot"))
        .filter(|v| supported.as_ref().is_none_or(|s| s.contains(&v.id)))
        .map(|v| VersionInfo { id: v.id, kind: v.kind, release_time: v.release_time })
        .collect())
}

#[tauri::command]
async fn list_loader_versions(state: State<'_, AppState>, loader: Loader, game_version: String) -> Result<Vec<LoaderVersion>> {
    loader::loader_versions(&state.http, loader, &game_version).await
}

// ---------------------------------------------------------------------------
// Profile
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ProfilesView {
    selected: Option<String>,
    profiles: Vec<Profile>,
}

#[tauri::command]
fn list_profiles(state: State<'_, AppState>) -> ProfilesView {
    let store = ProfileStore::load(&state.paths);
    ProfilesView { selected: store.selected, profiles: store.profiles }
}

#[tauri::command]
fn create_profile(state: State<'_, AppState>, profile: NewProfile) -> Result<Profile> {
    let mut store = ProfileStore::load(&state.paths);
    let p = store.create(&state.paths, profile)?;
    store.save(&state.paths)?;
    Ok(p)
}

#[tauri::command]
fn update_profile(state: State<'_, AppState>, profile: Profile) -> Result<()> {
    let mut store = ProfileStore::load(&state.paths);
    store.update(profile)?;
    store.save(&state.paths)
}

#[tauri::command]
fn select_profile(state: State<'_, AppState>, id: String) -> Result<()> {
    let mut store = ProfileStore::load(&state.paths);
    store.get(&id)?;
    store.selected = Some(id);
    store.save(&state.paths)
}

#[tauri::command]
async fn delete_profile(state: State<'_, AppState>, id: String, delete_files: bool) -> Result<()> {
    if state.games.lock().await.contains_key(&id) {
        return Err(Error::msg("Das Profil läuft gerade"));
    }
    let mut store = ProfileStore::load(&state.paths);
    store.delete(&state.paths, &id, delete_files)?;
    store.save(&state.paths)
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[tauri::command]
fn list_accounts(state: State<'_, AppState>) -> Vec<AccountInfo> {
    AccountStore::load(&state.paths).infos()
}

#[tauri::command]
fn add_offline_account(state: State<'_, AppState>, name: String) -> Result<()> {
    let mut store = AccountStore::load(&state.paths);
    // Wie bei Prism Launcher: Offline-Accounts nur, wenn ein gekaufter
    // Microsoft-Account vorhanden ist.
    if !store.accounts.iter().any(|a| a.kind == AccountKind::Microsoft) {
        return Err(Error::msg(
            "Offline-Accounts sind erst möglich, nachdem ein Microsoft-Account mit Minecraft hinzugefügt wurde.",
        ));
    }
    store.add_offline(name.trim())?;
    store.save(&state.paths)
}

#[tauri::command]
fn set_active_account(state: State<'_, AppState>, uuid: String) -> Result<()> {
    let mut store = AccountStore::load(&state.paths);
    store.set_active(&uuid)?;
    store.save(&state.paths)
}

#[tauri::command]
fn remove_account(state: State<'_, AppState>, uuid: String) -> Result<()> {
    let mut store = AccountStore::load(&state.paths);
    store.remove(&uuid);
    store.save(&state.paths)
}

#[tauri::command]
async fn ms_login_start(state: State<'_, AppState>) -> Result<auth::DeviceCode> {
    state.login_cancelled.store(false, Ordering::Relaxed);
    let settings = Settings::load(&state.paths);
    auth::start_device_code(&state.http, settings.ms_client_id.trim()).await
}

#[tauri::command]
async fn ms_login_finish(state: State<'_, AppState>, code: auth::DeviceCode) -> Result<AccountInfo> {
    let settings = Settings::load(&state.paths);
    let session = auth::poll_device_code(&state.http, settings.ms_client_id.trim(), &code, || {
        state.login_cancelled.load(Ordering::Relaxed)
    })
    .await?;
    let uuid = session.uuid.clone();
    let mut store = AccountStore::load(&state.paths);
    store.add_microsoft(session);
    store.save(&state.paths)?;
    store
        .infos()
        .into_iter()
        .find(|a| a.uuid == uuid)
        .ok_or_else(|| Error::msg("Account konnte nicht gespeichert werden"))
}

#[tauri::command]
fn ms_login_cancel(state: State<'_, AppState>) {
    state.login_cancelled.store(true, Ordering::Relaxed);
}

/// Aktiven Account holen und den Microsoft-Token bei Bedarf erneuern.
async fn active_auth(state: &AppState, settings: &Settings) -> Result<Auth> {
    let mut store = AccountStore::load(&state.paths);
    let account = store
        .active_account()
        .cloned()
        .ok_or_else(|| Error::msg("Bitte füge zuerst unter „Accounts“ einen Account hinzu."))?;

    let account = if account.kind == AccountKind::Microsoft && account.expires_at < auth::now() + 300 {
        let session = auth::refresh(&state.http, settings.ms_client_id.trim(), &account.refresh_token).await?;
        store.add_microsoft(session);
        store.save(&state.paths)?;
        store.active_account().cloned().ok_or_else(|| Error::msg("Account fehlt"))?
    } else {
        account
    };

    Ok(Auth {
        name: account.name,
        uuid: account.uuid,
        access_token: account.access_token,
        user_type: if account.kind == AccountKind::Microsoft { "msa" } else { "legacy" }.into(),
        xuid: "0".into(),
    })
}

// ---------------------------------------------------------------------------
// Starten
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
struct GameLog<'a> {
    profile_id: &'a str,
    line: &'a str,
}

#[derive(Clone, Serialize)]
struct GameExit {
    profile_id: String,
    code: Option<i32>,
}

fn loader_label(l: Loader) -> &'static str {
    match l {
        Loader::Vanilla => "Vanilla",
        Loader::Fabric => "Fabric",
        Loader::LegacyFabric => "Legacy Fabric",
    }
}

#[tauri::command]
async fn launch_profile(app: AppHandle, state: State<'_, AppState>, profile_id: String) -> Result<()> {
    if state.games.lock().await.contains_key(&profile_id) {
        return Err(Error::msg("Dieses Profil läuft bereits"));
    }
    let settings = Settings::load(&state.paths);
    let profile = ProfileStore::load(&state.paths).get(&profile_id)?.clone();
    let progress = Progress::new(&app, "launch");
    let concurrency = settings.concurrent_downloads.clamp(1, 64) as usize;

    progress.stage("Anmeldung wird geprüft");
    let auth = active_auth(&state, &settings).await?;

    progress.stage("Version wird geladen");
    let version = install::resolve_version(
        &state.http,
        &state.paths,
        &profile.game_version,
        profile.loader,
        profile.loader_version.as_deref(),
    )
    .await?;

    let game_dir = state.paths.instance(&profile.id);
    std::fs::create_dir_all(&game_dir)?;
    let installed = install::install(
        &state.http,
        &state.paths,
        &profile.game_version,
        &version,
        &game_dir,
        concurrency,
        &progress,
    )
    .await?;

    let java_path = if settings.java_path.trim().is_empty() {
        java::ensure_java(&state.http, &state.paths, &version, concurrency, &progress).await?
    } else {
        PathBuf::from(settings.java_path.trim())
    };

    let resolution = match (profile.width, profile.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => Some((w, h)),
        _ => None,
    };
    let args = launch::build_args(&LaunchOptions {
        version: &version,
        installed: &installed,
        game_dir: &game_dir,
        auth: &auth,
        memory_mb: profile.memory_mb.unwrap_or(settings.default_memory_mb).max(512),
        optimized_flags: settings.optimized_jvm_flags,
        extra_jvm_args: &profile.jvm_args,
        resolution,
    });

    progress.stage("Minecraft wird gestartet");
    let mut cmd = tokio::process::Command::new(&java_path);
    cmd.args(&args)
        .current_dir(&game_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::msg(format!("Java konnte nicht gestartet werden ({}): {e}", java_path.display())))?;

    // Log-Ausgabe an die Oberfläche weiterreichen (Token wird ausgeblendet).
    let token = auth.access_token.clone();
    for stream in [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let app = app.clone();
        let id = profile_id.clone();
        let token = token.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = if token.len() > 8 { line.replace(&token, "<token>") } else { line };
                let _ = app.emit("game-log", GameLog { profile_id: &id, line: &line });
            }
        });
    }

    let (kill_tx, kill_rx) = oneshot::channel();
    state.games.lock().await.insert(profile_id.clone(), kill_tx);

    {
        let mut store = ProfileStore::load(&state.paths);
        if let Ok(p) = store.get_mut(&profile_id) {
            p.last_played = Some(auth::now());
        }
        store.selected = Some(profile_id.clone());
        let _ = store.save(&state.paths);
    }

    if settings.discord_enabled {
        state.discord.lock().unwrap().set_playing(
            &settings.discord_app_id,
            &format!("Minecraft {}", profile.game_version),
            loader_label(profile.loader),
            auth::now() as i64,
        );
    }
    if settings.hide_launcher_on_launch {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.minimize();
        }
    }
    progress.update("Läuft", 1, 1);

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let status = tokio::select! {
            s = child.wait() => s,
            _ = kill_rx => {
                let _ = child.kill().await;
                child.wait().await
            }
        };
        let state = app2.state::<AppState>();
        let mut games = state.games.lock().await;
        games.remove(&profile_id);
        if games.is_empty() {
            state.discord.lock().unwrap().clear();
        }
        drop(games);
        if let Some(w) = app2.get_webview_window("main") {
            let _ = w.unminimize();
        }
        let _ = app2.emit("game-exit", GameExit { profile_id, code: status.ok().and_then(|s| s.code()) });
    });
    Ok(())
}

#[tauri::command]
async fn kill_game(state: State<'_, AppState>, profile_id: String) -> Result<()> {
    if let Some(tx) = state.games.lock().await.remove(&profile_id) {
        let _ = tx.send(());
    }
    Ok(())
}

#[tauri::command]
async fn running_games(state: State<'_, AppState>) -> Result<Vec<String>> {
    Ok(state.games.lock().await.keys().cloned().collect())
}

// ---------------------------------------------------------------------------
// Mods, Shader, Ressourcenpakete
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_presets() -> &'static [Preset] {
    mods::PRESETS
}

#[tauri::command]
fn list_content(state: State<'_, AppState>, profile_id: String, kind: ContentKind) -> Result<Vec<ContentItem>> {
    mods::list(&state.paths, &profile_id, kind)
}

#[tauri::command]
async fn search_content(
    state: State<'_, AppState>,
    profile_id: String,
    kind: ContentKind,
    query: String,
    offset: u32,
) -> Result<SearchResult> {
    let profile = ProfileStore::load(&state.paths).get(&profile_id)?.clone();
    mods::search(&state.http, &profile, kind, &query, offset).await
}

#[tauri::command]
async fn install_content(
    state: State<'_, AppState>,
    profile_id: String,
    project_id: String,
    kind: ContentKind,
) -> Result<()> {
    let profile = ProfileStore::load(&state.paths).get(&profile_id)?.clone();
    if !mods::install(&state.http, &state.paths, &profile, &project_id, kind).await? {
        return Err(Error::msg(format!(
            "Keine passende Version für Minecraft {} verfügbar",
            profile.game_version
        )));
    }
    Ok(())
}

#[tauri::command]
fn toggle_content(
    state: State<'_, AppState>,
    profile_id: String,
    kind: ContentKind,
    filename: String,
    enabled: bool,
) -> Result<()> {
    mods::set_enabled(&state.paths, &profile_id, kind, &filename, enabled)
}

#[tauri::command]
fn remove_content(state: State<'_, AppState>, profile_id: String, kind: ContentKind, filename: String) -> Result<()> {
    mods::remove(&state.paths, &profile_id, kind, &filename)
}

#[tauri::command]
async fn install_presets(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    presets: Vec<String>,
) -> Result<PresetReport> {
    let profile = ProfileStore::load(&state.paths).get(&profile_id)?.clone();
    let progress = Progress::new(&app, "mods");
    mods::install_presets(&state.http, &state.paths, &profile, &presets, &progress).await
}

#[tauri::command]
async fn update_content(app: AppHandle, state: State<'_, AppState>, profile_id: String) -> Result<PresetReport> {
    let profile = ProfileStore::load(&state.paths).get(&profile_id)?.clone();
    let progress = Progress::new(&app, "mods");
    mods::update_all(&state.http, &state.paths, &profile, &progress).await
}

// ---------------------------------------------------------------------------
// Welten & Claude
// ---------------------------------------------------------------------------

#[tauri::command]
async fn list_worlds(state: State<'_, AppState>) -> Result<Vec<world::WorldEntry>> {
    let paths = state.paths.clone();
    tokio::task::spawn_blocking(move || world::list_all(&paths)).await.map_err(|e| Error::msg(e.to_string()))
}

#[tauri::command]
fn add_server_world(state: State<'_, AppState>, name: String, path: String) -> Result<world::ServerWorld> {
    world::add_server(&state.paths, &name, &path)
}

#[tauri::command]
fn remove_server_world(state: State<'_, AppState>, id: String) -> Result<()> {
    world::remove_server(&state.paths, &id)
}

#[tauri::command]
fn open_world_folder(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<()> {
    let w = world::find(&state.paths, &id)?;
    app.opener().open_path(w.path, None::<&str>).map_err(|e| Error::msg(e.to_string()))
}

/// Vorschau, wie Claude die Welt sieht (Geländeübersicht um den Spawn bzw. Spieler).
#[tauri::command]
async fn world_preview(state: State<'_, AppState>, id: String) -> Result<String> {
    let paths = state.paths.clone();
    tokio::task::spawn_blocking(move || -> Result<String> {
        let w = world::find(&paths, &id)?;
        let path = std::path::Path::new(&w.path);
        let level = world::read_level(path);
        let center = world::players(path, level.as_ref())
            .into_iter()
            .find(|p| p.dimension == "overworld")
            .map(|p| (p.pos[0] as i32, p.pos[2] as i32))
            .or_else(|| level.as_ref().and_then(|l| l.spawn).map(|s| (s[0] as i32, s[2] as i32)))
            .unwrap_or((0, 0));
        let info = world::analysis::world_info(&w)?;
        let area = world::analysis::area_summary(
            path,
            world::anvil::Dimension::Overworld,
            center.0 - 48,
            center.1 - 48,
            center.0 + 47,
            center.1 + 47,
        )?;
        Ok(format!("{info}\n{area}"))
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))?
}

#[derive(Serialize)]
struct ClaudeStatus {
    installed: bool,
    config_path: Option<String>,
    snippet: String,
}

#[tauri::command]
fn claude_status() -> Result<ClaudeStatus> {
    let snippet = serde_json::to_string_pretty(&serde_json::json!({
        "mcpServers": { mcp::CONFIG_KEY: mcp::server_entry()? }
    }))?;
    Ok(ClaudeStatus {
        installed: mcp::is_installed_in_claude(),
        config_path: mcp::claude_config_path().map(|p| p.to_string_lossy().into_owned()),
        snippet,
    })
}

#[tauri::command]
fn claude_install() -> Result<String> {
    Ok(mcp::install_into_claude()?.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------

pub fn run_mcp() {
    mcp::run();
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Gleicher Ordner wie Tauris app_data_dir; auch der MCP-Server nutzt ihn.
            let root = paths::default_root();
            std::fs::create_dir_all(&root)?;
            app.manage(AppState {
                paths: Paths::new(root),
                http: net::client(),
                discord: Default::default(),
                games: Default::default(),
                login_cancelled: AtomicBool::new(false),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            system_info,
            open_url,
            open_folder,
            list_versions,
            list_loader_versions,
            list_profiles,
            create_profile,
            update_profile,
            select_profile,
            delete_profile,
            list_accounts,
            add_offline_account,
            set_active_account,
            remove_account,
            ms_login_start,
            ms_login_finish,
            ms_login_cancel,
            launch_profile,
            kill_game,
            running_games,
            get_presets,
            list_content,
            search_content,
            install_content,
            toggle_content,
            remove_content,
            install_presets,
            update_content,
            list_worlds,
            add_server_world,
            remove_server_world,
            open_world_folder,
            world_preview,
            claude_status,
            claude_install,
        ])
        .run(tauri::generate_context!())
        .expect("Fehler beim Starten von A Better Lunar Client");
}
