import test from "node:test";
import assert from "node:assert/strict";
import { MsgSend } from "cosmjs-types/cosmos/bank/v1beta1/tx";
import { describeSiteTx } from "../src/siteTx.ts";

const ME = "qor1zn2y6duvgcda5htklazv9etu7kwstg0d7ccpuz";
const YOU = "qor1r5v5srda7xfth3hn2s26txvrcrntldju0qscza";
const b64 = (u) => Buffer.from(u).toString("base64");

test("a send as an object is encoded by the wallet and described", () => {
  const d = describeSiteTx([{ messages: [{ typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: { toAddress: YOU, amount: [{ denom: "uqor", amount: "1500000" }] } }], memo: "hi" }], ME);
  assert.equal(d.kind, "decoded");
  assert.deepEqual(d.rows[0], ["Amount", "1.5 QOR"]);
  assert.equal(d.rows[1][1], YOU);
  assert.equal(d.memo, "hi");
  assert.equal(MsgSend.decode(d.msg.value).fromAddress, ME, "the wallet fills in the connected account as the sender");
});

test("a send as protobuf bytes is decoded for display", () => {
  const bytes = MsgSend.encode(MsgSend.fromPartial({ fromAddress: ME, toAddress: YOU, amount: [{ denom: "erc20:0x97143B771D34ddb9130b807b135Ae675A5a963CA", amount: "5" }] })).finish();
  const d = describeSiteTx([{ messages: [{ typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: b64(bytes) }] }], ME);
  assert.match(d.rows[0][1], /^5 erc20:0x9714/);
});

test("a send from another account, or to a bad address, is refused", () => {
  const bytes = MsgSend.encode(MsgSend.fromPartial({ fromAddress: YOU, toAddress: ME, amount: [{ denom: "uqor", amount: "1" }] })).finish();
  assert.throws(() => describeSiteTx([{ messages: [{ typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: b64(bytes) }] }], ME), /not from the connected account/);
  assert.throws(() => describeSiteTx([{ messages: [{ typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: { toAddress: "qor1bad", amount: [{ denom: "uqor", amount: "1" }] } }] }], ME), /valid qor1/);
  assert.throws(() => describeSiteTx([{ messages: [{ typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: { fromAddress: YOU, toAddress: YOU, amount: [{ denom: "uqor", amount: "1" }] } }] }], ME), /connected account/);
});

test("amounts must be positive whole numbers", () => {
  for (const amount of ["0", "-1", "1.5", "01", ""]) {
    assert.throws(() => describeSiteTx([{ messages: [{ typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: { toAddress: YOU, amount: [{ denom: "uqor", amount }] } }] }], ME), /whole numbers/);
  }
});

test("exactly one message, a short memo, a sane type url", () => {
  const one = { typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: { toAddress: YOU, amount: [{ denom: "uqor", amount: "1" }] } };
  assert.throws(() => describeSiteTx([{ messages: [one, one] }], ME), /exactly one/);
  assert.throws(() => describeSiteTx([{ messages: [] }], ME), /exactly one/);
  assert.throws(() => describeSiteTx([{ messages: [one], memo: "x".repeat(121) }], ME), /memo/);
  assert.throws(() => describeSiteTx([{ messages: [{ typeUrl: "javascript:alert(1)", value: "AA==" }] }], ME), /typeUrl/);
  assert.throws(() => describeSiteTx([], ME), /messages/);
});

test("an unknown message type is accepted but flagged: the wallet cannot read it", () => {
  const d = describeSiteTx([{ messages: [{ typeUrl: "/qorechain.crossvm.v1.MsgCrossVMCall", value: b64(new Uint8Array([10, 3, 97, 98, 99])) }] }], ME);
  assert.equal(d.kind, "raw");
  assert.equal(d.warnings.length, 1);
  assert.equal(d.title, "MsgCrossVMCall");
  assert.throws(() => describeSiteTx([{ messages: [{ typeUrl: "/x.y.Z", value: "***" }] }], ME), /base64/);
});
