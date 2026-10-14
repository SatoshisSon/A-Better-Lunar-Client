import { open } from "@tauri-apps/plugin-dialog";
import { api, type WorldEntry } from "../api";
import { attempt, clear, confirmDialog, errorText, field, h, modal, toast } from "../ui";

function lastPlayed(ms: number | null) {
  if (!ms) return "";
  return new Date(ms).toLocaleDateString("de-DE", { day: "2-digit", month: "2-digit", year: "numeric" });
}

function addServerDialog(onDone: () => void) {
  const name = h("input", { type: "text", placeholder: "z. B. Mein SMP" }) as HTMLInputElement;
  const path = h("input", { type: "text", placeholder: "Pfad zum Welt-Ordner (enthält level.dat)" }) as HTMLInputElement;
  const pick = h("button", { class: "btn ghost small", type: "button" }, "Ordner wählen …");
  pick.onclick = async () => {
    const dir = await open({ directory: true, title: "Welt-Ordner des Servers wählen" }).catch(() => null);
    if (typeof dir === "string") path.value = dir;
  };
  const add = h("button", { class: "btn primary" }, "Hinzufügen");
  const m = modal(
    "Server-Welt hinzufügen",
    h("div", { class: "form" },
      h("p", { class: "muted" },
        "Wähle den Welt-Ordner deines Servers (meist „world“, mit level.dat und region). Läuft der Server woanders, lade den Ordner vorher z. B. per FTP herunter. Tipp: Vorher im Server „/save-all“ ausführen, damit alles gespeichert ist."),
      field("Name", name),
      field("Welt-Ordner", h("div", { class: "row" }, path, pick)),
    ),
    [add],
  );
  add.onclick = async () => {
    if ((await attempt(() => api.addServerWorld(name.value, path.value))) !== undefined) {
      m.close();
      onDone();
    }
  };
}

async function previewDialog(w: WorldEntry) {
  const pre = h("pre", { class: "log preview" }, "Lese Welt …");
  modal(`So sieht Claude „${w.name}“`, h("div", {}, h("p", { class: "muted" }, "Gelände rund um deine letzte Position bzw. den Spawn:"), pre));
  try {
    pre.textContent = await api.worldPreview(w.id);
  } catch (e) {
    pre.textContent = errorText(e);
  }
}

export function renderWorlds(root: HTMLElement) {
  const list = h("div", { class: "card-list" });
  const claudeBox = h("section", { class: "panel claude-panel" });

  async function loadWorlds() {
    clear(list, h("p", { class: "empty" }, "Suche Welten …"));
    const worlds = (await attempt(() => api.listWorlds())) ?? [];
    clear(
      list,
      worlds.length === 0 ? h("p", { class: "empty" }, "Noch keine Welten. Spiele eine Einzelspieler-Welt oder füge die Welt deines Servers hinzu.") : null,
      worlds.map((w) =>
        h("div", { class: "card" },
          h("div", { class: "world-badge " + w.source }, w.source === "server" ? "Server" : "Welt"),
          h("div", { class: "card-main" },
            h("strong", {}, w.name),
            h("span", { class: "muted" },
              [w.profile ? `Profil ${w.profile}` : "Server-Welt", w.version, w.game_mode, lastPlayed(w.last_played)].filter(Boolean).join(" · "),
              w.available ? "" : " · ⚠ Ordner nicht gefunden",
            ),
          ),
          h("div", { class: "card-actions" },
            h("button", { class: "btn ghost small", onclick: () => previewDialog(w) }, "Vorschau"),
            h("button", { class: "btn ghost small", onclick: () => attempt(() => api.openWorldFolder(w.id)) }, "Ordner"),
            w.source === "server"
              ? h("button", {
                  class: "btn ghost small danger",
                  onclick: async () => {
                    if (await confirmDialog("Entfernen?", `„${w.name}“ aus der Liste entfernen? Die Dateien bleiben erhalten.`, "Entfernen")) {
                      await attempt(() => api.removeServerWorld(w.id));
                      loadWorlds();
                    }
                  },
                }, "Entfernen")
              : null,
          ),
        ),
      ),
    );
  }

  async function loadClaude() {
    const status = await attempt(() => api.claudeStatus());
    if (!status) return;
    const setup = h("button", { class: "btn primary" }, status.installed ? "Erneut einrichten" : "Claude-App automatisch einrichten");
    setup.onclick = async () => {
      const path = await attempt(() => api.claudeInstall());
      if (path) {
        toast("Eingerichtet! Starte die Claude-App neu.", "success");
        loadClaude();
      }
    };
    const snippet = h("pre", { class: "log snippet" }, status.snippet);
    clear(
      claudeBox,
      h("div", { class: "claude-head" },
        h("div", {},
          h("h3", {}, "Mit Claude verbinden"),
          h("p", {}, status.installed
            ? "✓ In der Claude-Desktop-App eingerichtet. Frag Claude z. B.: „Schau dir meine Welt an und schlag mir ein Haus am Wasser in der Nähe meiner Position vor.“"
            : "Claude kann deine Welten dann selbst auslesen: Gelände, Höhen, Biome und vorhandene Bauten, und dir passende Bauideen mit Koordinaten und Materiallisten vorschlagen."),
        ),
        setup,
      ),
      h("details", { class: "manual" },
        h("summary", {}, "Manuell einrichten"),
        h("p", { class: "muted small" }, `In der Claude-App: Einstellungen → Entwickler → Konfiguration bearbeiten${status.config_path ? ` (${status.config_path})` : ""} und Folgendes eintragen:`),
        snippet,
        h("button", { class: "btn ghost small", onclick: async () => { await navigator.clipboard?.writeText(status.snippet).catch(() => undefined); toast("Kopiert"); } }, "Kopieren"),
      ),
      h("p", { class: "muted small" }, "Claude liest nur – es verändert nichts an deinen Welten. Daten werden erst übertragen, wenn du Claude im Chat danach fragst."),
    );
  }

  clear(
    root,
    h("div", { class: "page-head" },
      h("h1", {}, "Welten"),
      h("div", { class: "head-tools" },
        h("button", { class: "btn ghost", onclick: loadWorlds }, "Aktualisieren"),
        h("button", { class: "btn primary", onclick: () => addServerDialog(loadWorlds) }, "+ Server-Welt"),
      ),
    ),
    claudeBox,
    list,
  );
  loadClaude();
  loadWorlds();
}
