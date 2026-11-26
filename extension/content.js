// Qorebit wallet: relays the page's requests (window.qorechain) to the extension's background and the answer back.
// It forwards only the method and params; the background learns the site from the browser (sender.origin), never from the page.
const METHODS = new Set(["qor_requestAccounts", "qor_accounts", "qor_chainId", "qor_signIn", "qor_sendTransaction"]);
window.addEventListener("message", (e) => {
  const d = e.data;
  if (e.source !== window || !d || d.target !== "qorebit-content" || typeof d.id !== "number") return;
  const reply = (r) => window.postMessage({ target: "qorebit-page", id: d.id, ...r }, "*");
  if (!METHODS.has(d.method)) return reply({ error: { code: 4200, message: `Qorebit does not support ${String(d.method).slice(0, 40)}` } });
  if (JSON.stringify(d.params ?? []).length > 8192) return reply({ error: { code: -32602, message: "request too large" } });
  (globalThis.browser ?? globalThis.chrome).runtime.sendMessage({ type: "qorebit:request", method: d.method, params: d.params ?? [] }).then(
    (r) => reply(r ?? { error: { code: -32603, message: "no answer from the wallet" } }),
    () => reply({ error: { code: 4900, message: "the Qorebit wallet is not available" } }),
  );
});
