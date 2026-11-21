// The companion's transaction code against @qorechain/qoresign: the bytes the device is asked to sign and
// the registration message must be exactly what QoreChain's own library makes.
import test from "node:test";
import assert from "node:assert/strict";
import { buildRegisterPqKeyMessage, deriveCanonicalPqKey, deriveClassicalKey, addressFromPublicKey, signProofOfPossession } from "@qorechain/qoresign";
import { TxBody, AuthInfo } from "cosmjs-types/cosmos/tx/v1beta1/tx";
import { MsgSend } from "cosmjs-types/cosmos/bank/v1beta1/tx";
import { parseQor, formatQor, isQorAddress, registerPqKeyMsg, sendMsg, buildTx, fee, simulationTx } from "../src/tx.ts";

const mnemonic = Array(23).fill("abandon").concat("art").join(" ");
const classical = deriveClassicalKey({ mnemonic });
const address = addressFromPublicKey(classical.publicKey, "secp256k1").bech32;

test("amounts: plain decimals with at most 6 places, nothing else", () => {
  assert.equal(parseQor("1"), 1_000_000n);
  assert.equal(parseQor("1.5"), 1_500_000n);
  assert.equal(parseQor("0.000001"), 1n);
  assert.equal(parseQor(" 2.25 "), 2_250_000n);
  for (const bad of ["", "-1", "1e3", "1.1234567", "1,5", "abc", "0x10", ".5", "1."]) assert.equal(parseQor(bad), null, bad);
  assert.equal(formatQor(1_234_567n), "1.234567");
  assert.equal(formatQor(2_000_000n), "2");
  assert.equal(formatQor(1_234_567_000_000n), "1,234,567");
  assert.equal(formatQor(500n, 4), "0.0005");
});

test("addresses: the bech32 checksum and the qor prefix are checked", () => {
  assert.ok(isQorAddress(address));
  assert.ok(!isQorAddress(address.slice(0, -1) + (address.endsWith("a") ? "b" : "a")));
  assert.ok(!isQorAddress(address.toUpperCase()));
  assert.ok(!isQorAddress("cosmos1" + address.slice(4)));
  assert.ok(!isQorAddress(""));
});

test("MsgRegisterPQCKeyV2 is byte-identical to QoreSign's", () => {
  const pqKey = deriveCanonicalPqKey({ mnemonic, address });
  const proof = signProofOfPossession({ chainId: "qorechain-local", account: address, pqKey });
  const theirs = buildRegisterPqKeyMessage({ chainId: "qorechain-local", account: address, pqKey, classicalPublicKey: classical.publicKey });
  const ours = registerPqKeyMsg(address, pqKey.publicKey, classical.publicKey, proof);
  assert.equal(ours.typeUrl, theirs.typeUrl);
  assert.deepEqual(Buffer.from(ours.value), Buffer.from(theirs.value));
});

test("a transfer: one message, one signer in direct mode, the fee is 0.1 uqor per gas rounded up", () => {
  const to = addressFromPublicKey(deriveClassicalKey({ mnemonic, index: 3 }).publicKey, "secp256k1").bech32;
  const { body, authInfo } = buildTx({ messages: [sendMsg(address, to, 1_500_000n)], memo: "hi", publicKey: classical.publicKey, sequence: 7n, gas: 172_537n });
  const b = TxBody.decode(body);
  assert.equal(b.messages.length, 1);
  assert.equal(b.memo, "hi");
  const m = MsgSend.decode(b.messages[0].value);
  assert.deepEqual([m.fromAddress, m.toAddress, m.amount[0].denom, m.amount[0].amount], [address, to, "uqor", "1500000"]);
  const a = AuthInfo.decode(authInfo);
  assert.equal(a.signerInfos.length, 1);
  assert.equal(a.signerInfos[0].sequence, 7n);
  assert.equal(a.signerInfos[0].modeInfo.single.mode, 1);
  assert.equal(a.fee.gasLimit, 172_537n);
  assert.equal(a.fee.amount[0].amount, "17254");
  assert.equal(fee(10n), 1n);
  assert.equal(fee(11n), 2n);
});

test("the simulation copy carries a placeholder post-quantum layer, the original does not", () => {
  const { body, authInfo } = buildTx({ messages: [sendMsg(address, address, 1n)], publicKey: classical.publicKey, sequence: 0n, gas: 500_000n });
  const raw = simulationTx(body, authInfo);
  assert.ok(raw.length > 4627 + 64);
  assert.ok(TxBody.decode(body).extensionOptions.length === 0);
});
