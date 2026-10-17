// OpenPager web app: talks only to the local service that served it.
// Every API call carries the session token the service put in this page.

const TOKEN = document.querySelector('meta[name="openpager-token"]').content;
const $ = (id) => document.getElementById(id);

async function api(path, body) {
  const options = { headers: { "x-openpager-token": TOKEN } };
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
}
document.querySelectorAll(".tabs button").forEach((b) => b.addEventListener("click", () => show(b.dataset.view)));
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
  $("now-app").textContent = s.app || "-";
  const layout = s.link === "connected" ? s.layout : null;
  $("now-layout").textContent = layout ? layout.name : "-";
  const badge = $("now-badge");
  badge.hidden = !layout || layout.reviewed;
  badge.textContent = "AI draft, unreviewed";

  const keys = $("keys-preview");
  keys.replaceChildren();
  for (let slot = 1; slot <= 9; slot++) {
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
    if (d.text && event.kind !== "status") logTo($("activity"), d.text.trim(), event.kind === "configure-failed" ? "error" : "");
  };
  source.onerror = () => {
    $("link-pill").textContent = "Service not reachable";
    $("link-pill").className = "pill warn";
  };
}

// ---- setup wizard ----

const setup = { step: 1, port: null, board: null, firmware: null, identified: null, mode: null };

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
      running ? `Running OpenPager ${running.firmware} (${running.board.profile}).` : "Not running OpenPager yet.",
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
    stage.textContent = `Installed OpenPager ${p.version}. The board restarted.`;
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
  button.disabled = true;
  logTo($("pair-log"), "Pairing over USB...");
  try {
    const result = await api("/provision", { port: setup.port, ssid: ssid || null, password: ssid ? password : null });
    logTo($("pair-log"), `Paired with board ${result.board}${result.ip ? ` on ${result.ip}` : ""}`);
    $("pair-done").hidden = false;
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
  storeSet("openpager.mode", mode);
  document.querySelectorAll('input[name="mode"]').forEach((i) => (i.checked = i.value === mode));
  $("ai-form").hidden = mode !== "ai";
}
document.querySelectorAll('input[name="mode"]').forEach((i) => i.addEventListener("change", () => setMode(i.value)));

function providerFields() {
  const provider = $("ai-provider").value;
  $("ai-base-wrap").hidden = provider !== "openai-compatible";
  $("ai-base-alt-wrap").hidden = provider === "openai-compatible";
  $("ai-key-wrap").hidden = provider === "ollama";
}
/** The API address: the main field for a generic server, the advanced one otherwise. */
function baseUrl() {
  return ($("ai-provider").value === "openai-compatible" ? $("ai-base").value : $("ai-base-alt").value).trim() || null;
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

async function loadAi() {
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
  pair: ["Go to pairing", () => { step(4); scanWifi(); }],
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
        $("advice-text").textContent = "Typing is allowed now. Press a key on the board to try it; if nothing is typed, restart openpager once.";
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
  const context = { wizard_step: setup.step, port: setup.port, chosen_board: setup.board,
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
  setMode(storeGet("openpager.mode") || "ai"); // guided is the recommended default
})();
