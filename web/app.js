// KnoEnd web app: talks only to the local service that served it.
// Every API call carries the session token the service put in this page.

const TOKEN = document.querySelector('meta[name="knoend-token"]').content;
const $ = (id) => document.getElementById(id);

async function api(path, body) {
  const options = { headers: { "x-knoend-token": TOKEN } };
  if (body !== undefined) {
    options.method = "POST";
    options.headers["content-type"] = "application/json";
    options.body = JSON.stringify(body);
  }
  const response = await fetch(`/api${path}`, options);
  const data = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(data.error || `${response.status}`);
  return data;
}

function time() {
  return new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function logTo(list, text, cls) {
  const li = document.createElement("li");
  const t = document.createElement("span");
  t.className = "t";
  t.textContent = time();
  li.append(t, document.createTextNode(text));
  if (cls) li.className = cls;
  list.prepend(li);
  while (list.children.length > 80) list.lastChild.remove();
}

// ---- views ----

function show(view) {
  document.querySelectorAll(".view").forEach((v) => (v.hidden = v.id !== `view-${view}`));
  document.querySelectorAll(".tabs button").forEach((b) => b.setAttribute("aria-selected", b.dataset.view === view));
  if (view === "setup" && setup.step === 1) scanPorts();
  if (view === "edit") loadEdit();
  showTip();
  if (!$("assistant").hidden) renderSuggestions();
}
document.querySelectorAll(".tabs button").forEach((b) => b.addEventListener("click", () => {
  if (b.dataset.view === "setup" && setup.task !== "setup") { setTask("setup"); step(1); }
  if (b.dataset.view === "setup") { setup.picked = false; renderKnown(); }
  show(b.dataset.view);
}));
document.addEventListener("click", (e) => {
  const go = e.target.closest("[data-goto]");
  if (go) { e.preventDefault(); show(go.dataset.goto); }
  const back = e.target.closest("[data-back]");
  if (back) step(Number(back.dataset.back));
});

// ---- dashboard ----

function renderStatus(s) {
  renderStatus.last = s;
  const pill = $("link-pill");
  const text = { connected: "Board connected", searching: "Looking for the board", "no-pairing": "No board paired" }[s.link] || "Starting";
  pill.textContent = text;
  pill.className = "pill " + (s.link === "connected" ? "ok" : s.link === "searching" ? "warn" : "");

  $("board-empty").hidden = !!s.board;
  $("board-facts").hidden = !s.board;
  if (s.board) {
    $("f-profile").textContent = s.board.profile;
    $("f-firmware").textContent = s.board.firmware;
    $("f-address").textContent = s.board.address;
    $("f-id").textContent = s.board.id;
  }
  // A new board connection (a restart for a new look, other firmware): the choices follow it.
  const look = s.board ? JSON.stringify([s.board.firmware, s.board.display]) : "";
  if (look !== renderStatus.look) {
    renderStatus.look = look;
    loadDisplay().catch(() => {});
    if ($("theme-note") && s.board) $("theme-note").hidden = !!s.board.display;
  }
  renderKnown();
  $("now-app").textContent = s.app || "-";
  const layout = s.link === "connected" ? s.layout : null;
  $("now-layout").textContent = layout ? layout.name + (layout.pages > 1 ? ` (page ${layout.page} of ${layout.pages})` : "") : "-";
  const badge = $("now-badge");
  badge.hidden = !layout || layout.reviewed;
  badge.textContent = "AI draft, unreviewed";

  renderStatus.layout = layout;
  drawScreen($("keys-preview"), layout, displayState);
  drawScreen($("setup-preview"), layout, displayState);
  const conf = $("configuring");
  conf.hidden = !s.configuring;
  conf.textContent = s.configuring ? `The AI agent is drafting keys for ${s.configuring}...` : "";
}

/** The person's display choices as last saved (or being tried in setup). */
let displayState = { key_size: "medium", theme: "dark", turn: 0 };

/**
 * Draws the board's screen into `el`: the keys of `layout` (as the board shows them now) in the
 * chosen theme and way round, with the status bar. Portrait reflows to the 3 columns it has.
 */
function drawScreen(el, layout, look) {
  if (!el) return;
  const portrait = look.turn % 2 === 1;
  el.className = `screen ${["white", "sepia"].includes(look.theme) ? look.theme : ""} ${portrait ? "portrait" : ""}`;
  // The keys as the board shows them now (it already follows the key size); turned to portrait, the
  // same slots in at most 3 columns and 4 rows.
  const sizes = { big: { cols: 2, rows: 2 }, medium: { cols: 3, rows: 3 }, small: { cols: 4, rows: 4 } };
  const shown = (layout && layout.grid) || sizes[look.key_size] || sizes.medium;
  const cols = portrait ? Math.min(shown.cols, 3) : shown.cols;
  const rows = portrait ? Math.min(Math.ceil((shown.cols * shown.rows) / cols), 4) : shown.rows;
  const keys = document.createElement("div");
  keys.className = "keys";
  keys.style.fontFamily = fontStack(look.key_font);
  keys.style.gridTemplateColumns = `repeat(${cols}, 1fr)`;
  keys.style.gridTemplateRows = `repeat(${rows}, 1fr)`;
  const labels = layout ? [...layout.keys].sort((a, b) => a.key - b.key).map((k) => k.label) : [];
  for (let i = 0; i < cols * rows; i++) {
    const div = document.createElement("div");
    div.textContent = labels[i] || "";
    if (!labels[i]) div.className = "blank";
    keys.append(div);
  }
  const bar = document.createElement("div");
  bar.className = "bar";
  bar.textContent = layout ? `${layout.name}${layout.pages > 1 ? `   ${layout.page}/${layout.pages} >` : ""}` : "No board connected";
  el.replaceChildren(keys, bar);
}

function redrawScreens() {
  drawScreen($("keys-preview"), renderStatus.layout, displayState);
  drawScreen($("setup-preview"), renderStatus.layout, displayState);
}

async function loadDrafts() {
  const { drafts } = await api("/drafts");
  const list = $("drafts");
  list.replaceChildren();
  if (!drafts.length) {
    const li = document.createElement("li");
    li.className = "muted";
    li.textContent = "None yet. Tap Configure on the board for an app with no layout.";
    list.append(li);
  }
  for (const d of drafts) {
    const li = document.createElement("li");
    const name = document.createElement("strong");
    name.textContent = d.name;
    const sub = document.createElement("span");
    sub.className = "muted small";
    sub.textContent = ` ${d.keys} keys: ${d.labels.join(", ")}`;
    li.append(name, sub);
    list.append(li);
  }
}

// ---- events from the service ----

function connectEvents() {
  const source = new EventSource(`/api/events?token=${encodeURIComponent(TOKEN)}`);
  source.onmessage = (message) => {
    const event = JSON.parse(message.data);
    const d = event.data || {};
    if (event.kind === "status") renderStatus(d);
    if (event.kind === "flash") onFlash(d);
    if (event.kind === "provision") logTo($("pair-log"), d.text);
    if (event.kind === "configured") loadDrafts();
    if (event.kind === "app-setup") onAppSetup(d);
    if (event.kind === "edit") onEditEvent(d);
    if (event.kind === "edit.tap") onEditTap(d);
    if (event.kind === "noticed") onNoticed(d);
    if (event.kind === "display") { loadDisplay().catch(() => {}); if (setup.step === 4) keySizeStep(); }
    if (d.text && event.kind !== "status") logTo($("activity"), d.text.trim(), event.kind === "configure-failed" ? "error" : "");
  };
  source.onerror = () => {
    $("link-pill").textContent = "Service not reachable";
    $("link-pill").className = "pill warn";
  };
}

// ---- edit keys (#29) ----
// While editing, the service shows the library edited here on the board instead of following focus.
// Every change goes to the board at once, unsaved: the board itself is the preview. A tap on a board
// key selects it here. Save writes the person's own version on top of the recipe or AI draft.

// rev counts local changes; sent is the last one the service has (its answers to older ones are
// not shown over newer typing).
const edit = { state: null, selected: null, timer: null, recording: false, rev: 0, sent: 0, libraries: [] };

// The shortcut grammar (spec/protocol.md section 5; the names in companion/crates/core/src/keys.rs).
const MODIFIERS = ["Primary", "Ctrl", "Alt", "Shift", "Meta"];
const NAMED_KEYS = ["Enter", "Escape", "Backspace", "Tab", "Space", "Minus", "Equal", "BracketLeft", "BracketRight", "Backslash",
  "Semicolon", "Quote", "Backquote", "Comma", "Period", "Slash", "Insert", "Home", "PageUp", "Delete", "End", "PageDown",
  "Right", "Left", "Down", "Up"];
// What a key types alone, and with Shift (US layout, as the board's keyboard).
const TYPES = { Space: [" ", " "], Minus: ["-", "_"], Equal: ["=", "+"], BracketLeft: ["[", "{"], BracketRight: ["]", "}"],
  Backslash: ["\\", "|"], Semicolon: [";", ":"], Quote: ["'", '"'], Backquote: ["`", "~"], Comma: [",", "<"], Period: [".", ">"],
  Slash: ["/", "?"], 1: ["1", "!"], 2: ["2", "@"], 3: ["3", "#"], 4: ["4", "$"], 5: ["5", "%"], 6: ["6", "^"], 7: ["7", "&"],
  8: ["8", "*"], 9: ["9", "("], 0: ["0", ")"] };
const ON_MAC = /Mac/.test(navigator.platform || navigator.userAgent);

function keyName(text) {
  if (/^[a-z]$/i.test(text) || /^[0-9]$/.test(text)) return text.toUpperCase();
  const f = /^f([0-9]{1,2})$/i.exec(text);
  if (f && Number(f[1]) >= 1 && Number(f[1]) <= 24) return `F${Number(f[1])}`;
  return NAMED_KEYS.find((n) => n.toLowerCase() === text.toLowerCase()) || null;
}

/** "ctrl+shift+a" -> "Ctrl+Shift+A", with what it does on this computer, or what is wrong. */
function readShortcut(text) {
  const parts = text.split("+").map((p) => p.trim());
  const key = parts.pop();
  const held = [];
  for (const part of parts) {
    const name = MODIFIERS.find((m) => m.toLowerCase() === part.toLowerCase());
    if (!name) return { error: `"${part}" is not a held key: use Primary, Ctrl, Alt, Shift or Meta.` };
    if (!held.includes(name)) held.push(name);
  }
  const name = key ? keyName(key) : null;
  if (!name) return { error: key ? `"${key}" is not a key name: see Key names below.` : "Which key? For example Primary+S." };
  held.sort((a, b) => MODIFIERS.indexOf(a) - MODIFIERS.indexOf(b));
  const shown = { Primary: ON_MAC ? "⌘" : "Ctrl", Meta: ON_MAC ? "⌘" : "Super", Alt: ON_MAC ? "⌥" : "Alt", Ctrl: "Ctrl", Shift: "Shift" };
  const shift = held.includes("Shift");
  let says = `Presses ${[...held.map((m) => shown[m]), name].join(" ")} on this computer`;
  if (held.every((m) => m === "Shift")) {
    const typed = /^[A-Z]$/.test(name) ? (shift ? name : name.toLowerCase()) : TYPES[name] && TYPES[name][shift ? 1 : 0];
    if (typed) says += `: types ${typed === " " ? "a space" : typed}`;
  }
  return { keys: [...held, name].join("+"), says };
}

/** A key press on this computer, in the shortcut grammar (null for a held key alone). */
function recordedShortcut(e) {
  const code = e.code;
  let name = null;
  if (/^Key[A-Z]$/.test(code)) name = code.slice(3);
  else if (/^(Digit|Numpad)[0-9]$/.test(code)) name = code.slice(-1);
  else if (/^F[0-9]{1,2}$/.test(code)) name = keyName(code);
  else if (code.startsWith("Arrow")) name = code.slice(5);
  else if (code === "NumpadEnter") name = "Enter";
  else name = NAMED_KEYS.includes(code) ? code : null;
  if (!name) return null;
  const held = [];
  if (ON_MAC ? e.metaKey : e.ctrlKey) held.push("Primary");
  if (ON_MAC && e.ctrlKey) held.push("Ctrl");
  if (e.altKey) held.push("Alt");
  if (e.shiftKey) held.push("Shift");
  if (!ON_MAC && e.metaKey) held.push("Meta");
  return [...held, name].join("+");
}
const SOURCES = { builtin: "recipe", community: "community", ai: "AI draft", user: "yours" };

function showEditError(text) {
  $("edit-error").textContent = text;
  $("edit-error").hidden = !text;
}

async function editCommand(command, extra = {}) {
  // Save and page turns carry the last change with them; the others drop what is not sent yet.
  if (command === "save" || command === "page") await sendChange();
  else if (command !== "list") dropChange();
  showEditError("");
  try {
    const answer = await api("/edit", { command, ...extra });
    if (command !== "list") onEditState(answer);
    return answer;
  } catch (e) {
    showEditError(e.message);
    throw e;
  }
}

async function loadEdit() {
  const list = await editCommand("list").catch(() => null);
  if (!list) return;
  edit.libraries = list.libraries;
  const pick = $("edit-pick");
  pick.replaceChildren(new Option("What the board shows", ""));
  // Drafts a recipe replaced are never shown on the board (#38): listed last, apart.
  const old = (l) => l.source === "ai" && l.replacedBy;
  const groups = { app: ["Apps", (l) => l.kind === "app" && !old(l)], site: ["Sites", (l) => l.kind === "site" && !old(l)],
                   terminal: ["Programs in a terminal", (l) => l.kind === "terminal"],
                   old: ["Not in use (a recipe replaced them)", old] };
  for (const [title, wanted] of Object.values(groups)) {
    const group = document.createElement("optgroup");
    group.label = title;
    // The person's own versions are reached through what they layer on.
    const libs = list.libraries.filter((l) => wanted(l) && !l.overrides).sort((a, b) => a.name.localeCompare(b.name));
    for (const l of libs) {
      const own = list.libraries.find((o) => o.overrides === l.id);
      const label = own ? (own.replacedBy ? "yours, on an old draft" : "yours") : old(l) ? "old AI draft" : SOURCES[l.source] || l.source;
      group.append(new Option(`${l.name} (${label})`, l.id));
    }
    if (libs.length) pick.append(group);
  }
  pick.disabled = false;
  $("edit-icon").replaceChildren(new Option("(none)", ""), ...list.icons.map((name) => new Option(name, name)));
  onEditState(list.edit);
}

function onEditState(state) {
  edit.state = state && state.editing ? state : null;
  const on = !!edit.state;
  $("edit-toggle").textContent = on ? "Stop editing" : "Start editing";
  $("edit-main").hidden = !on;
  $("edit-dirty").hidden = !(on && edit.state.dirty);
  $("edit-old").hidden = true;
  if (!on) {
    edit.selected = null;
    // What the board shows when Start editing is pressed, never a library picked earlier.
    $("edit-pick").value = "";
    return;
  }
  const pack = edit.state.library.pack;
  $("edit-pick").value = pack.overrides || pack.id;
  $("edit-remove").hidden = !(edit.state.own && edit.state.overrides);
  $("edit-save").disabled = $("edit-revert").disabled = !edit.state.dirty;
  $("edit-saved").textContent = edit.state.own ? "Saved as your own version." : "Not saved yet: saving makes your own version, on top of this one.";
  const recipe = replacingRecipe();
  if (recipe) {
    $("edit-old-text").textContent = edit.state.own
      ? `Your version is on an old AI draft. The recipe "${recipe.name}" replaced that draft, but your version hides it on the board. Remove my version to use the recipe.`
      : `Not in use: the recipe "${recipe.name}" replaced this AI draft. Saving makes your version of the old draft, and the board then shows it instead of the recipe.`;
    $("edit-old").hidden = false;
  }
  if (edit.selected !== null && !edit.state.library.key[edit.selected]) edit.selected = null;
  renderEditPage();
  // Not while the person types in the key's form (it would move the cursor).
  if (!$("edit-key").contains(document.activeElement)) renderEditKey();
}

/** The session changed (here, in another tab, or a page turn on the board). */
function onEditEvent(state) {
  if (edit.rev !== edit.sent && state.editing && edit.state) {
    edit.state.page = state.page; // changes of ours are not with the service yet: keep them
    renderEditPage();
  } else {
    onEditState(state);
  }
}

/** A tap on the board selects that key here. */
function onEditTap({ page, slot }) {
  if (!edit.state) return;
  edit.state.page = page;
  const number = currentPage().number;
  const i = edit.state.library.key.findIndex((k) => k.page === number && k.slot === slot);
  edit.selected = i >= 0 ? i : null;
  renderEditPage();
  renderEditKey();
}

function currentPage() {
  const s = edit.state;
  return s.pages[Math.min(s.page, s.pages.length - 1)] || { number: 1, grid: "3x3" };
}

function gridOf(text) {
  const [cols, rows] = text.split("x").map(Number);
  return { cols, rows };
}

function renderEditPage() {
  const s = edit.state;
  const page = currentPage();
  const grid = gridOf(page.grid);
  $("edit-page").textContent = `Page ${Math.min(s.page, s.pages.length - 1) + 1} of ${Math.max(s.pages.length, 1)}`;
  $("edit-prev").disabled = $("edit-next").disabled = s.pages.length < 2;
  const sizes = [];
  for (let rows = 1; rows <= s.max.rows; rows++) for (let cols = 1; cols <= s.max.cols; cols++) sizes.push(`${cols}x${rows}`);
  $("edit-grid").replaceChildren(...sizes.map((g) => new Option(g, g)));
  $("edit-grid").value = page.grid;

  const keys = $("edit-keys");
  keys.replaceChildren();
  keys.style.gridTemplateColumns = `repeat(${grid.cols}, 1fr)`;
  keys.style.gridTemplateRows = `repeat(${grid.rows}, 1fr)`;
  for (let slot = 1; slot <= grid.cols * grid.rows; slot++) {
    const i = s.library.key.findIndex((k) => k.page === page.number && k.slot === slot);
    const div = document.createElement("div");
    if (i >= 0) {
      const key = s.library.key[i];
      if (key.icon) {
        // The picture the board draws (served by the service from the firmware's icons).
        const icon = document.createElement("img");
        icon.src = `/icons/${encodeURIComponent(key.icon)}.png`;
        icon.alt = "";
        icon.draggable = false;
        div.append(icon);
      }
      const label = document.createElement("span");
      label.textContent = key.label;
      div.append(label);
      div.draggable = true;
      if (edit.selected === i) div.classList.add("selected");
      div.addEventListener("click", () => { edit.selected = i; renderEditPage(); renderEditKey(); });
      div.addEventListener("dragstart", (e) => e.dataTransfer.setData("text/plain", String(slot)));
    } else {
      div.classList.add("blank");
    }
    div.addEventListener("dragover", (e) => { e.preventDefault(); div.classList.add("over"); });
    div.addEventListener("dragleave", () => div.classList.remove("over"));
    div.addEventListener("drop", (e) => {
      e.preventDefault();
      moveKey(page.number, Number(e.dataTransfer.getData("text/plain")), slot);
    });
    keys.append(div);
  }
}

function describeAction(action) {
  if (action.kind === "sequence") return `types ${action.keys.join(", then ")}`;
  const detail = action.url || action.selector || action.name || action.command || action.reply || "";
  return detail ? `${action.kind} ${detail}` : action.kind;
}

function renderEditKey() {
  stopRecording();
  const key = edit.state && edit.selected !== null ? edit.state.library.key[edit.selected] : null;
  $("edit-none").hidden = !!key;
  $("edit-key").hidden = !key;
  if (!key) return;
  $("edit-label").value = key.label;
  $("edit-count").textContent = `${key.label.length}/24`;
  $("edit-icon").value = key.icon || "";
  $("edit-speak").value = key.speak || "";
  $("edit-confirm").checked = !!key.confirm;
  const shortcut = key.action.kind === "shortcut";
  $("edit-keys-row").hidden = !shortcut;
  $("edit-action").hidden = shortcut;
  if (shortcut) {
    $("edit-shortcut").value = key.action.keys;
    showShortcut(key.action.keys);
  } else {
    $("edit-action").textContent = `Does: ${describeAction(key.action)} (not editable here yet)`;
  }
}

function showShortcut(text) {
  const read = readShortcut(text);
  $("edit-shortcut-says").textContent = read.error || read.says;
  $("edit-shortcut-says").className = `small ${read.error ? "error" : "muted"}`;
  return read;
}

function stopRecording() {
  edit.recording = false;
  $("edit-record").classList.remove("recording");
  $("edit-record").textContent = "Record";
}

/** A local change: shown here at once, sent to the board a moment later (typing sends once). */
function changed() {
  edit.state.dirty = true;
  $("edit-dirty").hidden = false;
  $("edit-save").disabled = $("edit-revert").disabled = false;
  renderEditPage();
  edit.rev++;
  clearTimeout(edit.timer);
  edit.timer = setTimeout(sendChange, 250);
}

/** Sends the library as edited now, if the service does not have it yet. */
async function sendChange() {
  clearTimeout(edit.timer);
  edit.timer = null;
  if (!edit.state || edit.sent === edit.rev) return;
  const rev = edit.rev;
  try {
    const answer = await api("/edit", { command: "change", library: edit.state.library });
    edit.sent = rev;
    showEditError("");
    if (edit.rev === rev) onEditState(answer); // not over typing that came after
  } catch (e) {
    edit.sent = rev; // sent again with the next change
    showEditError(e.message);
  }
}

function dropChange() {
  clearTimeout(edit.timer);
  edit.timer = null;
  edit.sent = edit.rev;
}

function editKey(apply) {
  const key = edit.state && edit.state.library.key[edit.selected];
  if (!key) return;
  apply(key);
  $("edit-count").textContent = `${key.label.length}/24`;
  changed();
}

function moveKey(number, from, to) {
  if (from === to) return;
  const keys = edit.state.library.key;
  const a = keys.find((k) => k.page === number && k.slot === from);
  const b = keys.find((k) => k.page === number && k.slot === to);
  if (!a) return;
  a.slot = to;
  if (b) b.slot = from;
  changed();
}

$("edit-toggle").addEventListener("click", () => {
  if (edit.state) editCommand("stop").catch(() => {});
  else editCommand("start", $("edit-pick").value ? { id: $("edit-pick").value } : {}).catch(() => {});
});
$("edit-pick").addEventListener("change", (e) => {
  if (!e.target.value) return;
  if (edit.state && edit.state.dirty && !confirm("Leave your unsaved changes?")) {
    const pack = edit.state.library.pack;
    e.target.value = pack.overrides || pack.id;
    return;
  }
  edit.selected = null;
  editCommand("start", { id: e.target.value }).catch(() => {});
});
$("edit-prev").addEventListener("click", () => editCommand("page", { page: (edit.state.page + edit.state.pages.length - 1) % edit.state.pages.length }).catch(() => {}));
$("edit-next").addEventListener("click", () => editCommand("page", { page: (edit.state.page + 1) % edit.state.pages.length }).catch(() => {}));
$("edit-grid").addEventListener("change", (e) => {
  const lib = edit.state.library;
  const page = currentPage();
  const grid = gridOf(e.target.value);
  const size = grid.cols * grid.rows;
  const onPage = lib.key.filter((k) => k.page === page.number);
  if (onPage.length > size) {
    showEditError(`This page has ${onPage.length} keys: remove some first, or pick a bigger grid.`);
    e.target.value = page.grid;
    return;
  }
  // Keys past the smaller grid move into its free places.
  const used = new Set(onPage.filter((k) => k.slot <= size).map((k) => k.slot));
  for (const k of onPage.filter((k) => k.slot > size)) {
    let slot = 1;
    while (used.has(slot)) slot++;
    k.slot = slot;
    used.add(slot);
  }
  lib.page = (lib.page || []).filter((p) => p.number !== page.number);
  lib.page.push({ number: page.number, grid: e.target.value });
  page.grid = e.target.value;
  changed();
});
$("edit-add").addEventListener("click", () => {
  const page = currentPage();
  const grid = gridOf(page.grid);
  const used = new Set(edit.state.library.key.filter((k) => k.page === page.number).map((k) => k.slot));
  let slot = 1;
  while (used.has(slot)) slot++;
  if (slot > grid.cols * grid.rows) {
    showEditError("This page is full: pick a bigger grid first.");
    return;
  }
  edit.state.library.key.push({ page: page.number, slot, label: "New key", action: { kind: "shortcut", keys: "Escape" } });
  edit.selected = edit.state.library.key.length - 1;
  renderEditKey();
  changed();
});
$("edit-delete").addEventListener("click", () => {
  edit.state.library.key.splice(edit.selected, 1);
  edit.selected = null;
  renderEditKey();
  changed();
});
$("edit-label").addEventListener("input", (e) => editKey((k) => { k.label = e.target.value; }));
$("edit-icon").addEventListener("change", (e) => editKey((k) => { if (e.target.value) k.icon = e.target.value; else delete k.icon; }));
$("edit-speak").addEventListener("input", (e) => editKey((k) => { if (e.target.value) k.speak = e.target.value; else delete k.speak; }));
$("edit-confirm").addEventListener("change", (e) => editKey((k) => { k.confirm = e.target.checked; }));
$("edit-shortcut").addEventListener("input", (e) => {
  const read = showShortcut(e.target.value);
  if (read.keys) editKey((k) => { k.action.keys = read.keys; }); // lowercase and spaces are fine: sent as key names
});
$("edit-shortcut").addEventListener("change", (e) => {
  const read = readShortcut(e.target.value);
  if (read.keys) e.target.value = read.keys;
});
$("edit-record").addEventListener("click", () => {
  if (edit.recording) return stopRecording();
  edit.recording = true;
  $("edit-record").classList.add("recording");
  $("edit-record").textContent = "Press the keys...";
  $("edit-shortcut-says").textContent = "Press the keys now (click Record again to cancel).";
});
document.addEventListener("keydown", (e) => {
  if (!edit.recording) return;
  const keys = recordedShortcut(e);
  if (!keys) return; // a held key alone: wait for the rest
  e.preventDefault();
  e.stopPropagation();
  stopRecording();
  $("edit-shortcut").value = keys;
  showShortcut(keys);
  editKey((k) => { k.action.keys = keys; });
}, true);
$("edit-key-names").textContent = NAMED_KEYS.join(", ");
$("edit-key").addEventListener("submit", (e) => e.preventDefault());
/** The recipe that replaced the draft being edited (or the draft under the person's version), if any. */
function replacingRecipe() {
  if (!edit.state) return null;
  const pack = edit.state.library.pack;
  // A version saved in another tab is not in this tab's list yet: the draft under it is.
  const lib = edit.libraries.find((l) => l.id === pack.id) || edit.libraries.find((l) => l.id === pack.overrides);
  return lib ? lib.replacedBy : null;
}

$("edit-save").addEventListener("click", () => {
  const recipe = replacingRecipe();
  if (recipe && !edit.state.own && !confirm(`The recipe "${recipe.name}" replaced this AI draft. Save your version of the old draft anyway? The board will show it instead of the recipe.`)) return;
  editCommand("save").then(loadEdit).catch(() => {});
});
$("edit-use-recipe").addEventListener("click", () => {
  const recipe = replacingRecipe();
  if (!recipe || (edit.state.dirty && !confirm("Leave your unsaved changes?"))) return;
  edit.selected = null;
  editCommand("start", { id: recipe.id }).catch(() => {});
});
$("edit-revert").addEventListener("click", () => editCommand("revert").catch(() => {}));
$("edit-remove").addEventListener("click", () => {
  if (confirm("Remove your version? The keys go back to the recipe or AI draft underneath.")) editCommand("remove").then(loadEdit).catch(() => {});
});

// ---- setup wizard ----

const setup = { step: 1, port: null, board: null, firmware: null, identified: null, mode: null, task: "setup", usbPaired: false };

/**
 * The wizard does two jobs: set up a board (find, firmware, install, keys and look over USB,
 * optional Wi-Fi, optional Bluetooth), or move a board that is already set up to another Wi-Fi
 * network (find, Wi-Fi): over USB, keeping its pairings, so it works when the board cannot reach
 * its old network.
 */
function setTask(task) {
  setup.task = task;
  const wifi = task === "wifi";
  // Firmware, Install, Look and Bluetooth are not part of changing Wi-Fi (step() rewrites the class names, so by number).
  document.querySelectorAll('.steps li[data-step="2"], .steps li[data-step="3"], .steps li[data-step="4"], .steps li[data-step="6"]').forEach((el) => (el.hidden = wifi));
  $("mode-card").hidden = wifi;
  $("step5-name").textContent = wifi ? "Wi-Fi" : "Wi-Fi (optional)";
  $("step5-title").textContent = wifi ? "Change Wi-Fi" : "Wi-Fi (optional)";
  $("step5-intro").textContent = wifi
    ? "Pick the network the board should use from now on. Its pairing with this computer and its settings stay as they are; the password goes to the board over the cable only and is not stored here."
    : "Wi-Fi lets the board work wirelessly anywhere on your 2.4 GHz network, away from the USB cable. Your password goes to the board over the USB cable only and is never stored here. If you keep the board plugged into this computer over USB, you can skip this step.";
  $("step5-submit").textContent = wifi ? "Change Wi-Fi" : "Join Wi-Fi";
  $("step5-back").dataset.back = wifi ? "1" : "4";
  $("skip-wifi").hidden = wifi;
  $("identify").textContent = wifi ? "Next" : "Identify";
  $("pair-log").replaceChildren();
  $("pair-done").hidden = true;
}

$("change-wifi").addEventListener("click", () => {
  setTask("wifi");
  step(1);
  show("setup");
});

$("dashboard-bluetooth").addEventListener("click", () => {
  openBluetoothSettings().catch(() => {});
});

/** A board that is already set up and connected: the setup page offers what it may still need
 *  (apps, look, Wi-Fi, Bluetooth, reinstalling) instead of starting from "Find the board". */
function renderKnown() {
  const s = renderStatus.last;
  const board = s && s.link === "connected" ? s.board : null;
  const known = !!board && !setup.picked && setup.task === "setup";
  $("known-board").hidden = !known;
  const flow = document.querySelector(".setup-flow");
  if (flow) flow.hidden = known;
  // Came here from "Your board is set up": a way back to it, and Back on the apps step goes there too.
  const fromKnown = !!board && setup.picked;
  $("known-back").hidden = !fromKnown;
  if (fromKnown) delete $("apps-back").dataset.back; else $("apps-back").dataset.back = "6";
  if (!board) return;
  const wifi = !board.address.startsWith("USB");
  $("known-text").textContent = `${board.profile} with firmware ${board.firmware}, connected ${wifi ? "over Wi-Fi" : "by USB"}. Pick what you'd like to do next.`;
  $("known-wifi-title").textContent = wifi ? "Change Wi-Fi" : "Wi-Fi";
  $("known-wifi").textContent = wifi ? "Move the board to another network (over the USB cable)" : "Use the board wirelessly, away from the cable (set over USB)";
  const waiting = apps.list.filter((a) => a.state === "none" && a.minutes > 0).length;
  if (apps.list.length) $("known-apps").textContent = waiting ? `${waiting} app${waiting === 1 ? "" : "s"} you use have no keys yet: the AI can draft them` : "Every app you use has keys; add more";
}
function backToBoard() {
  setup.picked = false;
  setTask("setup");
  step(1);
  renderKnown();
  $("known-board").scrollIntoView({ block: "start" });
}
$("known-back").addEventListener("click", backToBoard);
$("apps-back").addEventListener("click", (e) => {
  if ($("apps-back").dataset.back) return; // the usual step back
  e.stopPropagation(); // not also the page's data-back handler, once backToBoard has restored it
  backToBoard();
});
document.addEventListener("click", (e) => {
  if (e.target.closest("[data-known-back]")) { e.preventDefault(); backToBoard(); }
});

document.querySelectorAll("[data-known]").forEach((b) => b.addEventListener("click", () => {
  setup.picked = true;
  const go = {
    apps: () => openApps(),
    look: () => { setTask("setup"); keySizeStep(); },
    // Joining or changing Wi-Fi goes over the USB cable: find the port, then the network.
    wifi: () => { setTask("wifi"); step(1); scanPorts(); },
    bluetooth: () => { setTask("setup"); step(6); },
    install: () => { setTask("setup"); step(1); scanPorts(); },
  }[b.dataset.known];
  renderKnown();
  go();
  document.querySelector(".setup-flow").scrollIntoView({ block: "start" });
}));

function step(n) {
  setup.step = n;
  const flow = document.querySelector(".setup-flow");
  if (flow) flow.dataset.step = n;
  document.querySelectorAll(".step").forEach((s) => (s.hidden = Number(s.dataset.step) !== n));
  document.querySelectorAll(".steps li").forEach((li) => {
    const i = Number(li.dataset.step);
    li.className = i === n ? "current" : i < n ? "done" : "";
  });
}

function choice(name, value, title, sub, disabled) {
  const li = document.createElement("li");
  const label = document.createElement("label");
  if (disabled) label.className = "disabled";
  const input = document.createElement("input");
  input.type = "radio";
  input.name = name;
  input.value = value;
  input.disabled = !!disabled;
  const text = document.createElement("span");
  const strong = document.createElement("strong");
  strong.textContent = title;
  const small = document.createElement("span");
  small.className = "sub";
  small.textContent = sub;
  text.append(strong, small);
  label.append(input, text);
  li.append(label);
  return { li, input };
}

// ---- key size (display.rs): how many keys a page may have at most ----

const KEY_SIZES = [
  ["big", "Big: 2x2", "Four large keys a page. Easiest to read and tap; most apps need several pages."],
  ["medium", "Medium: 3x3 (recommended)", "Nine keys a page. Comfortable on a 2.8-inch screen."],
  ["small", "Small: 4x4", "Sixteen keys a page. Short labels, or icons only; fewer pages to swipe."],
];

const THEMES = [
  ["dark", "Dark, in colour", "Light text on dark keys; colour shows a pressed key, a key waiting for its second tap, and messages."],
  ["white", "White: black on white", "An e-ink look with high contrast, easy to read in daylight. App pictures turn grey."],
  ["sepia", "Sepia: brown ink on warm paper", "An e-ink look, softer on the eyes than white. App pictures turn sepia."],
];

const TURNS = [
  ["0", "Landscape", "Wide, as the board comes."],
  ["1", "Portrait", "Tall: the board turned a quarter clockwise. Three keys across at most."],
  ["2", "Landscape, upside down", "Wide, turned half way round: for a cable that has to come out of the other side."],
  ["3", "Portrait, upside down", "Tall, turned a quarter anticlockwise."],
];

// ---- fonts from Google Fonts: the list live from the service, previews loaded by the browser ----

let fontFamilies = null;

/** Fills the font search list from Google's catalogue (once per page). */
async function loadFontFamilies() {
  if (fontFamilies) return;
  fontFamilies = [];
  try {
    ({ families: fontFamilies } = await api("/fonts"));
    $("font-families").replaceChildren(...fontFamilies.map((f) => new Option(f.category ? `${f.family} (${f.category})` : f.family, f.family)));
  } catch (err) {
    fontFamilies = null;
    logTo($("activity"), `Could not load the list of Google Fonts: ${err.message}`, "error");
  }
}

/** Lets the browser draw `family` (for the previews), with the characters the board shows. */
function previewFont(family) {
  if (!family || document.querySelector(`link[data-font="${CSS.escape(family)}"]`)) return;
  const link = document.createElement("link");
  link.rel = "stylesheet";
  link.dataset.font = family;
  link.href = `https://fonts.googleapis.com/css2?family=${encodeURIComponent(family)}&display=swap`;
  document.head.append(link);
}

function fontStack(family) {
  return family ? `"${family}", Montserrat, sans-serif` : "";
}

/** The font fields and the start-up previews, from the saved choice. */
function showFonts(d) {
  for (const id of ["key-font", "key-font-setup"]) if ($(id) && document.activeElement !== $(id)) $(id).value = d.key_font || "";
  for (const id of ["start-font", "start-font-setup"]) if ($(id) && document.activeElement !== $(id)) $(id).value = d.start_font || "";
  previewFont(d.key_font);
  previewFont(d.start_font);
  for (const id of ["start-size", "start-size-setup"]) if ($(id) && document.activeElement !== $(id)) $(id).value = d.start_size || 64;
  // The preview at the board's size, scaled as the screen previews are (320 px wide shown about 360).
  for (const id of ["start-preview", "start-preview-setup"]) {
    if (!$(id)) continue;
    $(id).style.fontFamily = fontStack(d.start_font);
    $(id).style.fontSize = `${Math.round((d.start_size || 64) * 1.1)}px`;
  }
}

for (const id of ["start-size", "start-size-setup"]) {
  $(id).addEventListener("change", (e) => {
    const size = Number(e.target.value);
    if (!Number.isInteger(size) || size < 24 || size > 96) { e.target.reportValidity(); return; }
    saveDisplay({ start_size: size }).catch((err) => alert(`Could not change the size: ${err.message}`));
  });
}

for (const [id, field] of [["key-font", "key_font"], ["key-font-setup", "key_font"], ["start-font", "start_font"], ["start-font-setup", "start_font"]]) {
  $(id).addEventListener("focus", loadFontFamilies);
  $(id).addEventListener("change", (e) => {
    const family = e.target.value.trim();
    if (family && fontFamilies && !fontFamilies.some((f) => f.family === family)) {
      e.target.setCustomValidity("Pick a family from the list");
      e.target.reportValidity();
      return;
    }
    e.target.setCustomValidity("");
    saveDisplay({ [field]: family }).catch((err) => alert(`Could not change the font: ${err.message}`));
  });
}

async function saveDisplay(change) {
  displayState = { ...displayState, ...change };
  redrawScreens();
  const saved = await api("/display", change);
  displayState = { ...displayState, ...saved };
  $("key-size").value = saved.key_size;
  $("theme").value = saved.theme;
  $("turn").value = String(saved.turn);
  showFonts(saved);
  redrawScreens();
  return saved;
}

/** Fills the dashboard's choices; the look only when the board's firmware can change it. */
async function loadDisplay() {
  const d = await api("/display");
  displayState = { key_size: d.key_size, theme: d.theme, turn: d.turn, key_font: d.key_font, start_font: d.start_font };
  showFonts(d);
  redrawScreens();
  $("key-size").value = d.key_size;
  $("theme").value = d.theme;
  $("turn").value = String(d.turn);
  // Kept and shown in the preview either way; the board takes it once its firmware can.
  $("theme").title = d.board_can_change_look ? "" : "This board's firmware has one look only: the preview shows it, and the board takes it after a firmware update.";
  return d;
}

/** Setup step 4: keys and look, available right after USB install/identify (before Wi-Fi or Bluetooth). */
async function keySizeStep() {
  step(4);
  $("setup-done").hidden = true;
  const current = await api("/display").catch(() => ({ key_size: "medium", theme: "dark", board_can_change_look: false }));
  const fill = (list, name, choices, value, disabled) => {
    list.replaceChildren();
    for (const [v, title, sub] of choices) {
      const { li, input } = choice(name, v, title, sub, disabled && v !== value);
      input.checked = v === value;
      list.append(li);
    }
  };
  fill($("key-size-setup"), "key-size-choice", KEY_SIZES, current.key_size);
  // The preview shows every look; the board follows when its firmware can.
  fill($("theme-setup"), "theme-choice", THEMES, current.theme);
  fill($("turn-setup"), "turn-choice", TURNS, String(current.turn || 0));
  const canLook = !!(current.board_can_change_look || setup.justInstalled || setup.identified?.running?.hasDisplay);
  $("theme-note").hidden = canLook;
  displayState = { key_size: current.key_size, theme: current.theme, turn: current.turn || 0, key_font: current.key_font, start_font: current.start_font };
  showFonts(current);
  redrawScreens();
  // Each choice applies at once: on the board (over USB or Wi-Fi), and in the preview.
  const field = { "key-size-choice": (v) => ({ key_size: v }), "theme-choice": (v) => ({ theme: v }), "turn-choice": (v) => ({ turn: Number(v) }) };
  for (const name of Object.keys(field)) {
    document.querySelectorAll(`input[name="${name}"]`).forEach((input) => input.addEventListener("change", () => {
      const change = field[name](input.value);
      saveDisplay(change).catch((err) => logTo($("activity"), `Could not apply that: ${err.message}`, "error"));
    }));
  }
}

// Every choice is applied as it is made; the buttons let the person finish over USB or continue to optional Wi-Fi / Bluetooth.
$("key-size-save").addEventListener("click", () => { $("setup-done").hidden = false; });
$("to-wifi").addEventListener("click", () => { step(5); scanWifi(); });
$("to-bluetooth").addEventListener("click", () => bluetoothStep());

$("key-size").addEventListener("change", (e) => saveDisplay({ key_size: e.target.value }).catch((err) => alert(`Could not save the key size: ${err.message}`)));
$("theme").addEventListener("change", (e) => saveDisplay({ theme: e.target.value }).catch((err) => alert(`Could not change the look: ${err.message}`)));
$("turn").addEventListener("change", (e) => saveDisplay({ turn: Number(e.target.value) }).catch((err) => alert(`Could not turn the screen: ${err.message}`)));
loadDisplay().catch(() => {});

/** What the last port scan saw, so watching only redraws (and asks the assistant) on a change. */
let lastPorts = null;

/** Lists the ports. `watching`: a background re-check; does nothing unless something changed, and
 *  carries on by itself when a board turns up. */
async function scanPorts(watching = false) {
  const list = $("ports");
  try {
    const { ports, no_port: noPort = [], os, wch_driver: wch } = await api("/ports");
    const seen = JSON.stringify([ports.map((p) => p.name), noPort.map((d) => d.usb_id), wch]);
    if (watching && seen === lastPorts) return;
    const appeared = watching && ports.length && lastPorts !== null;
    lastPorts = seen;
    list.replaceChildren();
    $("identify").disabled = true;
    setup.port = null;
    if (!ports.length) {
      const li = document.createElement("li");
      li.className = "muted";
      if (noPort.length) {
        li.textContent = noPortAdvice(noPort[0], os, wch);
        if (os === "macos" && noPort[0].bridge === "ch340") {
          const buttons = document.createElement("div");
          buttons.className = "no-port-actions";
          const installed = wch && wch !== "not installed";
          for (const [label, target] of installed ? [["Open Driver Extensions", "driver_settings"]]
                                                  : [["Install the USB driver", "usb_driver"], ["Open Driver Extensions", "driver_settings"]]) {
            const b = document.createElement("button");
            b.className = "secondary";
            b.textContent = label;
            b.addEventListener("click", async () => { li.firstChild.textContent = await openOnMac(target); });
            buttons.append(b);
          }
          li.replaceChildren(document.createTextNode(li.textContent), buttons);
        }
        autoAssist(`The board's USB chip (${noPort[0].usb_id}, ${noPort[0].bridge}) is plugged in but has no serial port: no driver claimed it.`);
      } else {
        li.textContent = "No board found. Check the cable carries data (not charge-only) and the board is powered, then look again.";
        autoAssist("No USB serial port, and no USB-serial chip on the USB bus.");
      }
      list.append(li);
    }
    for (const p of ports) {
      const usb = `${p.vid.toString(16).padStart(4, "0")}:${p.pid.toString(16).padStart(4, "0")}`;
      const { li, input } = choice("port", p.name, p.name, `${p.product || p.bridge} - USB ${usb}`);
      input.addEventListener("change", () => { setup.port = p.name; $("identify").disabled = false; });
      list.append(li);
      if (ports.length === 1) { input.checked = true; setup.port = p.name; $("identify").disabled = false; }
    }
    // Found while watching (a cable, a driver, a switch): carry on without being asked.
    if (appeared && setup.port) {
      const note = document.createElement("li");
      note.className = "note success";
      note.textContent = `Found the board on ${setup.port}.${setup.mode === "ai" ? " Identifying it now..." : " Press Identify to continue."}`;
      list.prepend(note);
      $("advice").hidden = true;
      if (setup.mode === "ai") $("identify").click();
    }
  } catch (e) {
    if (watching) return;
    list.innerHTML = "";
    logTo($("activity"), `Could not list ports: ${e.message}`, "error");
  }
}
$("rescan").addEventListener("click", () => scanPorts());

// While the wizard waits for a board, look again every few seconds.
setInterval(() => {
  if (setup.step === 1 && !setup.port && !$("view-setup").hidden && document.visibilityState === "visible") scanPorts(true);
}, 4000);
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible" && setup.step === 1 && !setup.port && !$("view-setup").hidden) scanPorts(true);
});

