// OpenPager browser extension: tells openpager which site is active and carries out board keys on
// the page (spec/browser.md). It talks only to the openpager native messaging host.
//
// Privacy: at install it can see which site is active, nothing on the page. Board buttons are
// turned on per site (popup), which grants that one site; only then does it act on or describe
// the page.

const api = globalThis.browser ?? globalThis.chrome;
const HOST = "io.github.jalabulajunx.openpager";
const BROWSER = typeof globalThis.browser !== "undefined" && api.runtime.getURL("").startsWith("moz-") ? "firefox" : "chrome";

let port = null;
let serviceRunning = false;
let hostError = "";
let retry = null;
/** The last "Configure keys for this site" progress, for the popup. */
let configureStatus = null;

function connect() {
  clearTimeout(retry);
  try {
    port = api.runtime.connectNative(HOST);
  } catch (e) {
    port = null;
    hostError = String(e.message || e);
    retry = setTimeout(connect, 5000);
    return;
  }
  port.onMessage.addListener(onMessage);
  port.onDisconnect.addListener((p) => {
    const error = (p && p.error) || api.runtime.lastError;
    hostError = error ? error.message : "openpager is not reachable";
    port = null;
    serviceRunning = false;
    retry = setTimeout(connect, 5000);
  });
  hostError = "";
  send({ type: "hello", browser: BROWSER, version: api.runtime.getManifest().version });
  reportTab();
}

function send(message) {
  if (!port) return;
  try { port.postMessage(message); } catch { /* reconnecting */ }
}

/** "https://www.example.com/*": the host permission for a page's site. */
function sitePattern(url) {
  const u = new URL(url);
  return `${u.protocol}//${u.hostname}/*`;
}

function isWeb(url) {
  return typeof url === "string" && /^https?:\/\//.test(url);
}

async function activeTab() {
  const [tab] = await api.tabs.query({ active: true, lastFocusedWindow: true });
  return tab;
}

async function granted(url) {
  return isWeb(url) && api.permissions.contains({ origins: [sitePattern(url)] });
}

// ---- telling openpager which site is active ----

let reportTimer = null;
function reportTab() {
  clearTimeout(reportTimer);
  reportTimer = setTimeout(async () => {
    const window = await api.windows.getLastFocused().catch(() => null);
    const tab = await activeTab().catch(() => null);
    const url = tab && isWeb(tab.url) ? tab.url : null;
    send({ type: "tab", url, title: url ? (tab.title || "") : "", windowFocused: !!(window && window.focused),
           granted: url ? await granted(url) : false });
  }, 150);
}

api.tabs.onActivated.addListener(reportTab);
api.tabs.onUpdated.addListener((tabId, change, tab) => {
  if (tab.active && (change.url || change.status === "complete" || change.title)) reportTab();
});
api.windows.onFocusChanged.addListener(reportTab);
api.permissions.onAdded.addListener(reportTab);
api.permissions.onRemoved.addListener(reportTab);

// ---- acting on the page ----

async function onMessage(message) {
  if (message.type === "configure-status") {
    configureStatus = { ok: !!message.ok, text: String(message.text || ""), done: true };
    return;
  }
  if (message.type === "service") {
    serviceRunning = !!message.running;
    if (serviceRunning) reportTab();
    return;
  }
  if (message.type === "grant") {
    send({ type: "result", request: message.request, ...(await askToGrant(message.request)) });
  } else if (message.type === "act") {
    send({ type: "result", request: message.request, ...(await act(message.action)) });
  } else if (message.type === "inventory") {
    send({ type: "inventory", request: message.request, ...(await inventory()) });
  }
}

async function act(action) {
  const tab = await activeTab();
  if (!tab || !isWeb(tab.url)) return { ok: false, error: "not-active" };
  if (!action || typeof action.kind !== "string") return { ok: false, error: "bad-action" };
  if (action.kind === "open") {
    // A path on the current site only.
    if (typeof action.url !== "string" || !action.url.startsWith("/") || action.url.startsWith("//")) return { ok: false, error: "bad-action" };
    const target = new URL(action.url, tab.url);
    if (target.origin !== new URL(tab.url).origin) return { ok: false, error: "bad-action" };
    await api.tabs.update(tab.id, { url: target.href });
    return { ok: true };
  }
  if (!["element", "focus", "media"].includes(action.kind)) return { ok: false, error: "bad-action" };
  if (!(await granted(tab.url))) return { ok: false, error: "not-granted" };
  try {
    const [result] = await api.scripting.executeScript({ target: { tabId: tab.id }, func: pageAction, args: [action] });
    return result && result.result ? result.result : { ok: false, error: "no-result" };
  } catch (e) {
    return { ok: false, error: String(e.message || e).slice(0, 120) };
  }
}

