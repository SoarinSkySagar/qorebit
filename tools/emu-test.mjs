#!/usr/bin/env node
// End-to-end wallet test on the EMULATOR against a real QoreChain node (the local devnet): the
// device firmware signs, the chain verifies. Everything goes through the bridge's HTTP API, as the
// companion app does, while this script answers the device's screens with taps and holds of its one
// button (DISPLAY_INPUT).
//
//   make test-emu        (starts a fresh emulator with a known wallet + headless bridge, runs this)
//   node tools/emu-test.mjs --entropy <hex> [--bridge 7620] [--http 7630]   (an emulator you started with a state file from make-emu-state.mjs)
//
// Refuses to run against a real board: it would approve signatures on it. Needs the devnet.
import { mnemonicToEntropy, entropyToMnemonic } from "@scure/bip39";
import { wordlist } from "@scure/bip39/wordlists/english.js";
import {
  addressFromPublicKey, deriveCanonicalPqKey, deriveClassicalKey, signHybrid, pqVerify, verifyHybridTx, decodeTxRaw,
  encodeAny, toHex, fromHex,
} from "@qorechain/qoresign";
import { broadcastTx, fetchPqKeyStatus, resolveSignBytesVersion } from "@qorechain/qoresign/network";
import { accountInfo, balance, buildTx, restOf, sendMsg, waitTx } from "./qorelib.mjs";
import net from "node:net";

const arg = (name, dflt) => Number(process.argv[process.argv.indexOf(name) + 1]) || dflt;
const BRIDGE = arg("--bridge", 7620), HTTP = arg("--http", 7630);
const CHAIN_ID = "qorechain-local";
const rest = restOf("devnet");
const ENTROPY = process.argv[process.argv.indexOf("--entropy") + 1]; // the wallet make-emu-state.mjs put in the emulator
if (!/^[0-9a-f]{64}$/i.test(ENTROPY ?? "")) { console.error("usage: node tools/emu-test.mjs --entropy <64 hex digits> [--bridge 7620] [--http 7630]"); process.exit(2); }
const MNEMONIC = entropyToMnemonic(fromHex(ENTROPY), wordlist);
const HOLD = 700;

// ---------------------------------------------------------------- the device's screen and its one button

const framed = (body) => { const h = Buffer.alloc(4); h.writeUInt32BE(body.length); return Buffer.concat([h, body]); };
function rawFrame(cmd, payload = Buffer.alloc(0)) {
  return new Promise((resolve, reject) => {
    const s = net.createConnection(BRIDGE, "127.0.0.1");
    let buf = Buffer.alloc(0);
    s.on("data", (d) => {
      buf = Buffer.concat([buf, d]);
      if (buf.length >= 4 && buf.length >= 4 + buf.readUInt32BE(0)) { s.end(); resolve({ status: buf[5], payload: buf.subarray(6, 4 + buf.readUInt32BE(0)) }); }
    });
    s.on("error", reject);
    s.write(framed(Buffer.concat([Buffer.from([0x51, cmd]), payload])));
  });
}
const poll = async () => {
  const r = await rawFrame(0x30);
  const p = r.payload;
  return { id: p.readUInt32BE(4), kind: p[8], holdMs: p.readUInt16BE(12) };
};
const pressOn = async (id, heldMs) => {
  const b = Buffer.alloc(8); b.writeUInt32BE(id, 0); b.writeUInt32BE(heldMs, 4);
  return rawFrame(0x31, b);
};

/** What the script does when the device asks: hold or tap on an approval, and the PIN to type. */
const script = { approval: [], pins: [], other: HOLD, seen: [] };
let driving = false;
const PIN = 0b1011_0010;
async function drive() {
  let last = -1, pinBits = 0, pin = null;
  while (driving) {
    await new Promise((r) => setTimeout(r, 25));
    let s;
    try { s = await poll(); } catch { continue; }
    if (s.id === last || s.kind === 0 || s.kind === 2) continue;
    last = s.id;
    if (s.kind === 1) {
      const held = script.approval.length ? script.approval.shift() : HOLD;
      script.seen.push(`approval:${held}`);
      await pressOn(s.id, held);
    } else if (s.kind === 3) {
      if (pin === null || pinBits === 8) { pin = script.pins.length ? script.pins.shift() : PIN; pinBits = 0; }
      const bit = (pin >> (7 - pinBits)) & 1;
      pinBits++;
      await pressOn(s.id, bit ? HOLD : 0);
    } else {
      await pressOn(s.id, script.other);
    }
  }
}