/** Why a USB-serial chip that is plugged in has no port, from what the computer reports. */
function noPortAdvice(d, os, wch) {
  const chip = `The board is plugged in (USB ${d.usb_id}, ${d.name || d.bridge}), but no driver has given it a port.`;
  if (os === "macos" && d.bridge === "ch340") {
    if (!wch || wch === "not installed") return `${chip} This chip needs WCH's driver on a Mac: install it (macOS asks for your password), then switch it on under Driver Extensions.`;
    if (!/enabled/.test(wch)) return `${chip} WCH's driver is installed but not switched on (${wch}): switch on CH34xVCPDriver under Driver Extensions, then unplug and replug the board.`;
    return `${chip} WCH's driver is on; unplug the board, plug it in again, and look again.`;
  }
  if (os === "linux") {
    if ((d.drivers || []).includes("usbfs")) return `${chip} Another program holds it (often brltty, the Braille display service). Remove brltty or stop it, then unplug and replug the board.`;
    if (!(d.drivers || []).length) return `${chip} The kernel's driver for it (${d.bridge === "ch340" ? "ch341" : d.bridge === "cp210x" ? "cp210x" : "cdc_acm"}) is not loaded: try "sudo modprobe ${d.bridge === "ch340" ? "ch341" : d.bridge === "cp210x" ? "cp210x" : "cdc_acm"}", then unplug and replug.`;
  }
  if (os === "windows") {
    if (d.bridge === "ch340") return `${chip} Install the WCH CH34x driver (Windows Settings > Windows Update > Advanced options > Optional updates > Driver updates, or WCH's CH341SER installer), then unplug and replug the board.`;
    if (d.bridge === "cp210x") return `${chip} Install the Silicon Labs CP210x driver (Windows Update > Optional updates > Driver updates, or from Silicon Labs), then unplug and replug the board.`;
  }
  return `${chip} Unplug it, plug it in again, and look again; the assistant can help find out why.`;
}

