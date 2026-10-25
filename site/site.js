// Kn°End project site: colour scheme, the board in the hero, the video gallery.

// ---- Videos ----
// To publish a video, put its YouTube ID (the part after "v=" in the link) in `id`.
// Until then the slot shows "Coming soon". Videos play from youtube-nocookie.com, and only after a click.
const VIDEOS = [
  { id: "d6Set7CRCjE", title: "Kn°End demo: keys that follow you", note: "Build North demo" },
  { id: "ENpDMOkb7OI", title: "Linux and MuseScore", note: "Sing a note, it lands in the score" },
  { id: "iCj722lLwPk", title: "Terminal coding agents", note: "Answer Claude Code from the board" },
  { id: "y1Sh5DLingc", title: "Contextual keys", note: "Keys that change with the app and site" },
];

// ---- Colour scheme (Auto / Light / Dark), remembered by this browser ----
function setScheme(choice) {
  if (choice === "light" || choice === "dark") document.documentElement.dataset.theme = choice;
  else delete document.documentElement.dataset.theme;
  try { localStorage.setItem("knoend-scheme", choice); } catch {}
  for (const b of document.querySelectorAll(".scheme button")) b.setAttribute("aria-pressed", String(b.dataset.scheme === choice));
}
let saved = "auto";
try { saved = localStorage.getItem("knoend-scheme") || "auto"; } catch {}
setScheme(saved);
for (const b of document.querySelectorAll(".scheme button")) b.addEventListener("click", () => setScheme(b.dataset.scheme));

// ---- The board: layouts it switches between as the focused app changes ----
const LAYOUTS = [
  { app: "VS Code", bar: "VS Code · Wi-Fi", keys: ["Command\nPalette", "Go to\nFile", "Find in\nFiles", "Terminal", "Run", "Debug", "Split", "Format", "Commit"] },
  { app: "YouTube", bar: "youtube.com · Wi-Fi", keys: ["Play /\nPause", "Back 10s", "Fwd 10s", "Captions", "Theatre", "Full\nscreen", "Speed −", "Speed +", "Mute"] },
  { app: "Claude Code", bar: "Claude Code wants to run: cargo test", keys: [{ t: "Allow" }, { t: "Always" }, { t: "Deny", risky: true }, "Stop", "Explain", "", "", "", ""] },
  { app: "PowerPoint", bar: "Slide 3/5 · notes 1/2", notes: "The companion service watches which app is in front and pushes the matching keys to every board…", keys: ["Prev", "Next", "Black"] },
];
const keysEl = document.getElementById("keys");
const barEl = document.getElementById("bar");
const appEl = document.getElementById("fig-app");
function show(layout) {
  keysEl.replaceChildren();
  if (layout.notes) {
    const n = document.createElement("div");
    n.className = "notes"; n.textContent = layout.notes;
    keysEl.append(n);
  }
  layout.keys.forEach((k, i) => {
    const d = document.createElement("div");
    const key = typeof k === "string" ? { t: k } : k;
    d.textContent = key.t;
    d.style.whiteSpace = "pre-line";
    d.style.animationDelay = `${i * 35}ms`;
    if (!key.t) d.className = "blank";
    if (key.risky) d.classList.add("risky");
    keysEl.append(d);
  });
  barEl.textContent = layout.bar;
  appEl.textContent = layout.app;
}
let at = 0;
show(LAYOUTS[0]);
if (!matchMedia("(prefers-reduced-motion: reduce)").matches) {
  setInterval(() => { at = (at + 1) % LAYOUTS.length; show(LAYOUTS[at]); }, 3200);
}

// ---- Video gallery ----
const gallery = document.getElementById("gallery");
VIDEOS.forEach((v, i) => {
  const fig = document.createElement("figure");
  fig.className = "video";
  const frame = document.createElement("div");
  frame.className = "frame";
  const play = document.createElement("button");
  play.type = "button";
  play.className = "play";
  const disc = document.createElement("span"); disc.className = "disc";
  const label = document.createElement("span"); label.className = "label";
  play.append(disc, label);
  const id = /^[\w-]{11}$/.test(v.id) ? v.id : "";
  if (id) {
    const thumb = document.createElement("img");
    thumb.src = `https://i.ytimg.com/vi/${id}/hqdefault.jpg`;
    thumb.alt = ""; thumb.loading = "lazy";
    frame.append(thumb);
    label.textContent = "Play";
    play.setAttribute("aria-label", `Play: ${v.title}`);
    play.addEventListener("click", () => {
      const f = document.createElement("iframe");
      f.src = `https://www.youtube-nocookie.com/embed/${id}?autoplay=1&rel=0`;
      f.title = v.title;
      f.allow = "accelerometer; autoplay; encrypted-media; gyroscope; picture-in-picture; fullscreen";
      f.allowFullscreen = true;
      frame.replaceChildren(f);
    });
  } else {
    label.textContent = "Coming soon";
    play.disabled = true;
  }
  frame.append(play);
  const cap = document.createElement("figcaption");
  const n = document.createElement("span"); n.className = "fig"; n.textContent = `Video ${i + 1}`;
  cap.append(n, document.createTextNode(v.title));
  const sub = document.createElement("div"); sub.className = "muted small"; sub.textContent = v.note;
  cap.append(sub);
  fig.append(frame, cap);
  gallery.append(fig);
});
