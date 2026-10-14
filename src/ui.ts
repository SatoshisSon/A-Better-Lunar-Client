// Kleine DOM-Helfer – kein Framework, damit der Launcher schlank bleibt.

type Child = Node | string | number | null | undefined | false;
type Attrs = Record<string, unknown>;

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  ...children: (Child | Child[])[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value === undefined || value === null || value === false) continue;
    if (key.startsWith("on") && typeof value === "function") {
      el.addEventListener(key.slice(2).toLowerCase(), value as EventListener);
    } else if (key === "class") {
      el.className = String(value);
    } else if (key === "value" && "value" in el) {
      (el as HTMLInputElement).value = String(value);
    } else if (key === "checked" && "checked" in el) {
      (el as HTMLInputElement).checked = Boolean(value);
    } else if (value === true) {
      el.setAttribute(key, "");
    } else {
      el.setAttribute(key, String(value));
    }
  }
  for (const child of children.flat()) {
    if (child === null || child === undefined || child === false) continue;
    el.append(child instanceof Node ? child : String(child));
  }
  return el;
}

export function clear(el: HTMLElement, ...children: (Child | Child[])[]) {
  el.replaceChildren();
  for (const child of children.flat()) {
    if (child === null || child === undefined || child === false) continue;
    el.append(child instanceof Node ? child : String(child));
  }
}

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}

let toastHost: HTMLElement | null = null;
export function toast(message: string, kind: "info" | "error" | "success" = "info") {
  if (!toastHost) {
    toastHost = h("div", { class: "toasts" });
    document.body.append(toastHost);
  }
  const t = h("div", { class: `toast ${kind}` }, message);
  toastHost.append(t);
  setTimeout(() => t.classList.add("hide"), kind === "error" ? 6000 : 3500);
  setTimeout(() => t.remove(), kind === "error" ? 6500 : 4000);
}

/** Führt eine Aktion aus und zeigt Fehler als Hinweis an. */
export async function attempt<T>(fn: () => Promise<T>): Promise<T | undefined> {
  try {
    return await fn();
  } catch (e) {
    toast(errorText(e), "error");
    return undefined;
  }
}

export function modal(title: string, body: HTMLElement, actions: HTMLElement[] = []) {
  const close = () => overlay.remove();
  const overlay = h(
    "div",
    {
      class: "overlay",
      onmousedown: (e: MouseEvent) => {
        if (e.target === overlay) close();
      },
    },
    h(
      "div",
      { class: "modal" },
      h("div", { class: "modal-head" }, h("h2", {}, title), h("button", { class: "icon-btn", onclick: close, title: "Schließen" }, "✕")),
      h("div", { class: "modal-body" }, body),
      actions.length ? h("div", { class: "modal-actions" }, actions) : null,
    ),
  );
  document.body.append(overlay);
  return { close, overlay };
}

export function confirmDialog(title: string, text: string, confirmLabel = "Bestätigen"): Promise<boolean> {
  return new Promise((resolve) => {
    const yes = h("button", { class: "btn danger" }, confirmLabel);
    const no = h("button", { class: "btn ghost" }, "Abbrechen");
    const m = modal(title, h("p", {}, text), [no, yes]);
    yes.onclick = () => {
      m.close();
      resolve(true);
    };
    no.onclick = () => {
      m.close();
      resolve(false);
    };
  });
}

export function field(label: string, input: HTMLElement, hint?: string) {
  return h("label", { class: "field" }, h("span", { class: "field-label" }, label), input, hint ? h("small", { class: "hint" }, hint) : null);
}

export function toggle(checked: boolean, onChange: (v: boolean) => void, label?: string) {
  const input = h("input", { type: "checkbox", checked }) as HTMLInputElement;
  input.addEventListener("change", () => onChange(input.checked));
  return h("label", { class: "switch" }, input, h("span", { class: "slider" }), label ? h("span", { class: "switch-label" }, label) : null);
}

export function formatMemory(mb: number) {
  return mb >= 1024 ? `${(mb / 1024).toFixed(mb % 1024 === 0 ? 0 : 1)} GB` : `${mb} MB`;
}

export function formatNumber(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)} Mio.`;
  if (n >= 1_000) return `${Math.round(n / 1_000)} Tsd.`;
  return String(n);
}

export function timeAgo(secs: number | null) {
  if (!secs) return "noch nie gespielt";
  const diff = Date.now() / 1000 - secs;
  if (diff < 60) return "gerade eben";
  if (diff < 3600) return `vor ${Math.floor(diff / 60)} Min.`;
  if (diff < 86400) return `vor ${Math.floor(diff / 3600)} Std.`;
  return `vor ${Math.floor(diff / 86400)} Tagen`;
}

export function memorySlider(value: number, max: number, onChange: (mb: number) => void) {
  const label = h("span", { class: "range-value" }, formatMemory(value));
  const input = h("input", { type: "range", min: 1024, max, step: 512, value }) as HTMLInputElement;
  input.addEventListener("input", () => {
    label.textContent = formatMemory(Number(input.value));
    onChange(Number(input.value));
  });
  return h("div", { class: "range" }, input, label);
}