$("identify").addEventListener("click", async () => {
  if (setup.task === "wifi") { step(5); scanWifi(); return; }
  const status = $("identify-status");
  status.hidden = false;
  status.className = "note";
  status.textContent = "Asking the board who it is (this restarts it for a moment)...";
  $("identify").disabled = true;
  try {
    const result = await api("/identify", { port: setup.port });
    const [firmware] = await Promise.all([api("/firmware").catch(() => null)]);
    setup.firmware = firmware;
    setup.justInstalled = false;
    setup.usbPaired = false;
    $("fw-source").textContent = firmware ? `${firmware.source} (version ${firmware.manifest.version})` : "unavailable";
    setup.identified = {
      chip: result.chip,
      chipError: result.chipError,
      running: result.running ? { firmware: result.running.firmware, profile: result.running.board?.profile, hasDisplay: !!result.running.board?.display } : null,
      candidates: result.candidates,
    };
    const chip = result.chip;
    const running = result.running;
    $("chip-summary").textContent = [
      chip ? `${chip.chip} ${chip.revision || ""}, ${chip.flash_size} flash, MAC ${chip.mac || "?"}` : `Chip not identified: ${result.chipError}`,
      running ? `Running KnoEnd ${running.firmware} (${running.board.profile}).` : "Not running KnoEnd yet.",
    ].join(" ");
    $("erase").checked = !running;
    $("skip-install").hidden = !running;
    renderCandidates(result.candidates);
    status.hidden = true;
    step(2);
  } catch (e) {
    status.className = "note error";
    status.textContent = `Could not identify the board: ${e.message}`;
    autoAssist(`Identify failed: ${e.message}`);
  } finally {
    $("identify").disabled = false;
  }
});