// ---- the board asks for a site that is not allowed yet ----

const grants = new Map();  // request -> { resolve, pageTab, grantTab }

/** Opens a small page asking to allow the active site; resolves when the person answers (or after
 *  two minutes). Returns to the page afterwards. */
async function askToGrant(request) {
  const tab = await activeTab();
  if (!tab || !isWeb(tab.url)) return { ok: false, error: "not-active" };
  if (await granted(tab.url)) return { ok: true };
  const origin = new URL(tab.url).origin;
  const page = api.runtime.getURL(`grant.html?origin=${encodeURIComponent(origin)}&request=${encodeURIComponent(request)}`);
  const grantTab = await api.tabs.create({ url: page, index: tab.index + 1, openerTabId: tab.id });
  return new Promise((resolve) => {
    grants.set(request, { resolve, pageTab: tab.id, grantTab: grantTab.id });
    setTimeout(() => settleGrant(request, false), 120000);
  });
}

async function settleGrant(request, ok) {
  const pending = grants.get(request);
  if (!pending) return;
  grants.delete(request);
  await api.tabs.update(pending.pageTab, { active: true }).catch(() => {});
  await api.tabs.remove(pending.grantTab).catch(() => {});
  pending.resolve(ok ? { ok: true } : { ok: false, error: "not-granted" });
}

async function inventory() {
  const tab = await activeTab();
  if (!tab || !isWeb(tab.url)) return { ok: false, error: "not-active" };
  if (!(await granted(tab.url))) return { ok: false, error: "not-granted" };
  try {
    const [result] = await api.scripting.executeScript({ target: { tabId: tab.id }, func: pageInventory });
    return { ok: true, url: tab.url, items: result.result };
  } catch (e) {
    return { ok: false, error: String(e.message || e).slice(0, 120) };
  }
}

// The two functions below run inside the page (scripting.executeScript), so they must not use
// anything from this file.

async function pageAction(action) {
  const find = (selector) => { try { return document.querySelector(selector); } catch { return null; } };
  if (action.kind === "element" || action.kind === "focus") {
    const el = find(action.selector);
    if (!el) return { ok: false, error: "not-found" };
    el.scrollIntoView({ block: "nearest" });
    if (action.kind === "element") {
      // A whole press, not just "click": many web apps (Google Docs, Slides, Sheets) act on the
      // mouse going down and up and ignore a bare click event.
      const r = el.getBoundingClientRect();
      const at = { bubbles: true, cancelable: true, composed: true, view: window, button: 0,
                   clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 };
      const pointer = { ...at, pointerId: 1, pointerType: "mouse", isPrimary: true };
      el.dispatchEvent(new PointerEvent("pointerdown", { ...pointer, buttons: 1 }));
      el.dispatchEvent(new MouseEvent("mousedown", { ...at, buttons: 1 }));
      el.dispatchEvent(new PointerEvent("pointerup", pointer));
      el.dispatchEvent(new MouseEvent("mouseup", at));
      el.dispatchEvent(new MouseEvent("click", at));
    } else {
      el.focus();
      if (typeof el.select === "function") el.select();
    }
    return { ok: true };
  }
  if (action.kind === "media") {
    if ((action.command === "next" || action.command === "previous") && action.selector) {
      const button = find(action.selector);
      if (!button) return { ok: false, error: "not-found" };
      button.click();
      return { ok: true };
    }
    const media = [...document.querySelectorAll("video, audio")]
      .filter((m) => m.currentSrc || m.src)
      .sort((a, b) => b.clientWidth * b.clientHeight - a.clientWidth * a.clientHeight)[0];
    if (!media) return { ok: false, error: "not-found" };
    // Report what actually happened: browsers refuse play() from a script without a click.
    const play = async () => {
      try { await media.play(); } catch { /* refused */ }
      return media.paused ? { ok: false, error: "blocked" } : { ok: true };
    };
    switch (action.command) {
      case "toggle": return media.paused ? play() : (media.pause(), { ok: media.paused });
      case "play": return play();
      case "pause": media.pause(); return { ok: media.paused };
      case "mute": media.muted = !media.muted; return { ok: true };
      case "forward": media.currentTime = Math.min(media.duration || Infinity, media.currentTime + 10); return { ok: true };
      case "back": media.currentTime = Math.max(0, media.currentTime - 10); return { ok: true };
      default: return { ok: false, error: "not-found" };
    }
  }
  return { ok: false, error: "bad-action" };
}

/** The page's controls (spec/browser.md section 3): role, accessible name, a unique selector.
 *  Never page text or field values. */
