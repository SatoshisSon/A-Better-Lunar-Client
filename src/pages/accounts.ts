import { api } from "../api";
import { on, refreshAccounts, store } from "../store";
import { attempt, clear, confirmDialog, errorText, h, modal, toast } from "../ui";

async function microsoftLogin() {
  const code = await attempt(() => api.msLoginStart());
  if (!code) return;
  const link = `${code.verification_uri}?otc=${encodeURIComponent(code.user_code)}`;
  const status = h("p", { class: "muted" }, "Warte auf Bestätigung im Browser …");
  const body = h(
    "div",
    { class: "device-code" },
    h("p", {}, "Öffne die Microsoft-Anmeldeseite und gib diesen Code ein:"),
    h("div", { class: "code" }, code.user_code),
    h("button", { class: "btn primary", onclick: () => attempt(() => api.openUrl(link)) }, "Anmeldeseite öffnen"),
    h("button", {
        class: "btn ghost small",
        onclick: async () => {
          await navigator.clipboard?.writeText(code.user_code).catch(() => undefined);
          toast("Code kopiert");
        },
      },
      "Code kopieren",
    ),
    status,
  );
  let done = false;
  const m = modal("Mit Microsoft anmelden", body);
  // Schließen des Dialogs bricht das Warten ab.
  new MutationObserver((_, obs) => {
    if (!document.body.contains(m.overlay)) {
      obs.disconnect();
      if (!done) api.msLoginCancel();
    }
  }).observe(document.body, { childList: true });

  api.openUrl(link).catch(() => undefined);
  try {
    const account = await api.msLoginFinish(code);
    done = true;
    toast(`Angemeldet als ${account.name}`, "success");
    m.close();
    await refreshAccounts();
  } catch (e) {
    done = true;
    if (document.body.contains(m.overlay)) {
      status.textContent = errorText(e);
      status.className = "error";
    }
  }
}

function offlineDialog() {
  const input = h("input", { type: "text", placeholder: "Spielername", maxlength: 16 }) as HTMLInputElement;
  const add = h("button", { class: "btn primary" }, "Hinzufügen");
  const m = modal(
    "Offline-Account",
    h("div", { class: "form" },
      h("p", { class: "muted" }, "Offline-Accounts funktionieren nur im Singleplayer und auf Servern im Offline-Modus."),
      input,
    ),
    [add],
  );
  setTimeout(() => input.focus(), 50);
  const submit = async () => {
    if ((await attempt(() => api.addOfflineAccount(input.value))) !== undefined) {
      m.close();
      await refreshAccounts();
    }
  };
  add.onclick = submit;
  input.addEventListener("keydown", (e) => e.key === "Enter" && submit());
}

export function renderAccounts(root: HTMLElement) {
  const list = h("div", { class: "card-list" });

  function render() {
    clear(
      list,
      store.accounts.length === 0 ? h("p", { class: "empty" }, "Noch kein Account. Melde dich mit Microsoft an, um zu spielen.") : null,
      store.accounts.map((a) =>
        h("div", { class: "card account-card" + (a.active ? " active" : "") },
          h("div", { class: "avatar" }, a.name.slice(0, 1).toUpperCase()),
          h("div", { class: "card-main" },
            h("strong", {}, a.name),
            h("span", { class: "muted" }, a.kind === "microsoft" ? "Microsoft-Account" : "Offline-Account"),
          ),
          h("div", { class: "card-actions" },
            a.active
              ? h("span", { class: "chip active" }, "✓ Aktiv")
              : h("button", { class: "btn ghost small", onclick: async () => { await attempt(() => api.setActiveAccount(a.uuid)); refreshAccounts(); } }, "Verwenden"),
            h("button", {
                class: "btn ghost small danger",
                onclick: async () => {
                  if (await confirmDialog("Account entfernen?", `${a.name} aus dem Launcher entfernen?`, "Entfernen")) {
                    await attempt(() => api.removeAccount(a.uuid));
                    refreshAccounts();
                  }
                },
              },
              "Entfernen",
            ),
          ),
        ),
      ),
    );
  }

  render();
  clear(
    root,
    h("div", { class: "page-head" },
      h("h1", {}, "Accounts"),
      h("div", { class: "head-tools" },
        h("button", { class: "btn ghost", onclick: offlineDialog }, "Offline-Account"),
        h("button", { class: "btn primary", onclick: microsoftLogin }, "Mit Microsoft anmelden"),
      ),
    ),
    list,
  );
  return on("accounts", render);
}
