import test from "node:test";
import assert from "node:assert/strict";
import { createAbstractAccountMsg, updateSpendingRulesMsg, rulesFromJson } from "../src/smartAccountMsg.ts";
import { bytesToHex } from "../src/crossVmMsg.ts";

const ME = "qor1zn2y6duvgcda5htklazv9etu7kwstg0d7ccpuz";
const ME_HEX = "716f72317a6e327936647576676364613568746b6c617a7639657475376b77737467306437636370757a";
// the bytes `qorechaind tx abstractaccount …` makes (captured from the node, v3.2.8)
const GOLDEN_CREATE = "0a2a" + ME_HEX + "12086d756c7469736967";
const GOLDEN_RULES = "0a2a" + ME_HEX + "122a" + ME_HEX + "1a430a0161100218032201642801322a3078616261626162616261626162616261626162616261626162616261626162616261626162616261623a090a01781201341a0135";
const TARGET = "0x" + "ab".repeat(20);

test("creating an account encodes like the node's CLI", () => {
  assert.equal(bytesToHex(createAbstractAccountMsg(ME, "multisig").value), GOLDEN_CREATE);
});

test("spending rules encode like the node's CLI", () => {
  const m = updateSpendingRulesMsg(ME, ME, [{ id: "a", enabled: true, dailyLimit: 2n, perTxLimit: 3n, allowedDenoms: ["d"], allowedEvmCalls: [TARGET], denomLimits: [{ denom: "x", perTx: "4", daily: "5" }] }]);
  assert.equal(bytesToHex(m.value), GOLDEN_RULES);
});

test("the rules the chain answers with round-trip through the editor", () => {
  const r = rulesFromJson({ rules: [{ id: "guardian", daily_limit: "3000000", per_tx_limit: "1000000", allowed_denoms: ["uqor"], enabled: true, allowed_evm_calls: [], denom_limits: [] }] });
  assert.equal(r[0].dailyLimit, 3000000n);
  assert.equal(r[0].enabled, true);
  assert.equal(updateSpendingRulesMsg(ME, ME, r).typeUrl, "/qorechain.abstractaccount.v1.MsgUpdateSpendingRules");
});
