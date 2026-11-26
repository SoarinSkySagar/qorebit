/** Smart accounts (the chain's `abstractaccount` module): create one, and set its spending rules. Both are plain messages the
 *  account's owner signs. Pure encoders, no network; checked against the bytes `qorechaind tx abstractaccount …` makes
 *  (test/smartaccount.test.mjs). MsgCreateAbstractAccount: owner 1, account_type 2. MsgUpdateSpendingRules: owner 1,
 *  account_address 2, rules 3 (repeated SpendingRule: id 1, daily_limit 2, per_tx_limit 3 (varints), allowed_denoms 4,
 *  enabled 5, allowed_evm_calls 6, denom_limits 7 {denom 1, per_tx_limit 2, daily_limit 3 as strings}). */
import type { AnyMsg } from "./tx";

export const CREATE_ABSTRACT = "/qorechain.abstractaccount.v1.MsgCreateAbstractAccount";
export const UPDATE_RULES = "/qorechain.abstractaccount.v1.MsgUpdateSpendingRules";
export const ACCOUNT_TYPES = ["session_based", "multisig", "social_recovery"] as const;
export type AccountType = (typeof ACCOUNT_TYPES)[number];
/** The chain keeps at most this many rules per account. */
export const MAX_RULES = 5;

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
const str = (field: number, s: string): number[] => (s ? [...varint((field << 3) | 2), ...varint(enc.encode(s).length), ...enc.encode(s)] : []);
const msg = (field: number, body: number[]): number[] => [...varint((field << 3) | 2), ...varint(body.length), ...body];
const num = (field: number, n: bigint): number[] => (n ? [...varint(field << 3), ...varint(n)] : []);

export interface DenomLimit {
  denom: string;
  perTx: string;
  daily: string;
}
export interface SpendingRule {
  id: string;
  enabled: boolean;
  /** uqor, whole units */
  dailyLimit: bigint;
  perTxLimit: bigint;
  allowedDenoms: string[];
  /** "0x<target>" or "0x<target>/0x<selector>" */
  allowedEvmCalls: string[];
  denomLimits: DenomLimit[];
}

export function createAbstractAccountMsg(owner: string, type: AccountType): AnyMsg {
  return { typeUrl: CREATE_ABSTRACT, value: Uint8Array.from([...str(1, owner), ...str(2, type)]) };
}

export function updateSpendingRulesMsg(owner: string, account: string, rules: SpendingRule[]): AnyMsg {
  const body = rules.flatMap((r) =>
    msg(3, [
      ...str(1, r.id),
      ...num(2, r.dailyLimit),
      ...num(3, r.perTxLimit),
      ...r.allowedDenoms.flatMap((d) => str(4, d)),
      ...(r.enabled ? [0x28, 1] : []),
      ...r.allowedEvmCalls.flatMap((c) => str(6, c)),
      ...r.denomLimits.flatMap((l) => msg(7, [...str(1, l.denom), ...str(2, l.perTx), ...str(3, l.daily)])),
    ]),
  );
  return { typeUrl: UPDATE_RULES, value: Uint8Array.from([...str(1, owner), ...str(2, account), ...body]) };
}

/** What the chain's REST answers for an account's rules, as the editor holds them. */
export function rulesFromJson(j: any): SpendingRule[] {
  return (j?.rules ?? []).map((r: any) => ({
    id: String(r.id ?? ""),
    enabled: !!r.enabled,
    dailyLimit: BigInt(r.daily_limit ?? 0),
    perTxLimit: BigInt(r.per_tx_limit ?? 0),
    allowedDenoms: r.allowed_denoms ?? [],
    allowedEvmCalls: r.allowed_evm_calls ?? [],
    denomLimits: (r.denom_limits ?? []).map((l: any) => ({ denom: l.denom, perTx: String(l.per_tx_limit ?? "0"), daily: String(l.daily_limit ?? "0") })),
  }));
}
