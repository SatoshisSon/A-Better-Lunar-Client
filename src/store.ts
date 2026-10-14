import { api, events, type AccountInfo, type Profile, type ProgressEvent, type Settings, type SystemInfo } from "./api";

type Topic = "profiles" | "accounts" | "running" | "progress" | "log";

export const store = {
  profiles: [] as Profile[],
  selected: null as string | null,
  accounts: [] as AccountInfo[],
  running: new Set<string>(),
  launching: null as string | null,
  progress: null as ProgressEvent | null,
  logs: new Map<string, string[]>(),
  settings: null as Settings | null,
  system: null as SystemInfo | null,
};

const listeners = new Map<Topic, Set<() => void>>();

export function on(topic: Topic, fn: () => void): () => void {
  if (!listeners.has(topic)) listeners.set(topic, new Set());
  listeners.get(topic)!.add(fn);
  return () => listeners.get(topic)!.delete(fn);
}

export function emit(topic: Topic) {
  listeners.get(topic)?.forEach((fn) => fn());
}

export function selectedProfile(): Profile | undefined {
  return store.profiles.find((p) => p.id === store.selected) ?? store.profiles[0];
}

export function activeAccount(): AccountInfo | undefined {
  return store.accounts.find((a) => a.active);
}

export async function refreshProfiles() {
  const view = await api.listProfiles();
  store.profiles = view.profiles;
  store.selected = view.selected;
  emit("profiles");
}

export async function refreshAccounts() {
  store.accounts = await api.listAccounts();
  emit("accounts");
}

export async function refreshSettings() {
  store.settings = await api.getSettings();
}

const MAX_LOG_LINES = 2000;

export async function init() {
  const [system, running] = await Promise.all([api.systemInfo(), api.runningGames(), refreshSettings(), refreshProfiles(), refreshAccounts()]);
  store.system = system;
  store.running = new Set(running);

  await events.progress((e) => {
    store.progress = e;
    emit("progress");
  });
  await events.gameLog(({ profile_id, line }) => {
    const lines = store.logs.get(profile_id) ?? [];
    lines.push(line);
    if (lines.length > MAX_LOG_LINES) lines.splice(0, lines.length - MAX_LOG_LINES);
    store.logs.set(profile_id, lines);
    emit("log");
  });
  await events.gameExit(({ profile_id }) => {
    store.running.delete(profile_id);
    emit("running");
  });
}
