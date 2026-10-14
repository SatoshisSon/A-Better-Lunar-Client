import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Loader = "vanilla" | "fabric" | "legacy_fabric";
export type ContentKind = "mod" | "shader" | "resource_pack";

export interface Settings {
  ms_client_id: string;
  discord_enabled: boolean;
  discord_app_id: string;
  default_memory_mb: number;
  optimized_jvm_flags: boolean;
  java_path: string;
  hide_launcher_on_launch: boolean;
  show_snapshots: boolean;
  concurrent_downloads: number;
}

export interface SystemInfo {
  total_memory_mb: number;
  data_dir: string;
  version: string;
}

export interface Profile {
  id: string;
  name: string;
  game_version: string;
  loader: Loader;
  loader_version: string | null;
  memory_mb: number | null;
  jvm_args: string;
  width: number | null;
  height: number | null;
  created_at: number;
  last_played: number | null;
}

export interface NewProfile {
  name: string;
  game_version: string;
  loader: Loader;
  loader_version?: string | null;
  memory_mb?: number | null;
}

export interface VersionInfo {
  id: string;
  kind: string;
  release_time: string;
}

export interface LoaderVersion {
  version: string;
  stable: boolean;
}

export interface AccountInfo {
  uuid: string;
  name: string;
  kind: "microsoft" | "offline";
  active: boolean;
}

export interface DeviceCode {
  user_code: string;
  device_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
  message: string;
}

export interface Preset {
  id: string;
  name: string;
  description: string;
  projects: string[];
}

export interface ContentItem {
  filename: string;
  enabled: boolean;
  title: string;
  version: string | null;
  project_id: string | null;
  icon_url: string | null;
  dependency: boolean;
}

export interface SearchHit {
  project_id: string;
  slug: string;
  title: string;
  description: string;
  icon_url: string | null;
  downloads: number;
  author: string;
}

export interface SearchResult {
  hits: SearchHit[];
  total_hits: number;
}

export interface PresetReport {
  installed: string[];
  unavailable: string[];
}

export interface ProgressEvent {
  task: string;
  stage: string;
  done: number;
  total: number;
}

export interface WorldEntry {
  id: string;
  name: string;
  source: "singleplayer" | "server";
  profile: string | null;
  path: string;
  version: string | null;
  last_played: number | null;
  game_mode: string | null;
  spawn: [number, number, number] | null;
  available: boolean;
}

export interface ClaudeStatus {
  installed: boolean;
  config_path: string | null;
  snippet: string;
}

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  systemInfo: () => invoke<SystemInfo>("system_info"),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  openFolder: (profileId?: string, sub?: string) =>
    invoke<void>("open_folder", { profileId: profileId ?? null, sub: sub ?? null }),

  listVersions: (loader: Loader, snapshots: boolean) =>
    invoke<VersionInfo[]>("list_versions", { loader, snapshots }),
  listLoaderVersions: (loader: Loader, gameVersion: string) =>
    invoke<LoaderVersion[]>("list_loader_versions", { loader, gameVersion }),

  listProfiles: () => invoke<{ selected: string | null; profiles: Profile[] }>("list_profiles"),
  createProfile: (profile: NewProfile) => invoke<Profile>("create_profile", { profile }),
  updateProfile: (profile: Profile) => invoke<void>("update_profile", { profile }),
  selectProfile: (id: string) => invoke<void>("select_profile", { id }),
  deleteProfile: (id: string, deleteFiles: boolean) =>
    invoke<void>("delete_profile", { id, deleteFiles }),

  listAccounts: () => invoke<AccountInfo[]>("list_accounts"),
  addOfflineAccount: (name: string) => invoke<void>("add_offline_account", { name }),
  setActiveAccount: (uuid: string) => invoke<void>("set_active_account", { uuid }),
  removeAccount: (uuid: string) => invoke<void>("remove_account", { uuid }),
  msLoginStart: () => invoke<DeviceCode>("ms_login_start"),
  msLoginFinish: (code: DeviceCode) => invoke<AccountInfo>("ms_login_finish", { code }),
  msLoginCancel: () => invoke<void>("ms_login_cancel"),

  launch: (profileId: string) => invoke<void>("launch_profile", { profileId }),
  kill: (profileId: string) => invoke<void>("kill_game", { profileId }),
  runningGames: () => invoke<string[]>("running_games"),

  getPresets: () => invoke<Preset[]>("get_presets"),
  listContent: (profileId: string, kind: ContentKind) =>
    invoke<ContentItem[]>("list_content", { profileId, kind }),
  searchContent: (profileId: string, kind: ContentKind, query: string, offset: number) =>
    invoke<SearchResult>("search_content", { profileId, kind, query, offset }),
  installContent: (profileId: string, projectId: string, kind: ContentKind) =>
    invoke<void>("install_content", { profileId, projectId, kind }),
  toggleContent: (profileId: string, kind: ContentKind, filename: string, enabled: boolean) =>
    invoke<void>("toggle_content", { profileId, kind, filename, enabled }),
  removeContent: (profileId: string, kind: ContentKind, filename: string) =>
    invoke<void>("remove_content", { profileId, kind, filename }),
  installPresets: (profileId: string, presets: string[]) =>
    invoke<PresetReport>("install_presets", { profileId, presets }),
  updateContent: (profileId: string) => invoke<PresetReport>("update_content", { profileId }),

  listWorlds: () => invoke<WorldEntry[]>("list_worlds"),
  addServerWorld: (name: string, path: string) => invoke<void>("add_server_world", { name, path }),
  removeServerWorld: (id: string) => invoke<void>("remove_server_world", { id }),
  openWorldFolder: (id: string) => invoke<void>("open_world_folder", { id }),
  worldPreview: (id: string) => invoke<string>("world_preview", { id }),
  claudeStatus: () => invoke<ClaudeStatus>("claude_status"),
  claudeInstall: () => invoke<string>("claude_install"),
};

export const events = {
  progress: (cb: (e: ProgressEvent) => void): Promise<UnlistenFn> =>
    listen<ProgressEvent>("progress", (e) => cb(e.payload)),
  gameLog: (cb: (e: { profile_id: string; line: string }) => void): Promise<UnlistenFn> =>
    listen<{ profile_id: string; line: string }>("game-log", (e) => cb(e.payload)),
  gameExit: (cb: (e: { profile_id: string; code: number | null }) => void): Promise<UnlistenFn> =>
    listen<{ profile_id: string; code: number | null }>("game-exit", (e) => cb(e.payload)),
};

export const LOADER_LABELS: Record<Loader, string> = {
  vanilla: "Vanilla",
  fabric: "Fabric",
  legacy_fabric: "Legacy Fabric",
};
