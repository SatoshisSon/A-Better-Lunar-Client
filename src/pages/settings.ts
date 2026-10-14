import { api, type Settings } from "../api";
import { store } from "../store";
import { attempt, clear, field, h, memorySlider, toast, toggle } from "../ui";

export function renderSettings(root: HTMLElement) {
  if (!store.settings) return;
  const s: Settings = { ...store.settings };
  const total = store.system?.total_memory_mb ?? 8192;
  const max = Math.max(2048, Math.floor((total * 0.75) / 512) * 512);

  let timer: number | undefined;
  const save = () => {
    clearTimeout(timer);
    timer = window.setTimeout(async () => {
      if ((await attempt(() => api.saveSettings(s))) !== undefined) {
        store.settings = { ...s };
        toast("Gespeichert", "success");
      }
    }, 400);
  };

  const text = (key: "ms_client_id" | "discord_app_id" | "java_path", placeholder: string) => {
    const input = h("input", { type: "text", value: s[key], placeholder }) as HTMLInputElement;
    input.addEventListener("input", () => {
      s[key] = input.value.trim();
      save();
    });
    return input;
  };

  const downloads = h("input", { type: "number", min: 1, max: 64, value: s.concurrent_downloads }) as HTMLInputElement;
  downloads.addEventListener("change", () => {
    s.concurrent_downloads = Math.min(64, Math.max(1, Number(downloads.value) || 32));
    save();
  });

  clear(
    root,
    h("div", { class: "page-head" }, h("h1", {}, "Einstellungen")),
    h("div", { class: "settings" },
      h("section", { class: "panel form" },
        h("h3", {}, "Leistung"),
        field("Standard-Arbeitsspeicher", memorySlider(s.default_memory_mb, max, (v) => { s.default_memory_mb = v; save(); }),
          `Dein PC hat ${Math.round(total / 1024)} GB. Mehr ist nicht immer besser – 4–6 GB reichen meist, auch mit Shadern.`),
        toggle(s.optimized_jvm_flags, (v) => { s.optimized_jvm_flags = v; save(); }, "Optimierte JVM-Flags (G1GC, weniger Ruckler)"),
        field("Parallele Downloads", downloads),
      ),
      h("section", { class: "panel form" },
        h("h3", {}, "Launcher"),
        toggle(s.hide_launcher_on_launch, (v) => { s.hide_launcher_on_launch = v; save(); }, "Launcher beim Spielstart minimieren"),
        toggle(s.show_snapshots, (v) => { s.show_snapshots = v; save(); }, "Snapshots in der Versionsliste anzeigen"),
        field("Eigener Java-Pfad", text("java_path", "Leer = automatisch passende Java-Version laden"),
          "Normalerweise leer lassen: Der Launcher lädt für jede Minecraft-Version das richtige Java."),
      ),
      h("section", { class: "panel form" },
        h("h3", {}, "Discord"),
        toggle(s.discord_enabled, (v) => { s.discord_enabled = v; save(); }, "Discord-Status anzeigen („Spielt Minecraft 1.21.1“)"),
        field("Discord-Application-ID", text("discord_app_id", "ID aus dem Discord Developer Portal")),
      ),
      h("section", { class: "panel form" },
        h("h3", {}, "Microsoft-Login"),
        field("Azure-Client-ID", text("ms_client_id", "00000000-0000-0000-0000-000000000000"),
          "Wird für den Microsoft-Login gebraucht. Die Anleitung steht in der README."),
      ),
      h("section", { class: "panel form" },
        h("h3", {}, "Über"),
        h("p", { class: "muted" }, `A Better Lunar Client v${store.system?.version ?? "?"} – keine Werbung, keine Telemetrie, Open Source.`),
        h("p", { class: "muted small" }, `Daten: ${store.system?.data_dir ?? ""}`),
        h("button", { class: "btn ghost small", onclick: () => attempt(() => api.openFolder()) }, "Datenordner öffnen"),
      ),
    ),
  );
}
