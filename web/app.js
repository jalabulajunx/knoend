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
}
document.querySelectorAll(".tabs button").forEach((b) => b.addEventListener("click", () => {
  if (b.dataset.view === "setup" && setup.task !== "setup") { setTask("setup"); step(1); }
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
  }
  $("now-app").textContent = s.app || "-";
  const layout = s.link === "connected" ? s.layout : null;
  $("now-layout").textContent = layout ? layout.name + (layout.pages > 1 ? ` (page ${layout.page} of ${layout.pages})` : "") : "-";
  const badge = $("now-badge");
  badge.hidden = !layout || layout.reviewed;
  badge.textContent = "AI draft, unreviewed";

  const keys = $("keys-preview");
  keys.replaceChildren();
  const grid = (layout && layout.grid) || { cols: 3, rows: 3 };
  keys.style.gridTemplateColumns = `repeat(${grid.cols}, 1fr)`;
  keys.style.gridTemplateRows = `repeat(${grid.rows}, 1fr)`;
  for (let slot = 1; slot <= grid.cols * grid.rows; slot++) {
    const div = document.createElement("div");
    const key = layout && layout.keys.find((k) => k.key === slot);
    div.textContent = key ? key.label : "";
    if (!key) div.className = "blank";
    keys.append(div);
  }
  const conf = $("configuring");
  conf.hidden = !s.configuring;
  conf.textContent = s.configuring ? `The AI agent is drafting keys for ${s.configuring}...` : "";
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
    if (event.kind === "edit") onEditEvent(d);
    if (event.kind === "edit.tap") onEditTap(d);
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
const edit = { state: null, selected: null, timer: null, recording: false, rev: 0, sent: 0 };

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
  const pick = $("edit-pick");
  pick.replaceChildren(new Option("Choose...", ""));
  const groups = { app: "Apps", site: "Sites", agent: "Agent screens" };
  for (const [kind, title] of Object.entries(groups)) {
    const group = document.createElement("optgroup");
    group.label = title;
    // The person's own versions are reached through what they layer on.
    const libs = list.libraries.filter((l) => l.kind === kind && !l.overrides).sort((a, b) => a.name.localeCompare(b.name));
    for (const l of libs) {
      const own = list.libraries.some((o) => o.overrides === l.id);
      group.append(new Option(`${l.name} (${own ? "yours" : SOURCES[l.source] || l.source})`, l.id));
    }
    if (libs.length) pick.append(group);
  }
  pick.disabled = false;
  $("edit-icon").replaceChildren(new Option("(none)", ""), ...list.icons.map((name) => new Option(name, name)));
  onEditState(list.edit, list.shown);
}

function onEditState(state, shown) {
  edit.state = state && state.editing ? state : null;
  const on = !!edit.state;
  $("edit-toggle").textContent = on ? "Stop editing" : "Start editing";
  $("edit-main").hidden = !on;
  $("edit-dirty").hidden = !(on && edit.state.dirty);
  if (!on) {
    edit.selected = null;
    if (shown && [...$("edit-pick").options].some((o) => o.value === shown)) $("edit-pick").value = shown;
    return;
  }
  const pack = edit.state.library.pack;
  $("edit-pick").value = pack.overrides || pack.id;
  $("edit-remove").hidden = !(edit.state.own && edit.state.overrides);
  $("edit-save").disabled = $("edit-revert").disabled = !edit.state.dirty;
  $("edit-saved").textContent = edit.state.own ? "Saved as your own version." : "Not saved yet: saving makes your own version, on top of this one.";
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
$("edit-save").addEventListener("click", () => editCommand("save").then(loadEdit).catch(() => {}));
$("edit-revert").addEventListener("click", () => editCommand("revert").catch(() => {}));
$("edit-remove").addEventListener("click", () => {
  if (confirm("Remove your version? The keys go back to the recipe or AI draft underneath.")) editCommand("remove").then(loadEdit).catch(() => {});
});

// ---- setup wizard ----

const setup = { step: 1, port: null, board: null, firmware: null, identified: null, mode: null, task: "setup" };

/**
 * The wizard does two jobs: set up a board (find, firmware, install, pair and Wi-Fi), or move a
 * board that is already set up to another Wi-Fi network (find, Wi-Fi): over USB, keeping its
 * pairings, so it works when the board cannot reach its old network.
 */
function setTask(task) {
  setup.task = task;
  const wifi = task === "wifi";
  // Firmware and Install are not part of changing Wi-Fi (step() rewrites the class names, so by number).
  document.querySelectorAll('.steps li[data-step="2"], .steps li[data-step="3"], .steps li[data-step="5"]').forEach((el) => (el.hidden = wifi));
  $("mode-card").hidden = wifi;
  $("step4-name").textContent = wifi ? "Wi-Fi" : "Pair and Wi-Fi";
  $("step4-title").textContent = wifi ? "Change Wi-Fi" : "Pair and Wi-Fi";
  $("step4-intro").textContent = wifi
    ? "Pick the network the board should use from now on. Its pairing with this computer and its settings stay as they are; the password goes to the board over the cable only and is not stored here."
    : "The board and this computer agree on a secret key over the cable. Wi-Fi lets the board work away from the desk; your password goes to the board over the cable only and is not stored here.";
  $("step4-submit").textContent = wifi ? "Change Wi-Fi" : "Pair";
  $("step4-back").dataset.back = wifi ? "1" : "3";
  $("identify").textContent = wifi ? "Next" : "Identify";
  $("pair-log").replaceChildren();
  $("pair-done").hidden = true;
}

$("change-wifi").addEventListener("click", () => {
  setTask("wifi");
  step(1);
  show("setup");
});

function step(n) {
  setup.step = n;
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
  ["big", "Big: up to 2x2", "Four large keys a page. Easiest to read and tap; most apps need several pages."],
  ["medium", "Medium: up to 3x3 (recommended)", "Nine keys a page. Comfortable on a 2.8-inch screen."],
  ["small", "Small: up to 4x4", "Sixteen keys a page. Short labels, or icons only; best for a bigger screen or a steady hand."],
];

const THEMES = [
  ["dark", "Dark, in colour", "Light text on dark keys; colour shows a pressed key, a key waiting for its second tap, and messages."],
  ["paper", "Paper: black on white", "An e-ink look with high contrast, easy to read in daylight. App pictures turn grey."],
];

async function saveDisplay(change) {
  const saved = await api("/display", change);
  $("key-size").value = saved.key_size;
  $("theme").value = saved.theme;
  return saved;
}

/** Fills the dashboard's choices; the look only when the board's firmware can change it. */
async function loadDisplay() {
  const d = await api("/display");
  $("key-size").value = d.key_size;
  $("theme").value = d.theme;
  $("theme").disabled = !d.board_can_change_look;
  $("theme").title = d.board_can_change_look ? "" : "This board's firmware has one look only; install the latest firmware to choose.";
  return d;
}

/** Setup step 5: the key size, chosen once after pairing (the current choice ticked). */
async function keySizeStep() {
  step(5);
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
  fill($("theme-setup"), "theme-choice", THEMES, current.theme, !current.board_can_change_look);
  $("theme-note").hidden = current.board_can_change_look;
}

$("key-size-save").addEventListener("click", async (e) => {
  const picked = document.querySelector('input[name="key-size-choice"]:checked');
  const look = document.querySelector('input[name="theme-choice"]:checked');
  e.target.disabled = true;
  try {
    await saveDisplay({ key_size: picked ? picked.value : "medium", theme: look ? look.value : "dark" });
    $("setup-done").hidden = false;
  } catch (err) {
    logTo($("pair-log"), `Could not save the key size: ${err.message}`, "error");
  } finally {
    e.target.disabled = false;
  }
});

$("key-size").addEventListener("change", (e) => saveDisplay({ key_size: e.target.value }).catch((err) => alert(`Could not save the key size: ${err.message}`)));
$("theme").addEventListener("change", (e) => saveDisplay({ theme: e.target.value }).catch((err) => alert(`Could not change the look: ${err.message}`)));
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
  return `${chip} Unplug it, plug it in again, and look again; the assistant can help find out why.`;
}

$("identify").addEventListener("click", async () => {
  if (setup.task === "wifi") { step(4); scanWifi(); return; }
  const status = $("identify-status");
  status.hidden = false;
  status.className = "note";
  status.textContent = "Asking the board who it is (this restarts it for a moment)...";
  $("identify").disabled = true;
  try {
    const result = await api("/identify", { port: setup.port });
    const [firmware] = await Promise.all([api("/firmware").catch(() => null)]);
    setup.firmware = firmware;
    $("fw-source").textContent = firmware ? `${firmware.source} (version ${firmware.manifest.version})` : "unavailable";
    setup.identified = { chip: result.chip, chipError: result.chipError, running: result.running ? { firmware: result.running.firmware, profile: result.running.board?.profile } : null, candidates: result.candidates };
    const chip = result.chip;
    const running = result.running;
    $("chip-summary").textContent = [
      chip ? `${chip.chip} ${chip.revision || ""}, ${chip.flash_size} flash, MAC ${chip.mac || "?"}` : `Chip not identified: ${result.chipError}`,
      running ? `Running KnoEnd ${running.firmware} (${running.board.profile}).` : "Not running KnoEnd yet.",
    ].join(" ");
    $("erase").checked = !running;
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
    stage.textContent = `Installed KnoEnd ${p.version}. The board restarted.`;
    $("to-pair").disabled = false;
  } else if (p.stage === "failed") {
    stage.className = "note error";
    stage.textContent = `Install failed: ${p.error}`;
    $("flash-back").hidden = false;
    autoAssist(`Install failed: ${p.error}`);
  }
  if (p.text) logTo($("flash-log"), p.text);
}

$("to-pair").addEventListener("click", () => { step(4); scanWifi(); });

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

$("pair-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  $("pair-done").hidden = true;
  const ssid = (chosenNetwork && chosenNetwork !== "other" ? chosenNetwork.ssid : $("ssid").value).trim();
  const password = $("password").value;
  const button = e.submitter;
  if (setup.task === "wifi") {
    if (!ssid) { logTo($("pair-log"), "Pick a network, or type its name under Other network.", "error"); return; }
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
  logTo($("pair-log"), "Pairing over USB...");
  try {
    const result = await api("/provision", { port: setup.port, ssid: ssid || null, password: ssid ? password : null });
    logTo($("pair-log"), `Paired with board ${result.board}${result.ip ? ` on ${result.ip}` : ""}`);
    keySizeStep();
  } catch (err) {
    logTo($("pair-log"), `Pairing failed: ${err.message}`, "error");
    autoAssist(`Pairing failed: ${err.message}`);
  } finally {
    $("password").value = "";
    button.disabled = false;
  }
});

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
  pair: ["Go to pairing", () => { setTask("setup"); step(4); scanWifi(); }],
  change_wifi: ["Change Wi-Fi", () => { setTask("wifi"); step(1); show("setup"); }],
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

/** One line per change in a suggested fix. */
function describeFix(fix) {
  const lines = [];
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

async function ask(question, error) {
  if (!aiReady) {
    $("assistant-off").hidden = false;
    return;
  }
  const box = $("advice");
  box.hidden = false;
  $("advice-text").textContent = "Thinking...";
  $("advice-actions").replaceChildren();
  $("advice-fix").hidden = true;
  const context = { wizard_task: setup.task, wizard_step: setup.step, port: setup.port, chosen_board: setup.board,
                    identify_result: setup.identified, error: error || null, setup_mode: setup.mode,
                    ai_models_available: models.slice(0, 200) };
  try {
    const r = await api("/assist", { question, context });
    $("advice-text").textContent = r.answer;
    $("advice-sent").textContent = JSON.stringify(r.sent, null, 2);
    for (const id of r.actions) {
      if (!ACTIONS[id]) continue;
      const b = document.createElement("button");
      b.className = "secondary";
      b.textContent = ACTIONS[id][0];
      b.addEventListener("click", ACTIONS[id][1]);
      $("advice-actions").append(b);
    }
    if (r.fix) {
      $("advice-fix-list").replaceChildren(...describeFix(r.fix).map((line) => { const li = document.createElement("li"); li.textContent = line; return li; }));
      $("advice-fix-status").hidden = true;
      const apply = $("advice-fix-apply");
      apply.disabled = false;
      apply.onclick = () => applyFix(r.fix);
      $("advice-fix").hidden = false;
    }
  } catch (e) {
    $("advice-text").textContent = `The assistant could not answer: ${e.message}`;
  }
}

/** In guided mode, every failure goes to the assistant with what was happening. */
function autoAssist(error) {
  if (setup.mode === "ai" && aiReady) ask("", error);
}

$("ask-form").addEventListener("submit", (e) => {
  e.preventDefault();
  ask($("ask-input").value);
});

// ---- start ----

(async function start() {
  connectEvents();
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
})();
