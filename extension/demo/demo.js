// A page using the Qorebit provider: sign in with QoreChain (connect + a signed sign-in message), then ask the wallet to send QOR
// (the wallet shows it, the device signs it). A real site verifies the sign-in signature on its server with `verifySiwq` from
// @qorechain/qoresign (both the secp256k1 and the ML-DSA-87 signature); this page only shows what it got back.
const $ = (id) => document.getElementById(id);
const show = (v) => ($("out").textContent = typeof v === "string" ? v : JSON.stringify(v, null, 2));
const fail = (e) => show(`error ${e.code ?? ""}: ${e.message ?? e}`);
const STATEMENT = "Sign in to the Qorebit demo site";
let account = null;

function ready() {
  $("status").textContent = window.qorechain?.isQorebit ? "Qorebit wallet found." : "No Qorebit wallet found: install the extension and reload this page.";
}
if (window.qorechain) ready(); else window.addEventListener("qorechain#initialized", ready);
setTimeout(ready, 600);

// the sign-in message in the QoreChain (SIWQ) layout, as createSiwqMessage from @qorechain/qoresign writes it
function siwqMessage({ domain, address, statement, uri, chainId, nonce, issuedAt, expirationTime }) {
  return [
    `${domain} wants you to sign in with your QoreChain account:`, address, "", statement, "",
    `URI: ${uri}`, "Version: 1", `Chain ID: ${chainId}`, `Nonce: ${nonce}`, `Issued At: ${issuedAt}`, `Expiration Time: ${expirationTime}`,
  ].join("\n");
}
const nonce = () => Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => "abcdefghijklmnopqrstuvwxyz0123456789"[b % 36]).join("");

function signedIn(on) {
  $("send").disabled = !on;
  $("signout").style.display = on ? "" : "none";
  $("signin").textContent = on ? "Signed in: sign in again" : "Sign in with QoreChain";
  $("who").textContent = on ? `Signed in as ${account}` : "";
}

$("signin").onclick = async () => {
  try {
    // 1. connect (the wallet asks the first time) and find out the network
    [account] = await window.qorechain.request({ method: "qor_requestAccounts" });
    const chainId = await window.qorechain.request({ method: "qor_chainId" });
    // 2. the sign-in message: this site, this account, a fresh nonce, a short expiry
    const now = new Date();
    const message = siwqMessage({
      domain: location.host, address: account, statement: STATEMENT, uri: `${location.origin}/`, chainId, nonce: nonce(),
      issuedAt: now.toISOString(), expirationTime: new Date(now.getTime() + 10 * 60_000).toISOString(),
    });
    // 3. the wallet shows it, the device shows it and signs it with both keys
    const signature = await window.qorechain.request({ method: "qor_signIn", params: [{ message }] });
    signedIn(true);
    show({ signedInAs: account, chainId, message, signature: { ...signature, pq: { ...signature.pq, signature: signature.pq.signature.slice(0, 40) + "… (" + signature.pq.signature.length + " base64 chars)" } } });
  } catch (e) { signedIn(false); fail(e); }
};

$("signout").onclick = () => { account = null; signedIn(false); show("signed out"); };

$("send").onclick = async () => {
  try {
    const uqor = BigInt(Math.round(Number($("amount").value) * 1e6)).toString();
    const result = await window.qorechain.request({
      method: "qor_sendTransaction",
      params: [{
        messages: [{ typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: { toAddress: $("to").value.trim(), amount: [{ denom: "uqor", amount: uqor }] } }],
        memo: $("memo").value,
      }],
    });
    show(result);
  } catch (e) { fail(e); }
};

// ---- QoreX: the same site, a different wallet. QoreX injects window.qorex (never window.keplr). signArbitrary signs a message with the account's
// classical key (ADR-36) and with ML-DSA-87; a server verifies both with verifyMessageWithChain from @qorechain/qoresign. This page only shows it.
const QOREX_CHAIN = "qorechain-diana";
const qorex = () => (typeof window.qorex?.native?.signArbitrary === "function" ? window.qorex.native : null);
function qorexReady() {
  $("qorex-status").textContent = qorex() ? "QoreX found." : "QoreX not found: install the QoreX extension and reload this page.";
  $("qorex-signin").disabled = !qorex();
}
qorexReady();
for (const ms of [300, 900, 2000, 4000]) setTimeout(qorexReady, ms);

$("qorex-signin").onclick = async () => {
  try {
    const native = qorex();
    if (!native) throw new Error("QoreX not found");
    await native.enable(QOREX_CHAIN); // one approval per site: it only reveals the public address
    const { bech32Address } = await native.getKey(QOREX_CHAIN);
    const message = `Sign in to the Qorebit demo site (${location.host})\nAccount: ${bech32Address}\nNonce: ${nonce()}\nIssued At: ${new Date().toISOString()}`;
    const signature = await native.signArbitrary(QOREX_CHAIN, bech32Address, message);
    const cut = (v) => (typeof v === "string" && v.length > 60 ? v.slice(0, 40) + `… (${v.length} chars)` : v);
    show({ signedInWith: "QoreX", account: bech32Address, chainId: QOREX_CHAIN, message, signature: JSON.parse(JSON.stringify(signature, (_k, v) => cut(v))) });
  } catch (e) { fail(e); }
};
