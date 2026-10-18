// Asks the browser for one site's permission after a click here (browsers only accept a permission
// request from a click), then tells the background script, which returns to the page.
const api = globalThis.browser ?? globalThis.chrome;
const params = new URLSearchParams(location.search);
const origin = params.get("origin");
const request = params.get("request");
document.getElementById("site").textContent = origin ? new URL(origin).hostname : "this site";

function finish(granted) {
  api.runtime.sendMessage({ type: "grant-result", request, granted });
}
document.getElementById("allow").addEventListener("click", async () => {
  let granted = false;
  try { granted = await api.permissions.request({ origins: [`${origin}/*`] }); }
  catch (e) { document.getElementById("note").textContent = `Could not ask: ${e.message}`; }
  finish(granted);
});
document.getElementById("deny").addEventListener("click", () => finish(false));