/** Pairs the board over USB (without requiring Wi-Fi) so the companion can talk to it immediately. */
async function pairOverUsb(logList) {
  if (!setup.port || setup.usbPaired) return true;
  try {
    const result = await api("/provision", { port: setup.port, ssid: null, password: null });
    setup.usbPaired = true;
    if (logList) logTo(logList, `Paired with board ${result.board} over USB.`);
    return true;
  } catch (e) {
    if (logList) logTo(logList, `Could not pair over USB yet: ${e.message}`, "error");
    return false;
  }
}

$("skip-install").addEventListener("click", async () => {
  $("skip-install").disabled = true;
  try {
    await pairOverUsb($("activity"));
    keySizeStep();
  } finally {
    $("skip-install").disabled = false;
  }
});

function renderCandidates(candidates) {
  const list = $("candidates");
  list.replaceChildren();
  setup.board = null;
  $("install").disabled = true;
  if (!candidates.length) {
    const li = document.createElement("li");
    li.className = "muted";
    li.textContent = "No known board matches this hardware yet.";
    list.append(li);
  }
  // Boards that look the same over USB (native-USB S3 boards all do): let the person choose.
  const installable = candidates.filter((c) => c.status === "supported" || c.status === "unverified");
  const tie = installable.length > 1 && installable[0].score === installable[1].score;
  if (tie) {
    const li = document.createElement("li");
    li.className = "note";
    li.textContent = "These boards look identical over USB. Pick the one you have (its name is on the box or the back of the board).";
    list.append(li);
  }
  let picked = tie;
  candidates.forEach((c) => {
    const installable = c.status === "supported" || c.status === "unverified";
    const prefix = c.status === "unverified"
      ? "Unverified: written from the maker's documentation, not yet tested on this board. If the screen or touch is wrong, tell the assistant. "
      : c.status === "supported" ? "" : "Firmware coming soon. ";
    const { li, input } = choice("board", c.board, c.name, prefix + c.reasons.join("; "), !installable);
    input.addEventListener("change", () => { setup.board = c.board; $("install").disabled = false; });
    list.append(li);
    if (!picked && installable) { picked = true; input.checked = true; setup.board = c.board; $("install").disabled = false; }
  });
}

