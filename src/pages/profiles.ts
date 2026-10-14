import { api, LOADER_LABELS, type Loader, type Profile } from "../api";
import { on, refreshProfiles, store } from "../store";
import { attempt, clear, confirmDialog, errorText, field, formatMemory, h, memorySlider, modal, timeAgo, toast } from "../ui";
import type { Navigate } from "../main";
import { play } from "./home";

const LOADERS: Loader[] = ["fabric", "vanilla", "legacy_fabric"];
const LOADER_HINTS: Record<Loader, string> = {
  fabric: "Empfohlen: Mods wie Sodium und Iris (ab 1.14)",
  vanilla: "Reines Minecraft ohne Mods",
  legacy_fabric: "Fabric für alte Versionen wie 1.8.9 (weniger Mods verfügbar)",
};

function maxMemory() {
  const total = store.system?.total_memory_mb ?? 8192;
  return Math.max(2048, Math.floor((total * 0.75) / 512) * 512);
}

function loaderPicker(value: Loader, onChange: (l: Loader) => void) {
  const wrap = h("div", { class: "segmented" });
  const hint = h("small", { class: "hint" }, LOADER_HINTS[value]);
  const render = () =>
    clear(
      wrap,
      LOADERS.map((l) =>
        h("button", {
            type: "button",
            class: l === value ? "active" : "",
            onclick: () => {
              value = l;
              hint.textContent = LOADER_HINTS[l];
              render();
              onChange(l);
            },
          },
          LOADER_LABELS[l],
        ),
      ),
    );
  render();
  return h("div", {}, wrap, hint);
}

/** Auswahlfeld mit den Minecraft-Versionen, die der Loader unterstützt. */
function versionSelect(initial: string | null) {
  const select = h("select", {}, h("option", {}, "Lade Versionen …")) as HTMLSelectElement;
  let current = initial;
  select.addEventListener("change", () => (current = select.value));
  async function load(loader: Loader) {
    select.disabled = true;
    try {
      const versions = await api.listVersions(loader, store.settings?.show_snapshots ?? false);
      clear(select, versions.map((v) => h("option", { value: v.id }, v.kind === "snapshot" ? `${v.id} (Snapshot)` : v.id)));
      if (current && versions.some((v) => v.id === current)) select.value = current;
      current = select.value;
    } catch (e) {
      clear(select, h("option", {}, "Fehler beim Laden"));
      toast(errorText(e), "error");
    } finally {
      select.disabled = false;
    }
  }
  return { select, load, value: () => select.value };
}

