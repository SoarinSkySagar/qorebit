/** Cross-VM calls: `/qorechain.crossvm.v1.MsgCrossVMCall` (sender 1, source VM 2, target VM 3, target contract 4, payload 5,
 *  funds 6). A native account calls a contract on another VM (an EVM contract, or a CosmWasm contract) in one transaction.
 *  Pure encoders, no network. Checked against the bytes `qorechaind tx crossvm call` makes (test/crossvm.test.mjs). */
import type { AnyMsg } from "./tx";

export type TargetVm = "evm" | "cosmwasm";
export const CROSS_VM_CALL = "/qorechain.crossvm.v1.MsgCrossVMCall";

const enc = new TextEncoder();
const varint = (n: number | bigint): number[] => {
  const o: number[] = [];
  let v = BigInt(n);
  do {
    let b = Number(v & 0x7fn);
    v >>= 7n;
    if (v) b |= 0x80;
    o.push(b);
  } while (v);
  return o;
};
const lenField = (field: number, bytes: Uint8Array): number[] => (bytes.length ? [...varint((field << 3) | 2), ...varint(bytes.length), ...bytes] : []);

export interface Coin {
  denom: string;
  amount: string;
}

export function crossVmCallMsg(a: { sender: string; targetVm: TargetVm; targetContract: string; payload: Uint8Array; funds?: Coin[] }): AnyMsg {
  // the source VM is the other one (what the CLI sends; the chain derives the real one from the sender)
  const source: TargetVm = a.targetVm === "evm" ? "cosmwasm" : "evm";
  const funds = (a.funds ?? []).flatMap((c) => lenField(6, Uint8Array.from([...lenField(1, enc.encode(c.denom)), ...lenField(2, enc.encode(c.amount))])));
  return {
    typeUrl: CROSS_VM_CALL,
    value: Uint8Array.from([
      ...lenField(1, enc.encode(a.sender)),
      ...lenField(2, enc.encode(source)),
      ...lenField(3, enc.encode(a.targetVm)),
      ...lenField(4, enc.encode(a.targetContract)),
      ...lenField(5, a.payload),
      ...funds,
    ]),
  };
}

// ---- helpers for the payloads

export const isHex = (s: string) => /^(0x)?([0-9a-fA-F]{2})*$/.test(s.trim());
export const hexToBytes = (s: string): Uint8Array => {
  const h = s.trim().replace(/^0x/, "");
  return Uint8Array.from(h.match(/../g) ?? [], (b) => parseInt(b, 16));
};
export const bytesToHex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");

/** ABI calldata of the ERC-20 `transfer(address to, uint256 amount)`. */
export function erc20TransferCalldata(toHex20: string, amount: bigint): Uint8Array {
  const to = toHex20.trim().toLowerCase().replace(/^0x/, "");
  if (!/^[0-9a-f]{40}$/.test(to)) throw new Error("the recipient is not a 20-byte address");
  if (amount < 0n || amount >= 1n << 256n) throw new Error("the amount does not fit 256 bits");
  return hexToBytes("a9059cbb" + to.padStart(64, "0") + amount.toString(16).padStart(64, "0"));
}

/** The 20 bytes inside a `qor1…` address (an account), as 0x-hex: the EVM encoding of the same identity. */
export function qorToEvmHex(addr: string): string {
  const CH = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
  const i = addr.lastIndexOf("1");
  const data = [...addr.slice(i + 1, -6)].map((c) => CH.indexOf(c));
  if (i < 0 || data.some((v) => v < 0)) throw new Error("not a bech32 address");
  let acc = 0, bits = 0;
  const out: number[] = [];
  for (const v of data) {
    acc = (acc << 5) | v;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((acc >> bits) & 0xff);
    }
  }
  if (out.length !== 20) throw new Error("only a 20-byte account address has an EVM form");
  return "0x" + bytesToHex(Uint8Array.from(out));
}