$("install").addEventListener("click", async () => {
  step(3);
  $("bar").style.width = "0";
  $("flash-log").replaceChildren();
  $("to-pair").disabled = true;
  $("flash-back").hidden = true;
  $("flash-stage").className = "note";
  $("flash-stage").textContent = "Downloading and checking the firmware...";
  try {
    $("to-pair").textContent = "Next: keys and look";
    await api("/flash", { port: setup.port, board: setup.board, erase: $("erase").checked });
  } catch (e) {
    onFlash({ stage: "failed", error: e.message });
  }
});

function onFlash(p) {
  if (setup.step !== 3) return;
  const stage = $("flash-stage");
  if (p.stage === "writing") {
    const overall = ((p.part - 1) + (p.total ? p.done / p.total : 1)) / p.parts;
    $("bar").style.width = `${Math.round(overall * 100)}%`;
    stage.textContent = `Writing part ${p.part} of ${p.parts}`;
  } else if (p.stage === "connecting") {
    stage.textContent = "Connecting to the board's bootloader...";
  } else if (p.stage === "erasing") {
    stage.textContent = "Erasing the board (can take 20-60 seconds)...";
  } else if (p.stage === "verifying") {
    stage.textContent = `Verifying part ${p.part}`;
  } else if (p.stage === "finished") {
    $("bar").style.width = "100%";
    stage.className = "note success";
    stage.textContent = `Installed KnoEnd ${p.version}. Pairing over USB...`;
    setup.justInstalled = true;
    afterInstallFinished(p.version);
  } else if (p.stage === "failed") {
    stage.className = "note error";
    stage.textContent = `Install failed: ${p.error}`;
    $("flash-back").hidden = false;
    autoAssist(`Install failed: ${p.error}`);
  }
  if (p.text) logTo($("flash-log"), p.text);
}

$("to-pair").addEventListener("click", () => keySizeStep());

/**
 * Right after installing: pairs the board over USB so Keys and look works immediately over the USB
 * cable, whether or not the person sets up Wi-Fi later.
 */
async function afterInstallFinished(version) {
  const button = $("to-pair");
  const stage = $("flash-stage");
  const paired = await pairOverUsb($("flash-log"));
  stage.textContent = paired
    ? `Installed KnoEnd ${version} and paired over USB. Next: choose your key size and look.`
    : `Installed KnoEnd ${version}. The board restarted.`;
  button.disabled = false;
}

// ---- Wi-Fi networks, as the board hears them ----

let chosenNetwork = null; // { ssid, secured } or "other"

/** Signal strength as four bars. */
function signal(rssi) {
  const level = rssi >= -55 ? 4 : rssi >= -65 ? 3 : rssi >= -75 ? 2 : 1;
  const el = document.createElement("span");
  el.className = "signal";
  el.setAttribute("aria-label", `signal ${level} of 4`);
  for (let i = 1; i <= 4; i++) {
    const bar = document.createElement("i");
    if (i <= level) bar.className = "on";
    el.append(bar);
  }
  return el;
}

const LOCK = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 7V5a4 4 0 1 1 8 0v2h1v8H3V7h1zm2 0h4V5a2 2 0 1 0-4 0v2z"/></svg>';

/** One row of the network list. */
function networkRow(title, { rssi, channel, secured, other } = {}) {
  const li = document.createElement("li");
  if (other) li.className = "other";
  const label = document.createElement("label");
  const input = document.createElement("input");
  input.type = "radio";
  input.name = "network";
  const name = document.createElement("span");
  name.className = "name";
  name.textContent = title;
  label.append(input, name);
  if (!other) {
    const strength = signal(rssi);
    strength.title = `${rssi} dBm, channel ${channel}`;
    const security = document.createElement("span");
    security.className = "security";
    if (secured) { security.innerHTML = LOCK; security.title = "secured"; } else security.textContent = "open";
    label.append(strength, security);
  }
  li.append(label);
  return { li, input };
}

function pickNetwork(choice) {
  chosenNetwork = choice;
  const other = choice === "other";
  $("ssid-wrap").hidden = !other;
  $("password-wrap").hidden = !other && choice && !choice.secured;
  if (!other && choice) $("ssid").value = choice.ssid;
  if (other) { $("ssid").value = ""; $("ssid").focus(); }
}

async function scanWifi() {
  const list = $("networks");
  const button = $("scan-wifi");
  list.replaceChildren();
  button.disabled = true;
  $("wifi-note").textContent = "Asking the board to look for networks (about 5 seconds)...";
  try {
    const { networks: heard = [], error } = await api("/wifi/scan", { port: setup.port });
    // Strongest first, each name once (mesh systems and extenders repeat names).
    const best = new Map();
    for (const n of heard) {
      if (n.ssid && (!best.has(n.ssid) || best.get(n.ssid).rssi < n.rssi)) best.set(n.ssid, n);
    }
    const networks = [...best.values()].sort((a, b) => b.rssi - a.rssi);
    for (const n of networks) {
      const { li, input } = networkRow(n.ssid, n);
      input.addEventListener("change", () => pickNetwork(n));
      list.append(li);
    }
    const { li, input } = networkRow("Other network...", { other: true });
    input.addEventListener("change", () => pickNetwork("other"));
    list.append(li);
    $("wifi-note").textContent = networks.length
      ? "Pick your network. It hears 2.4 GHz only: a 5 GHz-only network will not appear."
      : `No networks heard${error ? ` (${error})` : ""}. The board hears 2.4 GHz only.`;
    if (!networks.length) autoAssist(`The board's Wi-Fi scan found no networks${error ? `: ${error}` : ""}.`);
  } catch (e) {
    $("wifi-note").textContent = `Could not scan: ${e.message}. You can still type the name under "Other network".`;
    const { li, input } = networkRow("Other network: type the name...", { other: true });
    input.addEventListener("change", () => pickNetwork("other"));
    list.append(li);
  } finally {
    button.disabled = false;
  }
}
$("scan-wifi").addEventListener("click", scanWifi);
$("skip-wifi").addEventListener("click", () => bluetoothStep());

$("pair-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  $("pair-done").hidden = true;
  const ssid = (chosenNetwork && chosenNetwork !== "other" ? chosenNetwork.ssid : $("ssid").value).trim();
  const password = $("password").value;
  const button = e.submitter;
  if (!ssid) {
    logTo($("pair-log"), "Pick a network, or type its name under Other network (or click Skip Wi-Fi).", "error");
    return;
  }
  if (setup.task === "wifi") {
    button.disabled = true;
    logTo($("pair-log"), `Sending "${ssid}" to the board over USB...`);
    try {
      const result = await api("/wifi/change", { port: setup.port, ssid, password });
      logTo($("pair-log"), `Board ${result.board} joined "${ssid}"${result.ip ? ` as ${result.ip}` : ""}. It keeps its pairing; this computer finds it again on that network.`);
      $("pair-done").hidden = false;
    } catch (err) {
      logTo($("pair-log"), `Could not change Wi-Fi: ${err.message}`, "error");
      autoAssist(`Changing the board's Wi-Fi failed: ${err.message}`);
    } finally {
      $("password").value = "";
      button.disabled = false;
    }
    return;
  }
  button.disabled = true;
  logTo($("pair-log"), `Joining "${ssid}" over USB...`);
  try {
    const endpoint = setup.usbPaired ? "/wifi/change" : "/provision";
    const result = await api(endpoint, { port: setup.port, ssid, password });
    setup.usbPaired = true;
    logTo($("pair-log"), `Board ${result.board} joined "${ssid}"${result.ip ? ` on ${result.ip}` : ""}`);
    bluetoothStep();
  } catch (err) {
    logTo($("pair-log"), `Wi-Fi setup failed: ${err.message}`, "error");
    autoAssist(`Wi-Fi setup failed: ${err.message}`);
  } finally {
    $("password").value = "";
    button.disabled = false;
  }
});

// ---- Step 6: Bluetooth (optional) ----

function bluetoothStep() {
  step(6);
  $("bluetooth-done").hidden = true;
  $("bluetooth-status").hidden = true;
}

async function openBluetoothSettings() {
  const status = $("bluetooth-status");
  try {
    const r = await api("/open", { target: "bluetooth_settings" });
    if (status) {
      status.hidden = false;
      status.className = "note";
      status.textContent = `${r.done}. Pick "KnoEnd" there to pair the board as a Bluetooth keyboard.`;
    }
    if (!$("advice").hidden) {
      $("advice-text").textContent = `${r.done}. Pick "KnoEnd" there to pair the board as a Bluetooth keyboard.`;
    }
    return r.done;
  } catch (e) {
    if (status) {
      status.hidden = false;
      status.className = "note error";
      status.textContent = `Could not open Bluetooth settings: ${e.message}`;
    }
    throw e;
  }
}

$("open-bluetooth").addEventListener("click", () => openBluetoothSettings().catch(() => {}));
$("bluetooth-done-btn").addEventListener("click", () => { $("bluetooth-done").hidden = false; });

// ---- setup mode and AI settings ----

function storeGet(key) { try { return localStorage.getItem(key); } catch { return null; } }
function storeSet(key, value) { try { localStorage.setItem(key, value); } catch { /* private window */ } }

let aiReady = false;

function setMode(mode) {
  setup.mode = mode;
  storeSet("knoend.mode", mode);
  document.querySelectorAll('input[name="mode"]').forEach((i) => (i.checked = i.value === mode));
  $("ai-form").hidden = mode !== "ai";
}
document.querySelectorAll('input[name="mode"]').forEach((i) => i.addEventListener("change", () => setMode(i.value)));

