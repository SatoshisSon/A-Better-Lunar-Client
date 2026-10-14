import { init, on, store } from "./store";
import { clear, errorText, h } from "./ui";
import { renderHome } from "./pages/home";
import { renderProfiles } from "./pages/profiles";
import { renderMods } from "./pages/mods";
import { renderAccounts } from "./pages/accounts";
import { renderSettings } from "./pages/settings";
import { renderWorlds } from "./pages/worlds";

export type Page = "home" | "profiles" | "mods" | "worlds" | "accounts" | "settings";
export type Navigate = (page: Page, params?: Record<string, unknown>) => void;

const NAV: { page: Page; label: string; icon: string }[] = [
  { page: "home", label: "Spielen", icon: "▶" },
  { page: "profiles", label: "Profile", icon: "▦" },
  { page: "mods", label: "Mods", icon: "✦" },
  { page: "worlds", label: "Welten & Claude", icon: "◈" },
  { page: "accounts", label: "Accounts", icon: "◉" },
  { page: "settings", label: "Einstellungen", icon: "⚙" },
];

const app = document.getElementById("app")!;
const nav = h("nav", { class: "nav" });
const content = h("main", { class: "content" });
const status = h("div", { class: "sidebar-status" });
let current: Page = "home";
let dispose: (() => void) | void;

const navigate: Navigate = (page, params = {}) => {
  current = page;
  if (dispose) dispose();
  content.scrollTop = 0;
  clear(content);
  renderNav();
  switch (page) {
    case "home":
      dispose = renderHome(content, navigate);
      break;
    case "profiles":
      dispose = renderProfiles(content, navigate, params);
      break;
    case "mods":
      dispose = renderMods(content, navigate);
      break;
    case "worlds":
      renderWorlds(content);
      dispose = undefined;
      break;
    case "accounts":
      dispose = renderAccounts(content);
      break;
    case "settings":
      dispose = renderSettings(content);
      break;
  }
};

function renderNav() {
  clear(
    nav,
    NAV.map((n) =>
      h("button", { class: "nav-item" + (n.page === current ? " active" : ""), onclick: () => navigate(n.page) },
        h("span", { class: "nav-icon" }, n.icon),
        h("span", {}, n.label),
      ),
    ),
  );
}

function renderStatus() {
  const running = store.running.size;
  clear(status, running ? h("span", { class: "dot-running" }, `● ${running} Spiel${running > 1 ? "e" : ""} läuft`) : null);
}

clear(
  app,
  h("aside", { class: "sidebar" },
    h("div", { class: "brand" },
      h("img", { src: "/logo.svg", alt: "" }),
      h("div", {}, h("strong", {}, "A Better"), h("span", {}, "Lunar Client")),
    ),
    nav,
    status,
  ),
  content,
);

// Rechtsklick-Menü und Text-Markierung wie bei einer nativen App ausblenden.
document.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest("input, textarea, .log")) e.preventDefault();
});

init()
  .then(() => {
    on("running", renderStatus);
    renderStatus();
    navigate("home");
  })
  .catch((e) => {
    clear(content, h("div", { class: "empty error" }, `Start fehlgeschlagen: ${errorText(e)}`));
  });