export async function openCreateDialog(onDone?: (p: Profile) => void) {
  let loader: Loader = "fabric";
  let memory = store.settings?.default_memory_mb ?? 4096;
  const presets = await api.getPresets().catch(() => []);
  const chosen = new Set(["performance", "visuals"]);

  const name = h("input", { type: "text", placeholder: "z. B. Survival", value: "" }) as HTMLInputElement;
  const versions = versionSelect(null);
  const presetBox = h("div", { class: "preset-list" });
  const renderPresets = () =>
    clear(
      presetBox,
      loader === "vanilla"
        ? h("p", { class: "hint" }, "Vanilla unterstützt keine Mods.")
        : presets.map((p) => {
            const cb = h("input", { type: "checkbox", checked: chosen.has(p.id) }) as HTMLInputElement;
            cb.addEventListener("change", () => (cb.checked ? chosen.add(p.id) : chosen.delete(p.id)));
            return h("label", { class: "preset" }, cb, h("div", {}, h("strong", {}, p.name), h("small", {}, p.description)));
          }),
    );
  renderPresets();

  const status = h("div", { class: "modal-status" });
  const body = h(
    "div",
    { class: "form" },
    field("Name", name),
    field("Mod-Loader", loaderPicker(loader, (l) => {
      loader = l;
      renderPresets();
      versions.load(l);
    })),
    field("Minecraft-Version", versions.select),
    field("Vorinstallierte Mods", presetBox),
    field("Arbeitsspeicher", memorySlider(memory, maxMemory(), (v) => (memory = v))),
    status,
  );
  versions.load(loader);

  const create = h("button", { class: "btn primary" }, "Erstellen");
  const cancel = h("button", { class: "btn ghost" }, "Abbrechen");
  const m = modal("Neues Profil", body, [cancel, create]);
  cancel.onclick = m.close;
  setTimeout(() => name.focus(), 50);

  create.onclick = async () => {
    const gameVersion = versions.value();
    if (!gameVersion) return toast("Bitte eine Version wählen", "error");
    create.disabled = cancel.disabled = true;
    try {
      const profile = await api.createProfile({
        name: name.value.trim() || `Minecraft ${gameVersion}`,
        game_version: gameVersion,
        loader,
        memory_mb: memory === store.settings?.default_memory_mb ? null : memory,
      });
      if (loader !== "vanilla" && chosen.size) {
        const off = on("progress", () => {
          const p = store.progress;
          if (p?.task === "mods") status.textContent = `${p.stage} (${p.done}/${p.total})`;
        });
        status.textContent = "Mods werden installiert …";
        try {
          const report = await api.installPresets(profile.id, [...chosen]);
          if (report.unavailable.length) {
            toast(`Für ${gameVersion} nicht verfügbar: ${report.unavailable.join(", ")}`, "info");
          }
          toast(`${report.installed.length} Mods installiert`, "success");
        } catch (e) {
          toast(`Profil erstellt, aber Mods fehlgeschlagen: ${errorText(e)}`, "error");
        } finally {
          off();
        }
      }
      await refreshProfiles();
      m.close();
      onDone?.(profile);
    } catch (e) {
      toast(errorText(e), "error");
      create.disabled = cancel.disabled = false;
    }
  };
}

export function openEditDialog(original: Profile) {
  const p: Profile = { ...original };
  const name = h("input", { type: "text", value: p.name }) as HTMLInputElement;
  const versions = versionSelect(p.game_version);
  const loaderVersion = h("select", {}) as HTMLSelectElement;

  async function loadLoaderVersions() {
    clear(loaderVersion, h("option", { value: "" }, "Neueste stabile (empfohlen)"));
    loaderVersion.disabled = p.loader === "vanilla";
    if (p.loader === "vanilla" || !versions.value()) return;
    const list = await api.listLoaderVersions(p.loader, versions.value()).catch(() => []);
    for (const v of list.slice(0, 40)) loaderVersion.append(h("option", { value: v.version }, v.stable ? v.version : `${v.version} (Beta)`));
    loaderVersion.value = p.loader_version ?? "";
  }
  versions.select.addEventListener("change", () => {
    p.loader_version = null;
    loadLoaderVersions();
  });
  versions.load(p.loader).then(loadLoaderVersions);

  const useDefaultMem = h("input", { type: "checkbox", checked: p.memory_mb === null }) as HTMLInputElement;
  let memory = p.memory_mb ?? store.settings?.default_memory_mb ?? 4096;
  const slider = memorySlider(memory, maxMemory(), (v) => (memory = v));
  slider.style.display = useDefaultMem.checked ? "none" : "";
  useDefaultMem.addEventListener("change", () => (slider.style.display = useDefaultMem.checked ? "none" : ""));

  const width = h("input", { type: "number", min: 0, placeholder: "Breite", value: p.width ?? "" }) as HTMLInputElement;
  const height = h("input", { type: "number", min: 0, placeholder: "Höhe", value: p.height ?? "" }) as HTMLInputElement;
  const jvm = h("input", { type: "text", placeholder: "z. B. -Dfoo=bar", value: p.jvm_args }) as HTMLInputElement;

  const body = h(
    "div",
    { class: "form" },
    field("Name", name),
    field("Mod-Loader", loaderPicker(p.loader, (l) => {
      p.loader = l;
      p.loader_version = null;
      versions.load(l).then(loadLoaderVersions);
    })),
    h("div", { class: "row" }, field("Minecraft-Version", versions.select), field("Loader-Version", loaderVersion)),
    field("Arbeitsspeicher", h("div", {}, h("label", { class: "check" }, useDefaultMem, ` Standard verwenden (${formatMemory(store.settings?.default_memory_mb ?? 4096)})`), slider)),
    field("Fenstergröße", h("div", { class: "row" }, width, height), "Leer lassen für Standard"),
    field("Eigene JVM-Argumente", jvm, "Nur für Fortgeschrittene"),
  );

  const save = h("button", { class: "btn primary" }, "Speichern");
  const del = h("button", { class: "btn danger ghost" }, "Löschen");
  const m = modal(`Profil bearbeiten`, body, [del, h("div", { class: "spacer" }), save]);

  del.onclick = async () => {
    if (!(await confirmDialog("Profil löschen?", `„${p.name}“ inkl. Welten, Mods und Einstellungen endgültig löschen?`, "Löschen"))) return;
    if ((await attempt(() => api.deleteProfile(p.id, true))) !== undefined) {
      m.close();
      await refreshProfiles();
      toast("Profil gelöscht", "success");
    }
  };

  save.onclick = async () => {
    const updated: Profile = {
      ...p,
      name: name.value.trim() || p.name,
      game_version: versions.value() || p.game_version,
      loader_version: loaderVersion.value || null,
      memory_mb: useDefaultMem.checked ? null : memory,
      width: Number(width.value) || null,
      height: Number(height.value) || null,
      jvm_args: jvm.value,
    };
    if ((await attempt(() => api.updateProfile(updated))) === undefined) return;
    m.close();
    await refreshProfiles();
    const changed = updated.game_version !== original.game_version || updated.loader !== original.loader;
    if (changed && updated.loader !== "vanilla") {
      toast("Mods werden an die neue Version angepasst …");
      const report = await attempt(() => api.updateContent(updated.id));
      if (report) {
        toast(`${report.installed.length} Mods aktualisiert`, "success");
        if (report.unavailable.length) toast(`Nicht verfügbar für ${updated.game_version}: ${report.unavailable.join(", ")}`, "info");
      }
    } else {
      toast("Gespeichert", "success");
    }
  };
}