/** AI tools on this computer, signed in with the person's own account: what the settings say. */
const CLI_TOOLS = {
  "claude-code": "Uses the <code>claude</code> command signed in with your own account, with its tools switched off: it only reads the request and writes the answer. Not signed in yet? Run <code>claude</code> in a terminal and type <code>/login</code>. Leave the model empty for Claude Code's own choice.",
  antigravity: "Uses the <code>agy</code> command signed in with your Google account. It may not run commands or read your files, and works in an empty folder that is removed afterwards. Antigravity keeps these requests in its own conversation history. Not signed in yet? Run <code>agy</code> in a terminal. Leave the model empty for Antigravity's own choice.",
  codex: "Uses the <code>codex</code> command signed in with your ChatGPT account, running in an ephemeral read-only sandbox in an empty folder that is removed afterwards. Not signed in yet? Run <code>codex login</code> in a terminal. Leave the model empty for Codex's own choice.",
};

function providerFields() {
  const provider = $("ai-provider").value;
  $("ai-base-wrap").hidden = provider !== "openai-compatible";
  $("ai-base-alt-wrap").hidden = provider === "openai-compatible";
  const tool = CLI_TOOLS[provider];
  $("ai-key-wrap").hidden = provider === "ollama" || !!tool;
  $("ai-cli-note").hidden = !tool;
  $("ai-cli-note").innerHTML = tool || "";
  // No API address or request options for a command on this computer.
  $("ai-advanced").hidden = !!tool;
}
/** The API address: the main field for a generic server, the advanced one otherwise. */
function baseUrl() {
  const provider = $("ai-provider").value;
  if (CLI_TOOLS[provider]) return null;
  return (provider === "openai-compatible" ? $("ai-base").value : $("ai-base-alt").value).trim() || null;
}
$("ai-provider").addEventListener("change", () => {
  // Another provider: its own address and options.
  $("ai-base").value = ""; $("ai-base-alt").value = ""; $("ai-options").value = ""; $("ai-model").value = "";
  providerFields(); loadModels();
});

/** The provider's models, for the model combo box (search as you type, or open the whole list). */
let models = [];

/** Fills the model list from the provider (with the key just typed, if any; it is not saved). */
async function loadModels() {
  const note = $("ai-models-note");
  note.textContent = "(loading the provider's models...)";
  models = [];
  try {
    ({ models } = await api("/ai/models", { provider: $("ai-provider").value,
      api_key: $("ai-key").value || null, base_url: baseUrl() }));
    note.textContent = models.length ? `(${models.length} from the provider; search or open the list)` : "(the provider listed none; type a name)";
  } catch (e) {
    note.textContent = $("ai-key-wrap").hidden || $("ai-key").value || aiReady
      ? `(could not list models: ${e.message}; type a name)` : "(enter the API key to see the models)";
  }
}

const combo = { query: null, active: -1 };
function showModels(query) {
  combo.query = query;
  const list = $("ai-model-list");
  const q = (query || "").toLowerCase();
  const matches = models.filter((m) => m.toLowerCase().includes(q));
  list.replaceChildren(...matches.slice(0, 300).map((m, i) => {
    const li = document.createElement("li");
    li.role = "option";
    li.id = `ai-model-opt-${i}`;
    li.textContent = m;
    li.setAttribute("aria-selected", String(m === $("ai-model").value));
    li.addEventListener("mousedown", (e) => { e.preventDefault(); chooseModel(m); });
    return li;
  }));
  if (!matches.length) {
    const li = document.createElement("li");
    li.className = "muted";
    li.textContent = models.length ? "No model matches; this name will be used as typed." : "No list from the provider; type the model name.";
    list.append(li);
  }
  combo.active = -1;
  list.hidden = false;
  $("ai-model").setAttribute("aria-expanded", "true");
  const current = matches.indexOf($("ai-model").value);
  if (current >= 0) highlight(current);
}
function hideModels() {
  $("ai-model-list").hidden = true;
  $("ai-model").setAttribute("aria-expanded", "false");
  $("ai-model").removeAttribute("aria-activedescendant");
}
function highlight(i) {
  const items = [...$("ai-model-list").querySelectorAll('[role="option"]')];
  if (!items.length) return;
  combo.active = (i + items.length) % items.length;
  items.forEach((li, n) => li.classList.toggle("active", n === combo.active));
  items[combo.active].scrollIntoView({ block: "nearest" });
  $("ai-model").setAttribute("aria-activedescendant", items[combo.active].id);
}
function chooseModel(m) {
  $("ai-model").value = m;
  hideModels();
}
// Opening shows every model; typing filters (anywhere in the name, any case).
$("ai-model").addEventListener("focus", () => showModels(""));
$("ai-model").addEventListener("click", () => { if ($("ai-model-list").hidden) showModels(""); });
$("ai-model").addEventListener("input", () => showModels($("ai-model").value));
$("ai-model").addEventListener("blur", hideModels);
$("ai-model-toggle").addEventListener("mousedown", (e) => {
  e.preventDefault();
  if ($("ai-model-list").hidden) { $("ai-model").focus(); showModels(""); } else hideModels();
});
$("ai-model").addEventListener("keydown", (e) => {
  const open = !$("ai-model-list").hidden;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    if (!open) showModels("");
    highlight(combo.active + (e.key === "ArrowDown" ? 1 : -1));
  } else if (e.key === "Enter" && open && combo.active >= 0) {
    e.preventDefault();
    chooseModel($("ai-model-list").querySelectorAll('[role="option"]')[combo.active].textContent);
  } else if (e.key === "Escape" && open) {
    e.preventDefault();
    hideModels();
  }
});
$("ai-key").addEventListener("change", loadModels);
$("ai-base").addEventListener("change", loadModels);
$("ai-base-alt").addEventListener("change", loadModels);

function renderAi(info) {
  aiReady = !!info.configured && info.key !== "missing";
  $("assistant-by").textContent = aiReady ? `using ${info.describe}` : "";
  $("assistant-off").hidden = aiReady;
  $("ask-form").hidden = !aiReady;
  const keyText = { env: "key from the environment", saved: "key saved on this computer", "none-needed": "no key needed", missing: "no key yet" }[info.key] || "";
  $("ai-status").textContent = info.configured ? `Current: ${info.describe}, ${keyText}.` : "Not set up yet.";
}

/**
 * The AI tools found on this computer just now (Claude Code, Antigravity, Codex): installed ones are
 * offered in the provider list, with whether they are signed in. Nothing is assumed in advance.
 */
async function loadAiTools() {
  const line = $("ai-tools");
  line.textContent = "Looking for AI tools on this computer...";
  let tools = [];
  try { ({ tools } = await api("/ai/tools")); } catch (err) { line.textContent = `Could not look for AI tools: ${err.message}`; return; }
  const found = tools.filter((t) => t.installed);
  for (const t of tools) {
    const option = document.querySelector(`#ai-provider option[value="${t.provider}"]`);
    if (option) option.hidden = !t.installed;
  }
  line.replaceChildren();
  if (!found.length) {
    line.textContent = "No AI tools found on this computer (Claude Code, Antigravity CLI or Codex CLI). With one installed and signed in, you can use your own plan without an API key. ";
  } else {
    line.append("Found on this computer: ");
    found.forEach((t, i) => {
      const state = !t.supported ? "KnoEnd cannot use it yet"
        : t.signed_in === true ? "signed in"
        : t.signed_in === false ? `not signed in: ${t.sign_in}`
        : `could not check the sign-in${t.check_error ? ` (${t.check_error})` : ""}`;
      line.append(`${i ? "; " : ""}${t.name}${t.version ? ` ${t.version.replace(/\s*\(.*\)$/, "")}` : ""} (${state})`);
    });
    line.append(". ");
  }
  const again = document.createElement("button");
  again.type = "button"; again.className = "secondary small-button"; again.textContent = "Look again";
  again.addEventListener("click", loadAiTools);
  line.append(again);
}

async function loadAi() {
  loadAiTools();
  const info = await api("/ai");
  if (info.provider) $("ai-provider").value = info.provider;
  if (info.model) $("ai-model").value = info.model;
  if (info.base_url) $(info.provider === "openai-compatible" ? "ai-base" : "ai-base-alt").value = info.base_url;
  const options = { ...(info.options || {}) };
  for (const name of info.omit || []) options[name] = null;
  $("ai-options").value = Object.keys(options).length ? JSON.stringify(options) : "";
  $("ai-advanced").open = !!(Object.keys(options).length || (info.base_url && info.provider !== "openai-compatible"));
  providerFields();
  renderAi(info);
  loadModels();
  return info;
}

$("ai-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const button = e.submitter;
  button.disabled = true;
  $("ai-status").className = "note";
  $("ai-status").textContent = "Saving and sending a one-word test request...";
  try {
    let options = null;
    if ($("ai-options").value.trim()) {
      try { options = JSON.parse($("ai-options").value); } catch { throw new Error("request options are not valid JSON"); }
    }
    const r = await api("/ai", { provider: $("ai-provider").value, model: $("ai-model").value || null,
                                api_key: $("ai-key").value || null, base_url: baseUrl(), options });
    if (r.ok) {
      $("ai-status").className = "note success";
      $("ai-status").textContent = `Works: ${r.describe} answered.`;
      renderAi({ configured: true, key: r.key, describe: r.describe });
    } else {
      $("ai-status").className = "note error";
      $("ai-status").textContent = `Saved, but the test failed: ${r.error}`;
      if (/Advanced/.test(r.error)) $("ai-advanced").open = true;
      if (/ 404 /.test(r.error)) loadModels().then(() => { $("ai-model").focus(); showModels(""); });
    }
  } catch (err) {
    $("ai-status").className = "note error";
    $("ai-status").textContent = `Could not save: ${err.message}`;
  } finally {
    $("ai-key").value = "";
    button.disabled = false;
  }
});

// ---- setup assistant ----

const ACTIONS = {
  rescan: ["Look again", () => { step(1); scanPorts(); }],
  identify: ["Identify the board", () => { step(1); $("identify").click(); }],
  install: ["Install", () => { if (setup.board) { $("erase").checked = false; $("install").click(); } else step(1); }],
  install_erase: ["Erase and install", () => { if (setup.board) { $("erase").checked = true; $("install").click(); } else step(1); }],
  look_settings: ["Keys and look", () => { setTask("setup"); keySizeStep(); show("setup"); }],
  pair: ["Set up Wi-Fi", () => { setTask("setup"); step(5); scanWifi(); show("setup"); }],
  change_wifi: ["Change Wi-Fi", () => { setTask("wifi"); step(1); show("setup"); }],
  open_bluetooth_settings: ["Open Bluetooth settings", () => openBluetoothSettings().catch(() => {})],
  app_setup: ["Keys for your apps", () => openApps()],
  dashboard: ["Open the dashboard", () => show("dashboard")],
  open_accessibility_settings: ["Open Accessibility settings", () => openOnMac("accessibility_settings")],
  open_driver_settings: ["Open Driver Extensions", () => openOnMac("driver_settings")],
  install_usb_driver: ["Install the USB driver", () => openOnMac("usb_driver")],
};

/** Settings pages and the driver installer the person approves on their Mac (macOS only). Then
 *  keeps checking, so the wizard carries on once the person has done their part. */
async function openOnMac(target) {
  let text;
  try {
    const r = await api("/open", { target });
    if (target === "accessibility_settings") {
      text = `${r.done}. Switch on the app it names there; this page will notice.`;
      watchTyping();
    } else {
      if (setup.step !== 1) step(1);
      text = `${r.done}. When you have finished there, unplug the board and plug it in again; this page keeps looking and carries on when it appears.`;
    }
  } catch (e) {
    text = `Could not open it: ${e.message}`;
  }
  $("advice-text").textContent = text;
  return text;
}

