import { api, LOADER_LABELS, type Profile } from "../api";
import { activeAccount, emit, on, refreshProfiles, selectedProfile, store } from "../store";
import { attempt, clear, errorText, h, timeAgo, toast } from "../ui";
import type { Navigate } from "../main";

export async function play(profile: Profile) {
  if (store.launching || store.running.has(profile.id)) return;
  store.launching = profile.id;
  store.logs.set(profile.id, []);
  store.progress = null;
  emit("running");
  try {
    await api.launch(profile.id);
    store.running.add(profile.id);
    await refreshProfiles();
  } catch (e) {
    toast(errorText(e), "error");
  } finally {
    store.launching = null;
    store.progress = null;
    emit("running");
    emit("progress");
  }
}

export function renderHome(root: HTMLElement, navigate: Navigate) {
  const hero = h("section", { class: "hero" });
  const recent = h("section", { class: "recent" });
  const logBox = h("pre", { class: "log" });
  const logPanel = h("details", { class: "log-panel" }, h("summary", {}, "Spiel-Log"), logBox);

  function renderHero() {
    const p = selectedProfile();
    const account = activeAccount();
    if (!p) {
      clear(
        hero,
        h("div", { class: "hero-empty" },
          h("h1", {}, "Willkommen!"),
          h("p", {}, "Lege dein erstes Profil an. Performance-Mods wie Sodium sind auf Wunsch gleich mit dabei."),
          h("button", { class: "btn primary big", onclick: () => navigate("profiles", { create: true }) }, "Profil erstellen"),
        ),
      );
      return;
    }
    const running = store.running.has(p.id);
    const launching = store.launching === p.id;
    const prog = launching ? store.progress : null;
    const pct = prog && prog.total > 0 ? Math.round((prog.done / prog.total) * 100) : null;

    const playBtn = running
      ? h("button", { class: "btn danger big", onclick: () => attempt(() => api.kill(p.id)) }, "Beenden")
      : h("button", { class: "btn primary big play", disabled: !!store.launching, onclick: () => play(p) }, launching ? "Startet …" : "Spielen");

    clear(
      hero,
      h("div", { class: "hero-info" },
        h("div", { class: "eyebrow" }, `${LOADER_LABELS[p.loader]} · Minecraft ${p.game_version}`),
        h("h1", {}, p.name),
        h("div", { class: "meta" },
          account ? h("span", { class: "chip" }, `👤 ${account.name}`) : h("button", { class: "chip warn", onclick: () => navigate("accounts") }, "⚠ Kein Account – hier hinzufügen"),
          h("span", { class: "chip" }, running ? "● Läuft" : timeAgo(p.last_played)),
        ),
      ),
      h("div", { class: "hero-actions" },
        playBtn,
        launching
          ? h("div", { class: "progress" },
              h("div", { class: "progress-bar" }, h("div", { class: "progress-fill" + (pct === null ? " indeterminate" : ""), style: pct === null ? "" : `width:${pct}%` })),
              h("div", { class: "progress-text" }, prog ? `${prog.stage}${prog.total > 1 ? ` – ${prog.done}/${prog.total}` : ""}` : "Vorbereitung …"),
            )
          : h("div", { class: "hero-links" },
              h("button", { class: "btn ghost small", onclick: () => navigate("mods") }, "Mods"),
              h("button", { class: "btn ghost small", onclick: () => navigate("profiles", { edit: p.id }) }, "Bearbeiten"),
              h("button", { class: "btn ghost small", onclick: () => attempt(() => api.openFolder(p.id)) }, "Ordner"),
            ),
      ),
    );
  }

  function renderRecent() {
    const others = [...store.profiles].sort((a, b) => (b.last_played ?? b.created_at) - (a.last_played ?? a.created_at));
    if (others.length < 2) {
      clear(recent);
      return;
    }
    clear(
      recent,
      h("h3", {}, "Profile"),
      h("div", { class: "profile-grid" },
        others.map((p) =>
          h("button", {
              class: "profile-tile" + (p.id === selectedProfile()?.id ? " active" : ""),
              onclick: async () => {
                await attempt(() => api.selectProfile(p.id));
                store.selected = p.id;
                emit("profiles");
              },
            },
            h("strong", {}, p.name),
            h("span", {}, `${LOADER_LABELS[p.loader]} ${p.game_version}`),
            store.running.has(p.id) ? h("span", { class: "dot-running" }, "● läuft") : null,
          ),
        ),
      ),
    );
  }

  function renderLog() {
    const p = selectedProfile();
    const lines = p ? store.logs.get(p.id) ?? [] : [];
    const atBottom = logBox.scrollTop + logBox.clientHeight >= logBox.scrollHeight - 20;
    logBox.textContent = lines.length ? lines.join("\n") : "Noch keine Ausgabe. Starte das Spiel, um das Log zu sehen.";
    if (atBottom) logBox.scrollTop = logBox.scrollHeight;
  }

  const all = () => {
    renderHero();
    renderRecent();
    renderLog();
  };
  all();
  clear(root, hero, recent, logPanel);

  const offs = [on("profiles", all), on("accounts", renderHero), on("running", all), on("progress", renderHero), on("log", renderLog)];
  return () => offs.forEach((f) => f());
}
