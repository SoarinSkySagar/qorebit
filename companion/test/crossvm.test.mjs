import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { crossVmCallMsg, erc20TransferCalldata, qorToEvmHex, bytesToHex, hexToBytes, isHex } from "../src/crossVmMsg.ts";

const ME = "qor1zn2y6duvgcda5htklazv9etu7kwstg0d7ccpuz";
const FUSD = "0x97143B771D34ddb9130b807b135Ae675A5a963CA";
// the bytes `qorechaind tx crossvm call …` makes for these calls (captured from the node, v3.2.8)
const GOLDEN = { evm: "0a2a716f72317a6e327936647576676364613568746b6c617a7639657475376b77737467306437636370757a1208636f736d7761736d1a0365766d222a3078393731343342373731443334646462393133306238303762313335416536373541356139363343412a44a9059cbb00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000001", cosmwasm: "0a2a716f72317a6e327936647576676364613568746b6c617a7639657475376b77737467306437636370757a120365766d1a08636f736d7761736d223e716f723134686a32746176713866706573647778786375343472747933686839307668756a7276636d73746c347a723374786d66767739733933757571742a0b7b2270696e67223a7b7d7d32090a0475716f72120135" };

test("a call to an EVM contract encodes like the node's CLI", () => {
  const payload = hexToBytes("a9059cbb" + "00".repeat(31) + "01" + "00".repeat(31) + "01");
  const m = crossVmCallMsg({ sender: ME, targetVm: "evm", targetContract: FUSD, payload });
  assert.equal(m.typeUrl, "/qorechain.crossvm.v1.MsgCrossVMCall");
  assert.equal(bytesToHex(m.value), GOLDEN.evm);
});

test("a call to a native contract carries JSON and funds, like the node's CLI", () => {
  const m = crossVmCallMsg({ sender: ME, targetVm: "cosmwasm", targetContract: "qor14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9s93uuqt", payload: new TextEncoder().encode('{"ping":{}}'), funds: [{ denom: "uqor", amount: "5" }] });
  assert.equal(bytesToHex(m.value), GOLDEN.cosmwasm);
});

test("ERC-20 transfer calldata: selector, padded address, 32-byte amount", () => {
  const d = erc20TransferCalldata("0x0102030405060708090a0b0c0d0e0f1011121314", 1_000_000n);
  assert.equal(d.length, 68);
  assert.equal(bytesToHex(d.slice(0, 4)), "a9059cbb");
  assert.equal(bytesToHex(d.slice(4, 36)), "0".repeat(24) + "0102030405060708090a0b0c0d0e0f1011121314");
  assert.equal(BigInt("0x" + bytesToHex(d.slice(36))), 1_000_000n);
  assert.throws(() => erc20TransferCalldata("0x1234", 1n));
});

import { parseAddress } from "@qorechain/qoresign";
test("a qor1 account address has an EVM form (the same 20 bytes)", () => {
  for (const a of [ME, "qor1r5v5srda7xfth3hn2s26txvrcrntldju0qscza"]) {
    assert.equal(qorToEvmHex(a), "0x" + bytesToHex(parseAddress(a).bytes));
  }
  assert.throws(() => qorToEvmHex("qor1dlszg2sst9r69my4f84l3mj66zxcf3umcgujys30t84srg95dgvst4knx5"), /20-byte/);
  assert.equal(isHex("0xabcd"), true);
  assert.equal(isHex("0xabc"), false);
});