/** After the Accessibility page is opened: check every few seconds (up to 10 minutes). */
let typingWatch = null;
function watchTyping() {
  clearInterval(typingWatch);
  const until = Date.now() + 10 * 60 * 1000;
  typingWatch = setInterval(async () => {
    if (Date.now() > until) return clearInterval(typingWatch);
    try {
      const r = await api("/typing");
      if (r.can_type_keys) {
        clearInterval(typingWatch);
        $("advice").hidden = false;
        $("advice-text").textContent = "Typing is allowed now. Press a key on the board to try it; if nothing is typed, restart knoend once.";
        $("advice-actions").replaceChildren();
      }
    } catch { /* service restarting; try again */ }
  }, 3000);
}

/** One line per change in a suggested fix; `settings` are labelled by the service. */
function describeFix(fix, settings = []) {
  const lines = settings.map((s) => `${s.title}: ${s.label}`);
  if (fix.model) lines.push(`Model: ${fix.model}`);
  if (fix.base_url !== undefined) lines.push(fix.base_url ? `API address: ${fix.base_url}` : "API address: the provider's usual one");
  for (const [name, value] of Object.entries(fix.options || {})) {
    lines.push(value === null ? `Leave out the setting "${name}"` : `Set "${name}" to ${JSON.stringify(value)}`);
  }
  if (fix.timeout_secs) lines.push(`Wait up to ${fix.timeout_secs} seconds for a reply`);
  if (fix.board) {
    const c = (setup.identified?.candidates || []).find((c) => c.board === fix.board);
    lines.push(`Install for: ${c ? c.name : fix.board}`);
  }
  return lines;
}

/** Applies a fix the person accepted: the board choice here, AI settings saved and tested. */
async function applyFix(fix) {
  const status = $("advice-fix-status");
  status.hidden = false;
  status.className = "note";
  const done = [];
  if (fix.board) {
    const c = (setup.identified?.candidates || []).find((c) => c.board === fix.board && (c.status === "supported" || c.status === "unverified"));
    const input = c && document.querySelector(`#candidates input[value="${CSS.escape(c.board)}"]`);
    if (!input) {
      status.className = "note error";
      status.textContent = "That board is not one of the installable boards found for this hardware; nothing was changed.";
      return;
    }
    input.checked = true;
    setup.board = c.board;
    $("install").disabled = false;
    if (setup.step !== 2) step(2);
    done.push(`chose ${c.name}`);
  }
  if (fix.settings) {
    try {
      const r = await api("/settings", { changes: fix.settings });
      await loadDisplay().catch(() => {});
      done.push(r.done.join(", "));
    } catch (e) {
      status.className = "note error";
      status.textContent = `Could not apply it: ${e.message}`;
      return;
    }
  }
  if (fix.model || fix.base_url !== undefined || fix.options || fix.timeout_secs) {
    status.textContent = "Saving the change and sending a one-word test request...";
    try {
      const info = await api("/ai");
      const options = { ...(info.options || {}) };
      for (const name of info.omit || []) options[name] = null;
      Object.assign(options, fix.options || {});
      const r = await api("/ai", { provider: info.provider, model: fix.model || info.model || null, api_key: null,
        base_url: fix.base_url !== undefined ? (fix.base_url || null) : (info.base_url || null),
        options: Object.keys(options).length ? options : null, timeout_secs: fix.timeout_secs || info.timeout_secs || null });
      await loadAi();
      if (!r.ok) {
        status.className = "note error";
        status.textContent = `Saved, but the test still fails: ${r.error}`;
        return;
      }
      done.push(`AI settings saved; ${r.describe} answered the test`);
    } catch (e) {
      status.className = "note error";
      status.textContent = `Could not apply it: ${e.message}`;
      return;
    }
  }
  status.className = "note success";
  status.textContent = `Done: ${done.join("; ")}.`;
  $("advice-fix-apply").disabled = true;
}

/**
 * Shows an answer from the assistant in the floating panel: asked here, or raised by the service's
 * watcher (`noticed`: what it noticed, shown first).
 */
function showAdvice(r) {
  assistantPanel(true);
  $("advice").hidden = false;
  $("advice-text").classList.remove("thinking");
  if (r.noticed) { keepInThread(); $("advice-q").textContent = "Noticed by KnoEnd"; $("advice-q").hidden = false; }
  $("advice-actions").replaceChildren();
  $("advice-fix").hidden = true;
  const looked = (r.checked || []).map((c) => c.check.replaceAll("_", " "));
  const noticed = r.noticed ? `KnoEnd noticed, without being asked:\n- ${r.noticed.join("\n- ")}\n\n` : "";
  $("advice-text").textContent = noticed + (looked.length ? `(Checked: ${looked.join(", ")}.)\n` : "") + (r.answer || "");
  $("advice-sent").textContent = r.sent ? JSON.stringify(r.sent, null, 2) : "";
  for (const id of r.actions || []) {
    if (!ACTIONS[id]) continue;
    const b = document.createElement("button");
    b.className = "secondary";
    b.textContent = ACTIONS[id][0];
    b.addEventListener("click", ACTIONS[id][1]);
    $("advice-actions").append(b);
  }
  if (r.fix) {
    $("advice-fix-list").replaceChildren(...describeFix(r.fix, r.settings_described || []).map((line) => { const li = document.createElement("li"); li.textContent = line; return li; }));
    $("advice-fix-status").hidden = true;
    const apply = $("advice-fix-apply");
    apply.disabled = false;
    apply.onclick = () => applyFix(r.fix);
    $("advice-fix").hidden = false;
  }
}

/** Something the watcher raised: shown once per raise (remembered in this browser). */
function onNoticed(r) {
  if (!r) return;
  const key = JSON.stringify(r.noticed || []);
  try { if (localStorage.getItem("knoend-noticed") === key) return; localStorage.setItem("knoend-noticed", key); } catch {}
  showAdvice(r);
}

async function ask(question, error) {
  if (!aiReady) {
    assistantPanel(true);
    $("assistant-off").hidden = false;
    return;
  }
  assistantPanel(true);
  keepInThread();
  const box = $("advice");
  box.hidden = false;
  const shown = question.trim() || (error ? `Something went wrong: ${error}` : "What should I do next?");
  $("advice-q").textContent = shown;
  $("advice-q").hidden = false;
  $("advice-text").textContent = "Looking";
  $("advice-text").classList.add("thinking");
  $("advice-actions").replaceChildren();
  $("advice-fix").hidden = true;
  $("ask-input").value = "";
  const page = currentView();
  const context = { page, wizard_task: setup.task, wizard_step: setup.step, port: setup.port, chosen_board: setup.board,
                    identify_result: setup.identified, error: error || null, setup_mode: setup.mode,
                    ai_models_available: models.slice(0, 200) };
  try {
    showAdvice(await api("/assist", { question, context }));
  } catch (e) {
    $("advice-text").classList.remove("thinking");
    $("advice-text").textContent = `The assistant could not answer: ${e.message}`;
  }
}

function currentView() {
  return [...document.querySelectorAll("main .view")].find((v) => !v.hidden)?.id.replace("view-", "") || null;
}

/** The answer on show goes into the conversation above, as text, before the next one. */
function keepInThread() {
  if ($("advice").hidden || !$("advice-text").textContent || $("advice-text").classList.contains("thinking")) return;
  const li = document.createElement("li");
  li.className = "exchange";
  const q = document.createElement("p");
  q.className = "q";
  q.textContent = $("advice-q").textContent;
  const a = document.createElement("p");
  a.className = "advice";
  a.textContent = $("advice-text").textContent;
  li.append(q, a);
  $("thread").append(li);
  while ($("thread").children.length > 12) $("thread").firstElementChild.remove();
}

// ---- help across the app: suggested questions, an "ask about this" on every section, tips ----

/** Questions worth asking on each page, given what is going on. */
function suggestionsFor(page) {
  const s = renderStatus.last || {};
  const out = [];
  if (s.link && s.link !== "connected") out.push(s.link === "no-pairing" ? "How do I set up my board?" : "Why can't KnoEnd find my board?");
  if (page === "dashboard") out.push("Why don't my keys change with the app?", "Make the keys easier to read", "Is my board healthy?", "Suggest a font for the keys");
  if (page === "edit") out.push("How do I add another page of keys?", "What does Primary mean in a shortcut?", "Which version of these keys is the board using?");
  if (page === "setup") out.push("What should I do next?", "My board is not found over USB", "Which board do I have?");
  return out.slice(0, 4);
}

function renderSuggestions() {
  const box = $("ask-suggestions");
  box.replaceChildren(...suggestionsFor(currentView()).map((q) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "chip";
    b.textContent = q;
    b.addEventListener("click", () => ask(q));
    return b;
  }));
}

// ---- keys for the apps on this computer (step 7, and "Set up apps" on the dashboard) ----

const apps = { list: [], chosen: new Set(), progress: new Map(), icons: new Map(), drafting: false, pins: [], fill: true };
const APPS_MOST = 12;
const APP_STATE = {
  ready: ["Ready", "keys from a recipe"],
  yours: ["Yours", "your own keys"],
  drafted: ["Drafted", "AI keys, unreviewed"],
  none: ["", "the AI can draft keys"],
};

function openApps() {
  if (setup.task !== "setup") setTask("setup");
  setup.picked = true;
  renderKnown();
  show("setup");
  step(7);
  document.querySelector('.step[data-step="7"]').scrollIntoView({ block: "start" });
  loadApps().catch((e) => appsStatus(`Could not list the apps: ${e.message}`, true));
}
document.addEventListener("click", (e) => {
  if (e.target.closest("[data-apps]")) { e.preventDefault(); openApps(); }
});
$("apps-ai-setup").addEventListener("click", (e) => {
  e.preventDefault();
  setMode("ai");
  $("mode-card").scrollIntoView({ behavior: "smooth", block: "start" });
});
$("apps-search").addEventListener("input", () => renderApps());

function appsStatus(text, error) {
  const el = $("apps-status");
  el.hidden = !text;
  el.className = error ? "note error" : "note";
  el.textContent = text || "";
}

async function loadApps() {
  const [r, settings] = await Promise.all([api("/apps"), api("/settings").catch(() => ({ settings: [] }))]);
  apps.list = r.apps || [];
  const setting = (id) => (settings.settings || []).find((x) => x.id === id);
  if (setting("home_apps")) apps.pins = setting("home_apps").current || [];
  if (setting("home_fill")) apps.fill = setting("home_fill").current !== false;
  renderHome();
  renderKnown();
  $("apps-ai-off").hidden = aiReady;
  renderApps();
  const ready = apps.list.filter((a) => a.state !== "none").length;
  $("apps-summary").textContent = `${apps.list.length} apps on this computer; ${ready} have keys. The AI drafts keys for the others before you open them.`;
}

/** Used for a while, or with keys already: shown first. */
function suggested(app) {
  return app.minutes > 0 || app.state !== "none";
}

function appTile(app) {
  const li = document.createElement("li");
  const progress = apps.progress.get(app.id);
  const state = progress ? progress.state : app.state;
  li.className = `app-tile s-${state}`;
  const label = document.createElement("label");
  const box = document.createElement("input");
  box.type = "checkbox";
  box.checked = apps.chosen.has(app.id);
  // Apps with keys of their own, or being drafted now, are not picked again here.
  box.disabled = apps.drafting || ["ready", "yours", "waiting", "drafting", "done"].includes(state);
  box.addEventListener("change", () => {
    if (box.checked && apps.chosen.size >= APPS_MOST) { box.checked = false; return; }
    box.checked ? apps.chosen.add(app.id) : apps.chosen.delete(app.id);
    renderChosen();
    li.classList.toggle("picked", box.checked);
  });
  li.classList.toggle("picked", box.checked);
  const icon = document.createElement("span");
  icon.className = "app-icon";
  icon.textContent = app.name.slice(0, 1);
  loadIcon(app.id, icon);
  const text = document.createElement("span");
  text.className = "app-text";
  const name = document.createElement("strong");
  name.textContent = app.name;
  const sub = document.createElement("span");
  sub.className = "sub";
  sub.textContent = tileLine(app, progress);
  text.append(name, sub);
  label.append(box, icon, text);
  const chip = APP_STATE[state] ? APP_STATE[state][0] : "";
  if (chip) {
    const badge = document.createElement("span");
    badge.className = `app-chip c-${state}`;
    badge.textContent = chip;
    label.append(badge);
  }
  li.append(label);
  const key = `app:${app.id}`;
  const pin = document.createElement("button");
  pin.type = "button";
  pin.className = "pin";
  const pinned = apps.pins.includes(key);
  pin.setAttribute("aria-pressed", pinned);
  pin.title = pinned ? `Unpin ${app.name} from the board's home screen` : `Pin ${app.name} to the board's home screen`;
  pin.setAttribute("aria-label", pin.title);
  pin.addEventListener("click", () => savePins(pinned ? apps.pins.filter((k) => k !== key) : [...apps.pins, key]));
  li.append(pin);
  if (progress && progress.state === "failed") {
    const again = document.createElement("button");
    again.type = "button";
    again.className = "secondary small-button";
    again.textContent = "Ask why";
    again.addEventListener("click", () => ask(`Drafting keys for ${app.name} failed: ${progress.error}. Why, and what should I do?`, progress.error));
    li.append(again);
  }
  return li;
}

