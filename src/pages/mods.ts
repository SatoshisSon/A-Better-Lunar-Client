import { api, LOADER_LABELS, type ContentItem, type ContentKind, type Profile } from "../api";
import { emit, on, selectedProfile, store } from "../store";
import { attempt, clear, confirmDialog, errorText, formatNumber, h, toast, toggle } from "../ui";
import { restart } from "./home";
import type { Navigate } from "../main";

const KINDS: { kind: ContentKind; label: string; folder: string }[] = [
  { kind: "mod", label: "Mods", folder: "mods" },
  { kind: "shader", label: "Shader", folder: "shaderpacks" },
  { kind: "resource_pack", label: "Ressourcenpakete", folder: "resourcepacks" },
];

function icon(url: string | null) {
  return url ? h("img", { class: "item-icon", src: url, alt: "", loading: "lazy" }) : h("div", { class: "item-icon placeholder" }, "◆");
}

export function renderMods(root: HTMLElement, navigate: Navigate) {
  let kind: ContentKind = "mod";
  let installed: ContentItem[] = [];
  let query = "";
  let searchTimer: number | undefined;
  let searchSeq = 0;
  const busy = new Set<string>();

  const head = h("div", { class: "page-head" });
  const tabs = h("div", { class: "tabs" });
  const banner = h("div", {});
  const renderBanner = () => {
    const p = profile();
    clear(
      banner,
      p && store.running.has(p.id)
        ? h("div", { class: "banner" },
            h("span", {}, "Das Spiel läuft. Mods an/aus wirkt nach einem Neustart. Mod-Einstellungen änderst du direkt im Spiel unter „Mods“ (Mod Menu)."),
            h("button", { class: "btn primary small", onclick: () => { navigate("home"); restart(p); } }, "↻ Jetzt neu starten"),
          )
        : null,
    );
  };
  const installedList = h("div", { class: "item-list" });
  const searchInput = h("input", { type: "search", placeholder: "Modrinth durchsuchen …" }) as HTMLInputElement;
  const results = h("div", { class: "item-list" });
  const body = h(
    "div",
    { class: "mods-layout" },
    h("section", { class: "panel" }, h("div", { class: "panel-head" }, h("h3", {}, "Installiert"), h("div", { class: "panel-tools" })), installedList),
    h("section", { class: "panel" }, h("div", { class: "panel-head" }, searchInput), results),
  );

  const profile = (): Profile | undefined => selectedProfile();

  function renderHead() {
    const p = profile();
    const select = h(
      "select",
      {
        class: "profile-select",
        onchange: async (e: Event) => {
          const id = (e.target as HTMLSelectElement).value;
          await attempt(() => api.selectProfile(id));
          store.selected = id;
          emit("profiles");
        },
      },
      store.profiles.map((x) => h("option", { value: x.id }, `${x.name} – ${LOADER_LABELS[x.loader]} ${x.game_version}`)),
    ) as HTMLSelectElement;
    if (p) select.value = p.id;
    const updateBtn = h("button", { class: "btn ghost small" }, "Alle aktualisieren");
    updateBtn.onclick = async () => {
      if (!p) return;
      updateBtn.disabled = true;
      updateBtn.textContent = "Prüfe …";
      const report = await attempt(() => api.updateContent(p.id));
      updateBtn.disabled = false;
      updateBtn.textContent = "Alle aktualisieren";
      if (report) {
        toast(report.installed.length ? `${report.installed.length} aktualisiert` : "Alles ist aktuell", "success");
        loadInstalled();
      }
    };
    clear(
      head,
      h("h1", {}, "Mods"),
      h("div", { class: "head-tools" },
        select,
        updateBtn,
        h("button", { class: "btn ghost small", onclick: () => p && attempt(() => api.openFolder(p.id, KINDS.find((k) => k.kind === kind)!.folder)) }, "Ordner öffnen"),
      ),
    );
  }

  function renderTabs() {
    clear(
      tabs,
      KINDS.map((k) =>
        h("button", {
            class: k.kind === kind ? "active" : "",
            onclick: () => {
              kind = k.kind;
              renderTabs();
              loadInstalled();
              search();
            },
          },
          k.label,
        ),
      ),
    );
  }

  function renderInstalled() {
    const p = profile();
    if (!p) return;
    clear(
      installedList,
      installed.length === 0 ? h("p", { class: "empty" }, "Noch nichts installiert. Suche rechts nach Inhalten.") : null,
      installed.map((item) =>
        h("div", { class: "item" + (item.enabled ? "" : " disabled") },
          icon(item.icon_url),
          h("div", { class: "item-main" },
            h("strong", {}, item.title),
            h("small", { class: "muted" }, [item.version ?? item.filename, item.dependency ? " · Abhängigkeit" : ""].join("")),
          ),
          toggle(item.enabled, (v) => attempt(() => api.toggleContent(p.id, kind, item.filename, v)).then(loadInstalled)),
          h("button", {
              class: "icon-btn",
              title: "Entfernen",
              onclick: async () => {
                if (await confirmDialog("Entfernen?", `${item.title} entfernen?`, "Entfernen")) {
                  await attempt(() => api.removeContent(p.id, kind, item.filename));
                  loadInstalled();
                }
              },
            },
            "🗑",
          ),
        ),
      ),
    );
  }

  async function loadInstalled() {
    const p = profile();
    if (!p) return;
    installed = (await attempt(() => api.listContent(p.id, kind))) ?? [];
    renderInstalled();
  }

  async function search() {
    const p = profile();
    if (!p) return;
    if (kind === "mod" && p.loader === "vanilla") {
      clear(results, h("p", { class: "empty" }, "Vanilla-Profile unterstützen keine Mods. Stelle das Profil auf Fabric um."));
      return;
    }
    const seq = ++searchSeq;
    clear(results, h("p", { class: "empty" }, "Suche …"));
    try {
      const res = await api.searchContent(p.id, kind, query, 0);
      if (seq !== searchSeq) return;
      const installedIds = new Set(installed.map((i) => i.project_id).filter(Boolean));
      clear(
        results,
        res.hits.length === 0 ? h("p", { class: "empty" }, "Keine Treffer.") : null,
        res.hits.map((hit) => {
          const isInstalled = installedIds.has(hit.project_id);
          const btn = h("button", { class: "btn small " + (isInstalled ? "ghost" : "primary"), disabled: isInstalled || busy.has(hit.project_id) }, isInstalled ? "Installiert" : busy.has(hit.project_id) ? "…" : "Installieren");
          btn.onclick = async () => {
            busy.add(hit.project_id);
            btn.disabled = true;
            btn.textContent = "…";
            try {
              await api.installContent(p.id, hit.project_id, kind);
              toast(`${hit.title} installiert`, "success");
              await loadInstalled();
              btn.textContent = "Installiert";
              btn.className = "btn small ghost";
            } catch (e) {
              toast(errorText(e), "error");
              btn.disabled = false;
              btn.textContent = "Installieren";
            } finally {
              busy.delete(hit.project_id);
            }
          };
          return h("div", { class: "item" },
            icon(hit.icon_url),
            h("div", { class: "item-main" },
              h("strong", {}, hit.title, h("span", { class: "muted small" }, ` von ${hit.author}`)),
              h("small", { class: "muted clamp" }, hit.description),
              h("small", { class: "muted" }, `⬇ ${formatNumber(hit.downloads)}`),
            ),
            btn,
          );
        }),
      );
    } catch (e) {
      if (seq === searchSeq) clear(results, h("p", { class: "empty error" }, errorText(e)));
    }
  }

  searchInput.addEventListener("input", () => {
    query = searchInput.value;
    clearTimeout(searchTimer);
    searchTimer = window.setTimeout(search, 300);
  });

  const refreshAll = () => {
    if (!profile()) {
      clear(root, h("div", { class: "page-head" }, h("h1", {}, "Mods")), h("p", { class: "empty" }, "Lege zuerst ein Profil an."), h("button", { class: "btn primary", onclick: () => navigate("profiles", { create: true }) }, "Profil erstellen"));
      return;
    }
    if (!root.contains(body)) clear(root, head, banner, tabs, body);
    renderBanner();
    renderHead();
    renderTabs();
    loadInstalled().then(search);
  };
  refreshAll();

  const offs = [on("profiles", refreshAll), on("running", renderBanner)];
  return () => offs.forEach((f) => f());
}
