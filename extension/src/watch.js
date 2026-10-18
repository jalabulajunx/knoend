// Runs on sites the person allowed: tells the extension when the page goes full screen or leaves
// it (a presentation starting, a video going full screen), so the board can switch keys.
(() => {
  const api = globalThis.browser ?? globalThis.chrome;
  const report = () => {
    try { api.runtime.sendMessage({ type: "page-state", fullscreen: !!document.fullscreenElement }); } catch { /* reloading */ }
  };
  document.addEventListener("fullscreenchange", report);
  report();
})();
