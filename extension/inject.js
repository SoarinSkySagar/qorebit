// Qorebit wallet: the page-side provider, `window.qorechain` (runs in the page's own world, so a site can call it).
// It can do three things, each asked for through the extension and approved by the user:
//   request({ method: "qor_requestAccounts" })           -> ["qor1…"]   (asks to connect this site)
//   request({ method: "qor_accounts" })                  -> ["qor1…"] or []   (no prompt)
//   request({ method: "qor_chainId" })                   -> "qorechain-…"
//   request({ method: "qor_signIn", params: [{ message }] }) -> the SIWQ signature (ml-dsa-87-hybrid), signed on the device
// It never sees a key: the device signs, and only after the user presses the button on it.
(() => {
  if (window.qorechain) return;
  const pending = new Map();
  let next = 1;
  window.addEventListener("message", (e) => {
    const d = e.data;
    if (e.source !== window || !d || d.target !== "qorebit-page") return;
    const p = pending.get(d.id);
    if (!p) return;
    pending.delete(d.id);
    if (d.error) p.reject(Object.assign(new Error(d.error.message), { code: d.error.code }));
    else p.resolve(d.result);
  });
  const provider = {
    isQorebit: true,
    request(args) {
      if (!args || typeof args.method !== "string") return Promise.reject(Object.assign(new Error("request needs a method"), { code: -32600 }));
      return new Promise((resolve, reject) => {
        const id = next++;
        pending.set(id, { resolve, reject });
        window.postMessage({ target: "qorebit-content", id, method: args.method, params: args.params ?? [] }, "*");
      });
    },
  };
  Object.defineProperty(window, "qorechain", { value: Object.freeze(provider), configurable: false, enumerable: true });
  window.dispatchEvent(new Event("qorechain#initialized"));
})();