/** The pinned apps as chips, in order, each movable and removable. */
function renderHome() {
  $("home-fill").checked = apps.fill;
  const list = $("home-pins");
  if (!apps.pins.length) {
    list.replaceChildren(Object.assign(document.createElement("li"), { className: "muted small empty", textContent: apps.fill ? "Nothing pinned: the board shows what you use most." : "Nothing pinned, and filling is off: the home screen is empty." }));
    return;
  }
  list.replaceChildren(...apps.pins.map((key, i) => {
    const [kind, id] = [key.slice(0, key.indexOf(":")), key.slice(key.indexOf(":") + 1)];
    const app = kind === "app" ? apps.list.find((a) => a.id === id) : null;
    const name = app ? app.name : id;
    const li = document.createElement("li");
    li.className = "home-pin";
    const icon = document.createElement("span");
    icon.className = "app-icon";
    icon.textContent = name.slice(0, 1);
    if (app) loadIcon(app.id, icon);
    const label = document.createElement("span");
    label.textContent = name;
    const move = (to, text, glyph) => {
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = glyph;
      b.title = text;
      b.setAttribute("aria-label", `${text}: ${name}`);
      b.disabled = to < 0 || to >= apps.pins.length;
      b.addEventListener("click", () => {
        const next = [...apps.pins];
        if (text === "Remove") next.splice(i, 1);
        else [next[i], next[to]] = [next[to], next[i]];
        savePins(next);
      });
      return b;
    };
    li.append(Object.assign(document.createElement("span"), { className: "pos", textContent: String(i + 1) }), icon, label,
              move(i - 1, "Move earlier", "\u2190"), move(i + 1, "Move later", "\u2192"), move(0, "Remove", "\u00d7"));
    return li;
  }));
}

async function savePins(pins, fill = apps.fill) {
  const before = [apps.pins, apps.fill];
  apps.pins = pins;
  apps.fill = fill;
  renderHome();
  renderApps();
  try {
    await api("/settings", { changes: { home_apps: pins, home_fill: fill } });
  } catch (e) {
    [apps.pins, apps.fill] = before;
    renderHome();
    renderApps();
    appsStatus(`Could not change the home screen: ${e.message}`, true);
  }
}
$("home-fill").addEventListener("change", () => savePins(apps.pins, $("home-fill").checked));

function tileLine(app, progress) {
  if (progress) {
    return { waiting: "Waiting for its turn", drafting: "Drafting keys...", done: `${progress.keys} key${progress.keys === 1 ? "" : "s"} drafted`,
             failed: "Could not draft keys" }[progress.state] || "";
  }
  const used = app.minutes >= 60 ? `used ${Math.round(app.minutes / 60)} h` : app.minutes > 0 ? `used ${app.minutes} min` : "";
  const what = app.keys ? `${app.keys} key${app.keys === 1 ? "" : "s"}` : APP_STATE[app.state][1];
  return [what, used].filter(Boolean).join(" · ");
}

async function loadIcon(id, el) {
  let url = apps.icons.get(id);
  if (url === undefined) {
    url = null;
    try {
      const r = await fetch(`/api/apps/icon/${encodeURIComponent(id)}`, { headers: { "x-knoend-token": TOKEN } });
      if (r.ok) url = URL.createObjectURL(await r.blob());
    } catch { /* the letter stays */ }
    apps.icons.set(id, url);
  }
  if (url) {
    el.textContent = "";
    const img = document.createElement("img");
    img.src = url;
    img.alt = "";
    el.append(img);
  }
}

function renderApps() {
  const query = $("apps-search").value.trim().toLowerCase();
  const shown = apps.list.filter((a) => !query || a.name.toLowerCase().includes(query));
  const top = shown.filter(suggested);
  const rest = shown.filter((a) => !suggested(a));
  $("apps-suggested").replaceChildren(...(top.length ? top.map(appTile) : [Object.assign(document.createElement("li"), { className: "muted", textContent: query ? "None of these match." : "None yet." })]));
  $("apps-more").replaceChildren(...rest.map(appTile));
  $("apps-more-count").textContent = `(${rest.length})`;
  if (query && rest.length) $("apps-more-wrap").open = true;
  $("apps-count").textContent = query ? `${shown.length} of ${apps.list.length}` : `${apps.list.length} apps`;
  renderChosen();
}

function renderChosen() {
  const n = apps.chosen.size;
  $("apps-chosen").textContent = n ? `${n} picked${n >= APPS_MOST ? " (the most at once)" : ""}` : `Pick up to ${APPS_MOST} apps`;
  $("apps-draft").disabled = !n || apps.drafting || !aiReady;
  $("apps-draft").textContent = n ? `Draft keys for ${n} app${n === 1 ? "" : "s"}` : "Draft keys";
}

$("apps-draft").addEventListener("click", async () => {
  const ids = [...apps.chosen];
  apps.drafting = true;
  $("apps-done").hidden = true;
  for (const id of ids) apps.progress.set(id, { state: "waiting" });
  renderApps();
  appsStatus(`Drafting keys for ${ids.length} app${ids.length === 1 ? "" : "s"}, two at a time. You can keep using the computer.`);
  try {
    await api("/apps/draft", { ids });
  } catch (e) {
    apps.drafting = false;
    for (const id of ids) apps.progress.delete(id);
    renderApps();
    appsStatus(`Could not start: ${e.message}`, true);
  }
});

/** An "app-setup" event: how one app's draft is going. */
function onAppSetup(d) {
  apps.progress.set(d.id, d);
  const pending = [...apps.progress.values()].filter((p) => p.state === "waiting" || p.state === "drafting").length;
  if (!pending && apps.drafting) {
    apps.drafting = false;
    apps.chosen.clear();
    const done = [...apps.progress.values()].filter((p) => p.state === "done").length;
    const failed = [...apps.progress.values()].filter((p) => p.state === "failed").length;
    appsStatus(failed ? `${done} drafted, ${failed} could not be drafted (Ask why on each).` : "");
    $("apps-done").hidden = !done;
    loadApps().then(() => {
      // The list now says "Drafted"; keep the results showing until the person moves on.
      for (const [id, p] of apps.progress) if (p.state !== "failed") apps.progress.delete(id);
      renderApps();
    }).catch(() => {});
    loadDrafts().catch(() => {});
    return;
  }
  renderApps();
}

/** A small ° on every section title: ask the assistant about that part of the page. */
function addAskButtons() {
  for (const h2 of document.querySelectorAll("main .card h2")) {
    if (h2.querySelector(".ask-about")) continue;
    const title = [...h2.childNodes].filter((n) => n.nodeType === Node.TEXT_NODE).map((n) => n.textContent).join(" ").trim();
    const b = document.createElement("button");
    b.type = "button";
    b.className = "ask-about";
    b.title = `Ask the assistant about ${title}`;
    b.setAttribute("aria-label", `Ask the assistant about ${title}`);
    b.textContent = "°";
    b.addEventListener("click", () => ask(`About "${title}" on the ${currentView()} page: explain what I am looking at, and tell me if anything there should change.`));
    h2.append(b);
  }
}

const TIPS = {
  dashboard: [
    "Open an app without keys of its own, then tap Configure on the board: the AI drafts keys for it in seconds.",
    "Swipe the board sideways, or tap the page number on its status bar, for more keys.",
    "The ° in Kn°End lights up while the board is connected.",
    "When Claude asks a question, the board shows it with the answers on its keys.",
    "Something odd? The assistant watches in the background and tells you what it noticed.",
  ],
  edit: [
    "Tap a key on the board to select it here.",
    "Drag a key onto another place to move it; nothing changes on the board's real keys until you Save.",
    "Press Record and then the keys, instead of typing a shortcut.",
  ],
  setup: [
    "Guided by AI: when a step fails, the assistant reads what happened and tells you the next step.",
    "Keys for your apps: pick a few on the last step and the AI drafts them before you open the apps.",
    "Installing without erasing keeps the board's Wi-Fi and pairing.",
    "The board hears 2.4 GHz Wi-Fi only.",
  ],
};
let tipIndex = 0;

function showTip() {
  const page = currentView();
  const tips = TIPS[page] || [];
  const el = document.querySelector(`#view-${page} .tip`);
  if (!el || !tips.length) return;
  el.querySelector(".tip-text").textContent = tips[tipIndex % tips.length];
}

function addTips() {
  for (const page of Object.keys(TIPS)) {
    const view = $(`view-${page}`);
    if (!view || view.querySelector(".tip")) continue;
    const tip = document.createElement("p");
    tip.className = "tip";
    tip.innerHTML = `<span class="tip-label">Tip</span><span class="tip-text"></span>`;
    const next = document.createElement("button");
    next.type = "button";
    next.className = "tip-next";
    next.textContent = "Next";
    next.addEventListener("click", () => { tipIndex++; showTip(); });
    tip.append(next);
    view.prepend(tip);
  }
  showTip();
}
setInterval(() => { tipIndex++; showTip(); }, 20000);

/** In guided mode, every failure goes to the assistant with what was happening. */
function autoAssist(error) {
  if (setup.mode === "ai" && aiReady) ask("", error);
}

/** Opens or closes the floating assistant; it keeps its last answer while closed. */
function assistantPanel(open) {
  $("assistant").hidden = !open;
  $("assistant-open").hidden = open;
  $("assistant-open").setAttribute("aria-expanded", String(open));
  if (open) { renderSuggestions(); $("ask-input").focus(); }
}
$("assistant-open").addEventListener("click", () => assistantPanel(true));
$("assistant-close").addEventListener("click", () => assistantPanel(false));
document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !$("assistant").hidden) assistantPanel(false); });

$("ask-form").addEventListener("submit", (e) => {
  e.preventDefault();
  ask($("ask-input").value);
});

// ---- start ----

// ---- colours: Auto (the browser's setting), Light or Dark, remembered by this browser ----

function setScheme(choice) {
  if (choice === "light" || choice === "dark") document.documentElement.dataset.theme = choice;
  else delete document.documentElement.dataset.theme;
  try { if (choice === "auto") localStorage.removeItem("knoend-scheme"); else localStorage.setItem("knoend-scheme", choice); } catch {}
  document.querySelectorAll(".scheme button").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.scheme === choice)));
}
document.querySelectorAll(".scheme button").forEach((b) => b.addEventListener("click", () => setScheme(b.dataset.scheme)));
setScheme(document.documentElement.dataset.theme || "auto");

(async function start() {
  addAskButtons();
  addTips();
  connectEvents();
  api("/assist/noticed").then((r) => onNoticed(r.noticed)).catch(() => {});
  try {
    const { status } = await api("/state");
    renderStatus(status);
    if (status.link === "no-pairing") show("setup");
  } catch (e) {
    $("link-pill").textContent = "Service not reachable";
  }
  loadDrafts().catch(() => {});
  const info = await loadAi().catch(() => ({}));
  setMode(storeGet("knoend.mode") || "ai"); // guided is the recommended default
  loadApps().catch(() => {});
})();
