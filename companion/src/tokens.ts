/** Tokens on QoreChain. An ERC-20 contract is a bank coin with the denom `erc20:0x<contract, EIP-55 casing>`; the chain
 *  describes every token it knows in the bank module's denom metadata (symbol, name, units). Sending a token is a normal bank
 *  MsgSend with that denom, so the device signs it like any transfer. */
import { MsgSend } from "cosmjs-types/cosmos/bank/v1beta1/tx";
import type { NetworkId } from "./networks";
import { chainJson } from "./chain";
import { eip55, isEvmAddress } from "./tokenUnits";
import type { AnyMsg } from "./tx";

export { eip55, isEvmAddress, formatUnits, parseUnits, shortDenom } from "./tokenUnits";

export interface Token {
  denom: string;
  symbol: string;
  name: string;
  decimals: number;
}

/** The test tokens of the QoreChain testnet (fUSD, fETH, fBTC) are listed by default there. */
export const DEFAULT_TOKEN_DENOMS: Partial<Record<NetworkId, string[]>> = {
  testnet: [
    "erc20:0x97143B771D34ddb9130b807b135Ae675A5a963CA", // fUSD
    "erc20:0x02292293c13880a26242690eb70a0e42B8E91c28", // fETH
    "erc20:0x4604e7E45b2ecdbfbDc157103f5B2b62cb16451A", // fBTC
  ],
};

function tokenFrom(meta: any): Token {
  const units: any[] = meta.denom_units ?? [];
  const decimals = units.reduce((m, u) => Math.max(m, Number(u.exponent ?? 0)), 0);
  const base = String(meta.base ?? "");
  return {
    denom: base,
    symbol: String(meta.symbol || meta.display || base),
    name: String(meta.display && meta.display !== meta.symbol ? meta.display : meta.name && meta.name !== base ? meta.name : ""),
    decimals,
  };
}

/** Every token the network describes (bank denom metadata). */
export async function networkTokens(net: NetworkId): Promise<Token[]> {
  const out: Token[] = [];
  let key = "";
  for (let page = 0; page < 20; page++) {
    const r = await chainJson(net, `/cosmos/bank/v1beta1/denoms_metadata?pagination.limit=200${key ? `&pagination.key=${encodeURIComponent(key)}` : ""}`);
    if (r.status !== 200) throw new Error(`the ${net} node answered ${r.status}`);
    for (const m of r.json.metadatas ?? []) if (m.base !== "uqor") out.push(tokenFrom(m));
    key = r.json.pagination?.next_key ?? "";
    if (!key) break;
  }
  return out;
}

/** One token by its denom, or null when the chain has no such coin. */
export async function tokenByDenom(net: NetworkId, denom: string): Promise<Token | null> {
  const m = await chainJson(net, `/cosmos/bank/v1beta1/denoms_metadata/${encodeURIComponent(denom)}`);
  if (m.status === 200 && m.json.metadata) return tokenFrom(m.json.metadata);
  // no metadata: a coin that exists (it has a supply) can still be listed, in its smallest unit
  const s = await chainJson(net, `/cosmos/bank/v1beta1/supply/by_denom?denom=${encodeURIComponent(denom)}`);
  if (s.status === 200 && BigInt(s.json.amount?.amount ?? 0) > 0n) return { denom, symbol: denom.length > 14 ? `${denom.slice(0, 10)}..` : denom, name: "", decimals: 0 };
  return null;
}

/** What the user typed: an ERC-20 contract address (0x…) or a bank denom (`erc20:0x…`, `ibc/…`). */
export async function lookupToken(net: NetworkId, input: string): Promise<Token> {
  const t = input.trim();
  if (!t) throw new Error("Enter a token's contract address (0x…) or its denom.");
  if (isEvmAddress(t)) {
    const found = await tokenByDenom(net, `erc20:${eip55(t)}`);
    if (!found) throw new Error(`No token at ${t} on this network (the chain has no coin for that contract).`);
    return found;
  }
  if (/^[A-Za-z0-9/:._-]{3,128}$/.test(t)) {
    const found = await tokenByDenom(net, t);
    if (!found) throw new Error(`The chain has no coin called ${t}.`);
    return found;
  }
  throw new Error("That is not a contract address (0x followed by 40 hex digits) or a coin denom.");
}

/** Every coin balance of an account, by denom. */
export async function balancesOf(net: NetworkId, address: string): Promise<Record<string, bigint>> {
  const out: Record<string, bigint> = {};
  let key = "";
  for (let page = 0; page < 20; page++) {
    const r = await chainJson(net, `/cosmos/bank/v1beta1/balances/${address}?pagination.limit=200${key ? `&pagination.key=${encodeURIComponent(key)}` : ""}`);
    if (r.status !== 200) throw new Error(`the ${net} node answered ${r.status}`);
    for (const c of r.json.balances ?? []) out[c.denom] = BigInt(c.amount);
    key = r.json.pagination?.next_key ?? "";
    if (!key) break;
  }
  return out;
}

export const sendTokenMsg = (from: string, to: string, denom: string, amount: bigint): AnyMsg => ({
  typeUrl: "/cosmos.bank.v1beta1.MsgSend",
  value: MsgSend.encode(MsgSend.fromPartial({ fromAddress: from, toAddress: to, amount: [{ denom, amount: amount.toString() }] })).finish(),
});

// ---- the user's list, kept in this browser (it is only a list of denoms, nothing secret)
const key = (net: NetworkId) => `qorebit.tokens.${net}`;
export function savedDenoms(net: NetworkId): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(key(net)) ?? "[]");
    return Array.isArray(v) ? v.filter((d) => typeof d === "string") : [];
  } catch {
    return [];
  }
}
export function saveDenoms(net: NetworkId, denoms: string[]) {
  try {
    localStorage.setItem(key(net), JSON.stringify(denoms));
  } catch {
    /* private window: the list lasts until the page closes */
  }
}