// ---------------------------------------------------------------- bridge API

async function call(method, params = {}) {
  const r = await fetch(`http://127.0.0.1:${HTTP}/rpc`, { method: "POST", body: JSON.stringify({ method, params }) });
  const j = await r.json();
  if (j.error) { const e = new Error(j.error.message); e.code = j.error.code; throw e; }
  return j.result;
}
const expectError = async (what, fn, pattern) => {
  try { await fn(); } catch (e) { if (pattern.test(e.message)) return log(`ok   ${what}: ${e.message}`); throw new Error(`${what}: wrong error: ${e.message}`); }
  throw new Error(`${what}: expected an error`);
};
let passed = 0;
const log = (s) => { passed += s.startsWith("ok") ? 1 : 0; console.log(s); };
const check = (cond, what) => { if (!cond) throw new Error(`FAILED: ${what}`); log(`ok   ${what}`); };

// ---------------------------------------------------------------- the software wallet with the same words

const classical = (index) => deriveClassicalKey({ mnemonic: MNEMONIC, index });
const addr = (index) => addressFromPublicKey(classical(index).publicKey, "secp256k1").bech32;
const pq = (index) => deriveCanonicalPqKey({ mnemonic: MNEMONIC, address: addr(index) });

// a protobuf MsgRegisterPQCKeyV2 with the device's proof (the device never sees the message first)
const varint = (n) => { const o = []; let v = BigInt(n); do { let b = Number(v & 0x7fn); v >>= 7n; if (v) b |= 0x80; o.push(b); } while (v); return Uint8Array.from(o); };
const lenField = (f, b) => b.length ? Buffer.concat([Buffer.from(varint(f << 3 | 2)), Buffer.from(varint(b.length)), Buffer.from(b)]) : Buffer.alloc(0);
const registerMsg = (account, pk, classicalPk, proof) => ({
  typeUrl: "/qorechain.pqc.v1.MsgRegisterPQCKeyV2",
  value: Buffer.concat([lenField(1, Buffer.from(account)), lenField(2, pk), Buffer.from([0x18, 1]), lenField(4, classicalPk), lenField(5, Buffer.from("hybrid")), lenField(6, proof)]),
});

async function broadcast(txBytes) {
  const r = await broadcastTx({ rest, txBytes });
  if (r.code !== 0) throw new Error(`the chain refused the transaction (code ${r.code} ${r.codespace}): ${r.log}`);
  const done = await waitTx(rest, r.txhash);
  if (done.code !== 0) throw new Error(`failed on chain (code ${done.code}): ${done.raw_log}`);
  return r.txhash;
}

// ---------------------------------------------------------------- the test

