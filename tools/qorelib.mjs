// Shared helpers for the Node tools (faucet, end-to-end tests): building transactions the way
// QoreChain wallets do, and talking to a chain's REST endpoint. Uses @qorechain/qoresign.
import { sha256 } from "@noble/hashes/sha2.js";
import { TxBody, AuthInfo, SignerInfo, Fee, ModeInfo } from "cosmjs-types/cosmos/tx/v1beta1/tx";
import { SignMode } from "cosmjs-types/cosmos/tx/signing/v1beta1/signing";
import { PubKey } from "cosmjs-types/cosmos/crypto/secp256k1/keys";
import { MsgSend } from "cosmjs-types/cosmos/bank/v1beta1/tx";
import { toHex, fromHex } from "@qorechain/qoresign";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
export const networks = JSON.parse(readFileSync(join(here, "networks.json"), "utf8"));

/** Where the tools reach a network: the bridge's proxy when set, otherwise the endpoint itself. */
export function restOf(net) {
  return (process.env.QORE_REST ?? networks[net].rest).replace(/\/$/, "");
}

export async function getJson(url, init) {
  const r = await fetch(url, init);
  const text = await r.text();
  let json;
  try { json = JSON.parse(text); } catch { json = { raw: text }; }
  return { status: r.status, json };
}

/** account_number and sequence of an account, or undefined when it does not exist on chain yet. */
export async function accountInfo(rest, address) {
  const { status, json } = await getJson(`${rest}/cosmos/auth/v1beta1/accounts/${address}`);
  if (status !== 200) return undefined;
  const a = json.account ?? {};
  const base = a.base_account ?? a;
  return { accountNumber: BigInt(base.account_number ?? 0), sequence: BigInt(base.sequence ?? 0) };
}

export async function balance(rest, address, denom = "uqor") {
  const { json } = await getJson(`${rest}/cosmos/bank/v1beta1/balances/${address}`);
  return BigInt((json.balances ?? []).find((c) => c.denom === denom)?.amount ?? 0);
}

export const sendMsg = (from, to, amount, denom = "uqor") => ({
  typeUrl: "/cosmos.bank.v1beta1.MsgSend",
  value: MsgSend.encode(MsgSend.fromPartial({ fromAddress: from, toAddress: to, amount: [{ denom, amount: String(amount) }] })).finish(),
});

/** Unsigned body and auth info: one signer with `publicKey` (33 bytes) at `sequence`. */
export function buildTx({ messages, memo = "", publicKey, sequence, gas = 500000n, feeUqor }) {
  const fee = feeUqor ?? ((gas + 9n) / 10n); // 0.1 uqor per gas
  const bodyBytes = TxBody.encode(TxBody.fromPartial({ messages, memo })).finish();
  const authInfoBytes = AuthInfo.encode(AuthInfo.fromPartial({
    signerInfos: [SignerInfo.fromPartial({
      publicKey: { typeUrl: "/cosmos.crypto.secp256k1.PubKey", value: PubKey.encode({ key: publicKey }).finish() },
      modeInfo: ModeInfo.fromPartial({ single: { mode: SignMode.SIGN_MODE_DIRECT } }),
      sequence: BigInt(sequence),
    })],
    fee: Fee.fromPartial({ amount: [{ denom: "uqor", amount: String(fee) }], gasLimit: gas }),
  })).finish();
  return { bodyBytes, authInfoBytes };
}

/** Poll until the chain has the transaction; returns the tx_response. */
export async function waitTx(rest, hash, seconds = 60) {
  for (let i = 0; i < seconds * 2; i++) {
    const { status, json } = await getJson(`${rest}/cosmos/tx/v1beta1/txs/${hash}`);
    if (status === 200 && json.tx_response) return json.tx_response;
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`transaction ${hash} was not included within ${seconds} s`);
}

export { toHex, fromHex, sha256 };
