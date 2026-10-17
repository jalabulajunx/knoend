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

const setup = { step: 1, port: null, board: null, firmware: null };

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

async function scanPorts() {
  const list = $("ports");
  list.replaceChildren();
  $("identify").disabled = true;
  try {
    const { ports } = await api("/ports");
    if (!ports.length) {
      const li = document.createElement("li");
      li.className = "muted";
      li.textContent = "No board found. Check the cable carries data (not charge-only), then look again.";
      list.append(li);
    }
    for (const p of ports) {
      const usb = `${p.vid.toString(16).padStart(4, "0")}:${p.pid.toString(16).padStart(4, "0")}`;
      const { li, input } = choice("port", p.name, p.name, `${p.product || p.bridge} - USB ${usb}`);
      input.addEventListener("change", () => { setup.port = p.name; $("identify").disabled = false; });
      list.append(li);
      if (ports.length === 1) { input.checked = true; setup.port = p.name; $("identify").disabled = false; }
    }
  } catch (e) {
    list.innerHTML = "";
    logTo($("activity"), `Could not list ports: ${e.message}`, "error");
  }
}
$("rescan").addEventListener("click", scanPorts);

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
  candidates.forEach((c, index) => {
    const supported = c.status === "supported";
    const sub = (supported ? "" : "Firmware coming soon. ") + c.reasons.join("; ");
    const { li, input } = choice("board", c.board, c.name, sub, !supported);
    input.addEventListener("change", () => { setup.board = c.board; $("install").disabled = false; });
    list.append(li);
    if (index === 0 && supported) { input.checked = true; setup.board = c.board; $("install").disabled = false; }
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
  }
  if (p.text) logTo($("flash-log"), p.text);
}

$("to-pair").addEventListener("click", () => step(4));

$("pair-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  $("pair-done").hidden = true;
  const ssid = $("ssid").value.trim();
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
  } finally {
    $("password").value = "";
    button.disabled = false;
  }
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
})();
