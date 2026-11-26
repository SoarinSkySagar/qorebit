// Tests extension/background.js (the provider's brain) against a mock of the chrome.* API:
// connect needs an approval, the origin comes from the browser, sign-in needs a connection, one request per site.
import { readFileSync } from "node:fs";
import vm from "node:vm";
import assert from "node:assert/strict";
// objects made inside the vm context are not reference-equal to ours: compare as JSON
const same = (a, b, m) => assert.equal(JSON.stringify(a), JSON.stringify(b), m);

const store = {};
const listeners = { msg: null, removed: [] };
const opened = [];
const chrome = {
  runtime: { onInstalled: { addListener() {} }, getURL: (p) => "chrome-extension://abc/" + p, onMessage: { addListener: (f) => (listeners.msg = f) } },
  sidePanel: { setPanelBehavior: async () => {} },
  storage: { local: { get: async (k) => ({ [k]: store[k] }), set: async (o) => Object.assign(store, o) } },
  windows: { create: async (o) => { opened.push(o.url); return { id: opened.length }; }, remove: async () => {}, onRemoved: { addListener: (f) => listeners.removed.push(f) } },
};
vm.runInNewContext(readFileSync("extension/background.js", "utf8"), { chrome, crypto: globalThis.crypto, URL, console, Date, Promise, setInterval, clearInterval });
const call = (msg, sender) => new Promise((res) => listeners.msg(msg, sender, res));
const site = { tab: { id: 1 }, origin: "https://app.example.com", url: "https://app.example.com/x" };
// the approval window is a popup: the browser gives its messages a tab too
const ext = { url: "chrome-extension://abc/index.html#/approve/x", tab: { id: 9 } };
const request = (method, params = []) => call({ type: "qorebit:request", method, params }, site);

await chrome.storage.local.set({ "qorebit.state": { address: "qor1abc", index: 0, accountName: "Account 1", net: "testnet", chainId: "qorechain-diana" } });
assert.equal((await request("qor_chainId")).result, "qorechain-diana");
same((await request("qor_accounts")).result, []);
assert.equal((await request("qor_signIn", [{ message: "m" }])).error.code, 4100, "sign-in needs a connection first");
assert.equal((await request("qor_sendEverything")).error.code, 4200, "unknown methods are refused");
assert.equal(await call({ type: "qorebit:request", method: "qor_accounts" }, { url: "https://x.test/" }), null, "a message without a tab is not a website");

// connect: opens the approval window; a page message cannot answer it, only the wallet's own page
const pending = request("qor_requestAccounts");
await new Promise((r) => setTimeout(r, 10));
assert.equal(opened.length, 1);
const id = /#\/approve\/(.+)$/.exec(opened[0])[1];
assert.equal((await request("qor_requestAccounts")).error.code, -32002, "one waiting request per site");
assert.equal(await call({ type: "qorebit:done", id, result: { address: "evil" } }, site), null, "a website cannot approve its own request");
assert.equal(await call({ type: "qorebit:request", method: "qor_accounts" }, ext), null, "a wallet page is not a website");
same(await call({ type: "qorebit:get", id }, ext), { origin: "https://app.example.com", method: "qor_requestAccounts", params: [] });
await call({ type: "qorebit:done", id, result: { address: "qor1abc" } }, ext);
same((await pending).result, ["qor1abc"]);
same((await request("qor_accounts")).result, ["qor1abc"], "connected now");
same((await request("qor_requestAccounts")).result, ["qor1abc"], "no second prompt");

// sign in: the message goes to the approval page unchanged; a rejection comes back as 4001
const si = request("qor_signIn", [{ message: "the message" }]);
await new Promise((r) => setTimeout(r, 10));
const id2 = /#\/approve\/(.+)$/.exec(opened[1])[1];
assert.equal((await call({ type: "qorebit:get", id: id2 }, ext)).params[0].message, "the message");
await call({ type: "qorebit:fail", id: id2, code: 4001, message: "rejected" }, ext);
assert.equal((await si).error.code, 4001);

// closing the window rejects
const si2 = request("qor_signIn", [{ message: "m2" }]);
await new Promise((r) => setTimeout(r, 10));
listeners.removed.forEach((f) => f(opened.length));
assert.equal((await si2).error.code, 4001);
assert.equal((await request("qor_signIn", [{ message: "x".repeat(5000) }])).error.code, -32602, "a huge message is refused");
// send a transaction: needs a connection, exactly one message, and only {messages, memo} reach the approval window
const tx = { messages: [{ typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: { toAddress: "qor1x", amount: [{ denom: "uqor", amount: "1" }] } }], memo: "m", sneaky: "x" };
const other = { tab: { id: 2 }, origin: "https://other.example", url: "https://other.example/" };
assert.equal((await call({ type: "qorebit:request", method: "qor_sendTransaction", params: [tx] }, other)).error.code, 4100, "an unconnected site cannot ask");
assert.equal((await request("qor_sendTransaction", [{ messages: [] }])).error.code, -32602, "one message exactly");
assert.equal((await request("qor_sendTransaction", [{ messages: [tx.messages[0], tx.messages[0]] }])).error.code, -32602);
assert.equal((await request("qor_sendTransaction", [])).error.code, -32602);
const st = request("qor_sendTransaction", [tx]);
await new Promise((r) => setTimeout(r, 10));
const id3 = /#\/approve\/(.+)$/.exec(opened[opened.length - 1])[1];
const shown = await call({ type: "qorebit:get", id: id3 }, ext);
same(Object.keys(shown.params[0]).sort(), ["memo", "messages"], "extra fields are dropped");
await call({ type: "qorebit:done", id: id3, result: { hash: "ABC", height: "5" } }, ext);
same((await st).result, { hash: "ABC", height: "5" });
console.log("extension provider: ok");