function pageInventory() {
  const ROLES = new Set(["button", "link", "textbox", "searchbox", "tab", "checkbox", "menuitem", "media", "combobox", "switch"]);
  const roleOf = (el) => {
    const role = el.getAttribute("role");
    if (role) return role;
    const tag = el.tagName.toLowerCase();
    if (tag === "a") return "link";
    if (tag === "button" || tag === "summary") return "button";
    if (tag === "textarea") return "textbox";
    if (tag === "select") return "combobox";
    if (tag === "video" || tag === "audio") return "media";
    if (tag === "input") {
      const type = (el.type || "text").toLowerCase();
      if (type === "search") return "searchbox";
      if (["button", "submit", "reset", "image"].includes(type)) return "button";
      if (type === "checkbox" || type === "radio") return "checkbox";
      if (["hidden", "password", "file"].includes(type)) return null;
      return "textbox";
    }
    return null;
  };
  const clean = (text) => (text || "").replace(/\s+/g, " ").trim().slice(0, 60);
  const nameOf = (el) => {
    const label = el.getAttribute("aria-label") || el.getAttribute("title") || el.getAttribute("alt")
      || (el.labels && el.labels[0] && el.labels[0].innerText) || el.getAttribute("placeholder");
    if (label) return clean(label);
    if (["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName)) return "";  // never a typed value
    const own = clean(el.innerText);
    return own.length <= 40 ? own : "";  // a control's own short text, not page content
  };
  const unique = (selector) => { try { return document.querySelectorAll(selector).length === 1; } catch { return false; } };
  const quote = (value) => `"${value.replace(/["\\]/g, "\\$&")}"`;
  const selectorOf = (el) => {
    const tag = el.tagName.toLowerCase();
    const tries = [];
    if (el.id && /^[A-Za-z][\w-]{0,60}$/.test(el.id) && !/\d{4,}/.test(el.id)) tries.push(`#${CSS.escape(el.id)}`);
    for (const attr of ["data-testid", "data-test", "data-qa", "data-test-id"]) {
      const v = el.getAttribute(attr);
      if (v) tries.push(`[${attr}=${quote(v)}]`);
    }
    const aria = el.getAttribute("aria-label");
    if (aria) tries.push(`${tag}[aria-label=${quote(aria)}]`);
    const name = el.getAttribute("name");
    if (name) tries.push(`${tag}[name=${quote(name)}]`);
    for (const s of tries) if (unique(s)) return s;
    // A short path of tags and positions.
    const parts = [];
    for (let node = el; node && node.nodeType === 1 && parts.length < 5; node = node.parentElement) {
      if (node.id && /^[A-Za-z][\w-]{0,60}$/.test(node.id) && node !== el) { parts.unshift(`#${CSS.escape(node.id)}`); break; }
      const siblings = node.parentElement ? [...node.parentElement.children].filter((c) => c.tagName === node.tagName) : [];
      parts.unshift(siblings.length > 1 ? `${node.tagName.toLowerCase()}:nth-of-type(${siblings.indexOf(node) + 1})` : node.tagName.toLowerCase());
    }
    const path = parts.join(" > ");
    return unique(path) ? path : null;
  };
  const items = [];
  const candidates = document.querySelectorAll("a[href], button, summary, input, textarea, select, video, audio, [role], [accesskey], [aria-keyshortcuts]");
  for (const el of candidates) {
    if (items.length >= 300) break;
    const role = roleOf(el);
    if (!role || !ROLES.has(role)) continue;
    if (el.disabled || el.getAttribute("aria-disabled") === "true" || !el.getClientRects().length) continue;
    const name = nameOf(el);
    if (!name && role !== "media") continue;
    const selector = selectorOf(el);
    if (!selector) continue;
    const item = { role, name, selector };
    const shortcut = el.getAttribute("aria-keyshortcuts") || el.getAttribute("accesskey");
    if (shortcut) item.shortcut = shortcut.slice(0, 30);
    if (role === "link") {
      try {
        const u = new URL(el.href, location.href);
        // Path and query, plus an app's in-page route ("#/album/all") when it keeps one there.
        if (u.origin === location.origin) item.href = (u.pathname + u.search + (u.hash.startsWith("#/") ? u.hash : "")).slice(0, 200);
      } catch { /* not a URL */ }
    }
    items.push(item);
  }
  return items;
}

// ---- the popup asks for status ----

api.runtime.onMessage.addListener((message, sender, reply) => {
  if (message && message.type === "status") {
    reply({ host: !!port, service: serviceRunning, hostError, configure: configureStatus });
  } else if (message && message.type === "grant-result") {
    settleGrant(message.request, !!message.granted);
  } else if (message && message.type === "configure") {
    configureStatus = { ok: true, text: "Reading the page and asking the AI for keys (about 10 seconds)...", done: false };
    send({ type: "configure" });
    reply(configureStatus);
  }
});

connect();
