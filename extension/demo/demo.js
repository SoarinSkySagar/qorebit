// A page using the Qorebit provider: connect, then ask the wallet to send QOR (the wallet shows it, the device signs it).
const $ = (id) => document.getElementById(id);
const show = (v) => ($("out").textContent = typeof v === "string" ? v : JSON.stringify(v, null, 2));
const fail = (e) => show(`error ${e.code ?? ""}: ${e.message ?? e}`);
let account = null;

function ready() {
  $("status").textContent = window.qorechain?.isQorebit ? "Qorebit wallet found." : "No Qorebit wallet found: install the extension and reload this page.";
}
if (window.qorechain) ready(); else window.addEventListener("qorechain#initialized", ready);
setTimeout(ready, 600);

$("connect").onclick = async () => {
  try {
    [account] = await window.qorechain.request({ method: "qor_requestAccounts" });
    const chainId = await window.qorechain.request({ method: "qor_chainId" });
    show({ account, chainId });
  } catch (e) { fail(e); }
};

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
