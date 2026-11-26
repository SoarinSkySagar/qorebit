// Qorebit wallet background: opens the side panel from the toolbar button, and answers websites that use window.qorechain.
// A site's request is shown to the user in a small approval window (the wallet's own page); nothing is signed or shared
// without it, and a signature is made on the device after its button is pressed.
chrome.runtime.onInstalled.addListener(() => {
  chrome.sidePanel.setPanelBehavior({ openPanelOnActionClick: true }).catch(() => {});
});

const STATE = "qorebit.state"; // written by the wallet page: { address, index, accountName, net, chainId }
const CONNECTIONS = "qorebit.connections"; // { [origin]: connectedAt }
const requests = new Map(); // id -> { id, origin, method, params, windowId, resolve }
let counter = 0;

const err = (code, message) => ({ error: { code, message } });
const getState = async () => (await chrome.storage.local.get(STATE))[STATE] ?? null;
const connected = async (origin) => !!((await chrome.storage.local.get(CONNECTIONS))[CONNECTIONS] ?? {})[origin];
async function connect(origin) {
  const c = (await chrome.storage.local.get(CONNECTIONS))[CONNECTIONS] ?? {};
  c[origin] = Date.now();
  await chrome.storage.local.set({ [CONNECTIONS]: c });
}

function ask(origin, method, params) {
  return new Promise(async (resolve) => {
    if ([...requests.values()].some((r) => r.origin === origin)) return resolve(err(-32002, "a request from this site is already waiting in the wallet"));
    if (requests.size >= 5) return resolve(err(-32002, "the wallet has too many requests waiting"));
    const id = String(++counter) + "-" + crypto.randomUUID();
    const req = { id, origin, method, params, windowId: null, resolve };
    requests.set(id, req);
    try {
      const w = await chrome.windows.create({ url: chrome.runtime.getURL("index.html") + "#/approve/" + id, type: "popup", width: 420, height: 720, focused: true });
      req.windowId = w.id;
    } catch {
      requests.delete(id);
      resolve(err(4900, "could not open the wallet"));
    }
  });
}
chrome.windows.onRemoved.addListener((windowId) => {
  for (const r of requests.values()) {
    if (r.windowId === windowId) {
      requests.delete(r.id);
      r.resolve(err(4001, "the user closed the wallet without approving"));
    }
  }
});

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  (async () => {
    // from a website (through content.js)
    if (msg?.type === "qorebit:request" && sender.tab) {
      const origin = sender.origin ?? new URL(sender.url).origin;
      if (!/^https?:\/\//.test(origin)) return sendResponse(err(4100, "unsupported page"));
      const st = await getState();
      const isConn = await connected(origin);
      switch (msg.method) {
        case "qor_chainId":
          return sendResponse(st ? { result: st.chainId } : err(4900, "open the Qorebit wallet once first"));
        case "qor_accounts":
          return sendResponse({ result: isConn && st ? [st.address] : [] });
        case "qor_requestAccounts": {
          if (!st) return sendResponse(err(4900, "open the Qorebit wallet once first"));
          if (isConn) return sendResponse({ result: [st.address] });
          return sendResponse(await ask(origin, msg.method, []));
        }
        case "qor_signIn": {
          if (!isConn) return sendResponse(err(4100, "this site is not connected: call qor_requestAccounts first"));
          const m = msg.params?.[0]?.message;
          if (typeof m !== "string" || m.length > 4000) return sendResponse(err(-32602, "params[0].message must be the sign-in message"));
          return sendResponse(await ask(origin, msg.method, [{ message: m }]));
        }
        default:
          return sendResponse(err(4200, "unsupported method"));
      }
    }
    // from the wallet's own approval page (an extension page, not a tab)
    if (sender.url && sender.url.startsWith(chrome.runtime.getURL("")) && !sender.tab) {
      const r = requests.get(msg?.id);
      if (msg?.type === "qorebit:get") return sendResponse(r ? { origin: r.origin, method: r.method, params: r.params } : null);
      if (msg?.type === "qorebit:done" && r) {
        requests.delete(r.id);
        if (r.method === "qor_requestAccounts") {
          await connect(r.origin);
          r.resolve({ result: [msg.result.address] });
        } else {
          r.resolve({ result: msg.result });
        }
        if (r.windowId) chrome.windows.remove(r.windowId).catch(() => {});
        return sendResponse(true);
      }
      if (msg?.type === "qorebit:fail" && r) {
        requests.delete(r.id);
        r.resolve(err(msg.code ?? 4001, String(msg.message ?? "rejected")));
        if (r.windowId) chrome.windows.remove(r.windowId).catch(() => {});
        return sendResponse(true);
      }
      if (msg?.type === "qorebit:disconnect-all") {
        await chrome.storage.local.set({ [CONNECTIONS]: {} });
        return sendResponse(true);
      }
    }
    sendResponse(null);
  })();
  return true; // the answer is sent later
});