export function renderProfiles(root: HTMLElement, navigate: Navigate, params: Record<string, unknown> = {}) {
  const list = h("div", { class: "card-list" });

  function render() {
    const sorted = [...store.profiles].sort((a, b) => (b.last_played ?? b.created_at) - (a.last_played ?? a.created_at));
    clear(
      list,
      sorted.length === 0 ? h("p", { class: "empty" }, "Noch keine Profile.") : null,
      sorted.map((p) =>
        h("div", { class: "card profile-card" + (p.id === store.selected ? " active" : "") },
          h("div", { class: "card-main" },
            h("strong", {}, p.name),
            h("span", { class: "muted" }, `${LOADER_LABELS[p.loader]} · ${p.game_version} · ${timeAgo(p.last_played)}`),
          ),
          h("div", { class: "card-actions" },
            store.running.has(p.id)
              ? h("button", { class: "btn danger small", onclick: () => attempt(() => api.kill(p.id)) }, "Beenden")
              : h("button", { class: "btn primary small", disabled: !!store.launching, onclick: () => { navigate("home"); play(p); } }, "Spielen"),
            h("button", { class: "btn ghost small", onclick: async () => { await attempt(() => api.selectProfile(p.id)); store.selected = p.id; navigate("mods"); } }, "Mods"),
            h("button", { class: "btn ghost small", onclick: () => attempt(() => api.openFolder(p.id)) }, "Ordner"),
            h("button", { class: "btn ghost small", onclick: () => openEditDialog(p) }, "Bearbeiten"),
          ),
        ),
      ),
    );
  }

  render();
  clear(
    root,
    h("div", { class: "page-head" }, h("h1", {}, "Profile"), h("button", { class: "btn primary", onclick: () => openCreateDialog() }, "+ Neues Profil")),
    list,
  );

  if (params.create) openCreateDialog();
  if (typeof params.edit === "string") {
    const p = store.profiles.find((x) => x.id === params.edit);
    if (p) openEditDialog(p);
  }

  const offs = [on("profiles", render), on("running", render)];
  return () => offs.forEach((f) => f());
}