const info = (await call("device.info")).text;
if (!/emulator|host file/.test(info)) { console.error("this device is not the emulator: refusing to approve signatures on it"); process.exit(2); }
console.log(info.split("\n").slice(0, 5).join("\n"));
driving = true;
const driver = drive();
try {
  // 1. PIN
  script.pins.push(PIN, PIN);
  await call("device.setPin");
  check((await call("device.security")).pinSet, "the signing PIN is set (tap = 0, hold = 1, eight presses, twice)");

  // 2. the device's accounts are the ones QoreSign derives from the same words
  for (const i of [0, 1, 2]) {
    const a = await call("device.account", { account: i });
    check(a.bech32 === addr(i) && a.publicKey === "0x" + toHex(classical(i).publicKey), `account ${i + 1}: address and key match QoreSign (${a.bech32})`);
    const k = await call("device.pqPublicKey", { account: i });
    check(k.publicKey === "0x" + toHex(pq(i).publicKey), `account ${i + 1}: ML-DSA-87 public key matches QoreSign`);
  }
  const me = addr(0), you = addr(1);

  // 3. funds from the devnet faucet, through the bridge
  const f = await fetch(`http://127.0.0.1:${HTTP}/devnet/fund`, { method: "POST", body: JSON.stringify({ address: me, amount: 20_000_000 }) }).then((r) => r.json());
  check(!f.error, `the faucet funded account 1 (${f.result?.result ?? f.error?.message})`);
  const info0 = await accountInfo(rest, me);
  check(info0 && (await balance(rest, me)) >= 20_000_000n, "the chain knows the account and its balance");
  await call("device.home", { account: 0, balance: String(await balance(rest, me)), chainId: CHAIN_ID, address: me });

  // 4. key registration: the device proves the key, the host builds the message, the device signs classical-only
  check(!(await fetchPqKeyStatus({ rest, address: me })).registered, "the post-quantum key is not registered yet");
  const proof = await call("device.registerProof", { account: 0, chainId: CHAIN_ID });
  const pk0 = fromHex((await call("device.pqPublicKey", { account: 0 })).publicKey);
  const msg = registerMsg(me, pk0, classical(0).publicKey, fromHex(proof.proof));
  const reg = buildTx({ messages: [{ typeUrl: msg.typeUrl, value: msg.value }], publicKey: classical(0).publicKey, sequence: info0.sequence });
  const version = await resolveSignBytesVersion({ rest, chainId: CHAIN_ID });
  const signedReg = await call("device.signTx", {
    account: 0, accountNumber: String(info0.accountNumber), chainId: CHAIN_ID, body: toHex(reg.bodyBytes), authInfo: toHex(reg.authInfoBytes),
    classicalOnly: true, signBytes: version,
  });
  check(signedReg.pqSignature === null, "a key registration is signed classical-only");
  await broadcast(fromHex(signedReg.txBytes));
  const status = await fetchPqKeyStatus({ rest, address: me });
  check(status.registered && toHex(status.publicKey) === toHex(pk0), "the chain accepted the registration: the device's key is registered, with its proof of possession");

  // 5. a hybrid transfer: the chain verifies both signatures
  const sendOnce = async (amount, extra = {}) => {
    const acc = await accountInfo(rest, me);
    const tx = buildTx({ messages: [sendMsg(me, you, amount)], publicKey: classical(0).publicKey, sequence: acc.sequence });
    const params = { account: 0, accountNumber: String(acc.accountNumber), chainId: CHAIN_ID, body: toHex(tx.bodyBytes), authInfo: toHex(tx.authInfoBytes), signBytes: version, ...extra };
    return { tx, acc, params, signed: await call("device.signTx", params) };
  };
  const before = await balance(rest, you);
  const { tx, acc, signed } = await sendOnce(1_500_000);
  check(signed.pqSignature && fromHex(signed.pqSignature).length === 4627, "a transfer is signed with both an ML-DSA-87 and a secp256k1 signature");
  // the software wallet signs the same transaction to the same bytes: deterministic signatures
  const soft = signHybrid({ chainId: CHAIN_ID, accountNumber: acc.accountNumber, bodyBytes: tx.bodyBytes, authInfoBytes: tx.authInfoBytes, classicalKey: classical(0), pqKey: pq(0), signBytesVersion: version });
  check(signed.txBytes === "0x" + toHex(soft.txBytes), "the device's transaction is byte-identical to QoreSign's");
  await broadcast(fromHex(signed.txBytes));
  check((await balance(rest, you)) === before + 1_500_000n, "the chain accepted the transfer and moved 1.5 QOR");

  // 6. the one button: a tap rejects, a hold approves, and the threshold is where the chip says
  script.approval.push(0);
  await expectError("a tap on the approval screen rejects", () => sendOnce(1), /rejected/);
  script.approval.push(HOLD - 1);
  await expectError(`a press of ${HOLD - 1} ms is still a tap`, () => sendOnce(1), /rejected/);
  script.approval.push(HOLD);
  const ok = await sendOnce(1_000);
  check(!!ok.signed.txBytes, `a press of ${HOLD} ms is a hold: it approves`);
  await broadcast(fromHex(ok.signed.txBytes));

  // 7. the PIN
  script.pins.push(PIN ^ 1);
  await expectError("a wrong PIN signs nothing", () => sendOnce(1), /wrong PIN \(4 tries left\)/);
  const afterWrong = await sendOnce(2_000);
  check(!!afterWrong.signed.txBytes, "the right PIN works again, and resets the tries");
  await broadcast(fromHex(afterWrong.signed.txBytes));

  // 8. what the device refuses to sign
  const own = classical(0).publicKey;
  const refuse = async (what, params, pattern) => expectError(what, () => call("device.signTx", { account: 0, accountNumber: "1", chainId: CHAIN_ID, signBytes: version, ...params }), pattern);
  const fresh = await accountInfo(rest, me);
  const mk = (messages, pub = own) => buildTx({ messages, publicKey: pub, sequence: fresh.sequence });
  const t1 = mk([sendMsg(you, me, 1)]);
  await refuse("a transfer from another account", { body: toHex(t1.bodyBytes), authInfo: toHex(t1.authInfoBytes) }, /not from this account/);
  const t2 = mk([sendMsg(me, you, 1), sendMsg(me, you, 1)]);
  await refuse("two messages in one transaction", { body: toHex(t2.bodyBytes), authInfo: toHex(t2.authInfoBytes) }, /one message/);
  const t3 = mk([sendMsg(me, you, 1)], classical(1).publicKey);
  await refuse("another signer's key in the auth info", { body: toHex(t3.bodyBytes), authInfo: toHex(t3.authInfoBytes) }, /another signer key/);
  const t4 = mk([sendMsg(me, you, 1)]);
  await refuse("classical-only signing of a transfer", { body: toHex(t4.bodyBytes), authInfo: toHex(t4.authInfoBytes), classicalOnly: true }, /only for a key registration/);
  await refuse("a truncated transaction", { body: toHex(t4.bodyBytes).slice(0, -4), authInfo: toHex(t4.authInfoBytes) }, /can't read|refuse|bad/i);
  const t5 = mk([{ typeUrl: "/cosmos.bank.v1beta1.MsgMultiSend", value: new Uint8Array([1, 2, 3]) }]);
  script.approval.push(0); // an unknown message is shown as BLIND; the user can reject it
  await expectError("an unknown message type reaches the screen (BLIND) and a tap rejects it", () => call("device.signTx", { account: 0, accountNumber: "1", chainId: CHAIN_ID, signBytes: version, body: toHex(t5.bodyBytes), authInfo: toHex(t5.authInfoBytes) }), /rejected/);

  // 9. other networks: the chain id is part of what is signed, and mainnet says so on the screen
  const main = buildTx({ messages: [sendMsg(me, you, 250_000_000)], publicKey: own, sequence: 5n });
  const onMain = await call("device.signTx", { account: 0, accountNumber: "9", chainId: "qorechain-vladi", signBytes: "v2", body: toHex(main.bodyBytes), authInfo: toHex(main.authInfoBytes) });
  const softMain = signHybrid({ chainId: "qorechain-vladi", accountNumber: 9, bodyBytes: main.bodyBytes, authInfoBytes: main.authInfoBytes, classicalKey: classical(0), pqKey: pq(0), signBytesVersion: "v2" });
  check(onMain.txBytes === "0x" + toHex(softMain.txBytes), "a transaction for qorechain-vladi (mainnet) is byte-identical to QoreSign's, v2 sign bytes");
  const onV1 = await call("device.signTx", { account: 0, accountNumber: "9", chainId: "qorechain-diana", signBytes: "v1", body: toHex(main.bodyBytes), authInfo: toHex(main.authInfoBytes) });
  const softV1 = signHybrid({ chainId: "qorechain-diana", accountNumber: 9, bodyBytes: main.bodyBytes, authInfoBytes: main.authInfoBytes, classicalKey: classical(0), pqKey: pq(0), signBytesVersion: "v1" });
  check(onV1.txBytes === "0x" + toHex(softV1.txBytes), "a v1 sign-bytes transaction (chains that have not switched) is byte-identical too");
  const verdict = verifyHybridTx({ txBytes: fromHex(onMain.txBytes), chainId: "qorechain-vladi", accountNumber: 9, classicalPublicKey: classical(0).publicKey, pqPublicKey: pq(0).publicKey, classicalKeyType: "secp256k1", signBytesVersion: "v2" });
  check(verdict.valid === true, "QoreSign verifies the device's transaction offline (both signatures)");

  console.log(`\n${passed} checks passed`);
} finally {
  driving = false;
  await driver;
}
