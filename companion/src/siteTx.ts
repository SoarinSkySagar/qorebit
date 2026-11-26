/** A website's `qor_sendTransaction` request: validate it and describe it for the approval window.
 *  params[0] = { messages: [{ typeUrl, value }], memo? } with exactly one message (the device signs one message per transaction).
 *  `value` is the protobuf message as base64, or for a plain send an object { toAddress, amount: [{ denom, amount }], fromAddress? }. */
import { MsgSend } from "cosmjs-types/cosmos/bank/v1beta1/tx";
import { MsgDelegate, MsgUndelegate } from "cosmjs-types/cosmos/staking/v1beta1/tx";
import { isQorAddress, formatQor, type AnyMsg } from "./tx.ts";

export const MSG_SEND = "/cosmos.bank.v1beta1.MsgSend";
const MSG_DELEGATE = "/cosmos.staking.v1beta1.MsgDelegate";
const MSG_UNDELEGATE = "/cosmos.staking.v1beta1.MsgUndelegate";
export const MAX_MESSAGE_BYTES = 4096;
export const MAX_MEMO = 120;

export interface Described {
  msg: AnyMsg;
  memo: string;
  /** What the wallet understood. "decoded": the rows are the message's own fields; "raw": only the type and the bytes are known. */
  kind: "decoded" | "raw";
  title: string;
  rows: [string, string][];
  /** QOR (in uqor) this message moves out of the account, so the wallet can check the balance covers it and the fee. */
  uqorSpend: bigint;
  /** Warnings the approval window shows in red (the device still reads the transaction itself). */
  warnings: string[];
}

const b64ToBytes = (s: string): Uint8Array => {
  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(s) || s.length % 4 === 1) throw new Error("value is not base64");
  return Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
};
const hex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");

function coinText(c: { denom: string; amount: string }): string {
  if (c.denom === "uqor") return `${formatQor(BigInt(c.amount))} QOR`;
  return `${c.amount} ${c.denom}`;
}
function checkCoins(coins: unknown): { denom: string; amount: string }[] {
  if (!Array.isArray(coins) || coins.length === 0 || coins.length > 4) throw new Error("amount must be a list of 1 to 4 coins");
  return coins.map((c) => {
    if (!c || typeof c.denom !== "string" || typeof c.amount !== "string") throw new Error("each coin is { denom, amount } with strings");
    if (!/^[a-zA-Z][a-zA-Z0-9/:._-]{2,127}$/.test(c.denom)) throw new Error(`"${String(c.denom).slice(0, 20)}" is not a coin denom`);
    if (!/^[1-9][0-9]{0,38}$/.test(c.amount)) throw new Error("coin amounts are positive whole numbers in the smallest unit");
    return { denom: c.denom, amount: c.amount };
  });
}

/** Throws an Error whose message is safe to show (and to send back to the site). */
export function describeSiteTx(params: unknown, account: string): Described {
  const p = (Array.isArray(params) ? params[0] : null) as { messages?: unknown; memo?: unknown } | null;
  if (!p || !Array.isArray(p.messages)) throw new Error("params[0].messages must be a list with one message");
  if (p.messages.length !== 1) throw new Error("the device signs one message per transaction: send a list with exactly one message");
  const memo = p.memo === undefined ? "" : p.memo;
  if (typeof memo !== "string" || memo.length > MAX_MEMO) throw new Error(`memo must be text of at most ${MAX_MEMO} characters`);
  const m = p.messages[0] as { typeUrl?: unknown; value?: unknown };
  if (typeof m?.typeUrl !== "string" || !/^\/[A-Za-z0-9_.]{3,120}$/.test(m.typeUrl)) throw new Error("typeUrl must look like /cosmos.bank.v1beta1.MsgSend");
  const warnings: string[] = [];
  let value: Uint8Array;

  if (m.typeUrl === MSG_SEND && m.value && typeof m.value === "object") {
    const o = m.value as Record<string, unknown>;
    const from = String(o.fromAddress ?? o.from_address ?? account);
    const to = String(o.toAddress ?? o.to_address ?? "");
    if (from !== account) throw new Error("fromAddress must be the connected account");
    if (!isQorAddress(to)) throw new Error("toAddress is not a valid qor1… address");
    value = MsgSend.encode(MsgSend.fromPartial({ fromAddress: account, toAddress: to, amount: checkCoins(o.amount) })).finish();
  } else if (typeof m.value === "string") {
    value = b64ToBytes(m.value);
  } else {
    throw new Error("value must be the protobuf message as base64 (or { toAddress, amount } for a send)");
  }
  if (value.length === 0 || value.length > MAX_MESSAGE_BYTES) throw new Error(`the message must be 1 to ${MAX_MESSAGE_BYTES} bytes`);
  const msg: AnyMsg = { typeUrl: m.typeUrl, value };

  try {
    if (m.typeUrl === MSG_SEND) {
      const d = MsgSend.decode(value);
      if (d.fromAddress !== account) throw new Error("the send is not from the connected account");
      if (!isQorAddress(d.toAddress)) throw new Error("the send's recipient is not a valid qor1… address");
      return { msg, memo, kind: "decoded", title: "Send", warnings, uqorSpend: d.amount.filter((c) => c.denom === "uqor").reduce((t, c) => t + BigInt(c.amount), 0n), rows: [["Amount", d.amount.map(coinText).join(" + ")], ["To", d.toAddress]] };
    }
    if (m.typeUrl === MSG_DELEGATE || m.typeUrl === MSG_UNDELEGATE) {
      const d = (m.typeUrl === MSG_DELEGATE ? MsgDelegate : MsgUndelegate).decode(value);
      if (d.delegatorAddress !== account) throw new Error("the staking message is not from the connected account");
      return {
        msg, memo, kind: "decoded", title: m.typeUrl === MSG_DELEGATE ? "Delegate (stake)" : "Undelegate (unstake)", warnings,
        uqorSpend: m.typeUrl === MSG_DELEGATE && d.amount?.denom === "uqor" ? BigInt(d.amount.amount) : 0n,
        rows: [["Amount", d.amount ? coinText(d.amount) : "?"], ["Validator", d.validatorAddress]],
      };
    }
  } catch (e) {
    if (e instanceof Error && /connected account|valid qor1/.test(e.message)) throw e;
    throw new Error("the message does not decode as its type says");
  }
  // anything else: the wallet cannot read it, the device shows its own label for it (or BLIND)
  warnings.push("The wallet cannot read this kind of message. Check what the device shows, and only approve it if you trust the site.");
  return { msg, memo, kind: "raw", uqorSpend: 0n, title: m.typeUrl.split(".").pop()!, warnings, rows: [["Type", m.typeUrl], ["Size", `${value.length} bytes`], ["Data", hex(value.slice(0, 48)) + (value.length > 48 ? "…" : "")]] };
}
