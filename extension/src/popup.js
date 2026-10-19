// The popup: is knoend reachable, and are board buttons on for this site? Turning them on asks
// the browser for this one site's permission (spec/browser.md section 5).
const api = globalThis.browser ?? globalThis.chrome;
const $ = (id) => document.getElementById(id);

/** Help text with one command in it, built without innerHTML. */
function help(before, command, after) {
  const code = document.createElement("code");
  code.textContent = command;
  $("help").replaceChildren(before, code, after);
  $("help").hidden = false;
}

async function render() {
  const [tab] = await api.tabs.query({ active: true, currentWindow: true });
  const status = await api.runtime.sendMessage({ type: "status" }).catch(() => null);
  const link = $("link");
  if (status && status.service) { link.textContent = "connected"; link.className = "pill ok"; }
  else if (status && status.host) { link.textContent = "knoend not running"; link.className = "pill bad"; }
  else { link.textContent = "not set up"; link.className = "pill bad"; }
  if (!status || !status.host) {
    help("Run ", "knoend browser install", " once, then restart the browser.");
  } else if (!status.service) {
    help("Start it with ", "knoend serve", ".");
  }

  if (!tab || !/^https?:\/\//.test(tab.url || "")) {
    $("site").textContent = "No website in this tab";
    $("explain").textContent = "Open a website to use board buttons on it.";
    return;
  }
  const url = new URL(tab.url);
  const pattern = `${url.protocol}//${url.hostname}/*`;
  const on = await api.permissions.contains({ origins: [pattern] });
  $("site").textContent = url.hostname;
  $("explain").textContent = on
    ? "Board buttons are on here: the board can press buttons and control media on this site."
    : "The board can see that you are on this site, nothing more. Turn buttons on to let it act on the page.";
  const toggle = $("toggle");
  toggle.hidden = false;
  toggle.textContent = on ? "Turn off board buttons here" : "Turn on board buttons here";
  toggle.className = on ? "off" : "";
  const configure = $("configure");
  configure.hidden = !on || !(status && status.service);
  configure.onclick = async () => {
    configure.disabled = true;
    showConfigure(await api.runtime.sendMessage({ type: "configure" }));
    watchConfigure();
  };
  showConfigure(status && status.configure);
  toggle.onclick = async () => {
    try {
      if (on) await api.permissions.remove({ origins: [pattern] });
      else await api.permissions.request({ origins: [pattern] });
    } catch (e) {
      $("explain").textContent = `Could not change it: ${e.message}`;
    }
    render();
  };
}

function showConfigure(state) {
  const line = $("configure-status");
  if (!state) { line.hidden = true; return; }
  line.hidden = false;
  line.textContent = state.text;
  line.style.color = state.done && !state.ok ? "var(--bad)" : "";
  $("configure").disabled = !state.done;
}

/** While the AI works, check every second (the popup may stay open). */
function watchConfigure() {
  const timer = setInterval(async () => {
    const status = await api.runtime.sendMessage({ type: "status" }).catch(() => null);
    showConfigure(status && status.configure);
    if (!status || !status.configure || status.configure.done) clearInterval(timer);
  }, 1000);
}

render();
