import { useCallback, useEffect, useState } from "react";
import type { DeviceAccount } from "./bridge";
import { chainJson } from "./chain";
import type { NetworkId } from "./networks";
import { ACCOUNT_TYPES, MAX_RULES, createAbstractAccountMsg, rulesFromJson, updateSpendingRulesMsg, type AccountType, type SpendingRule } from "./smartAccountMsg";
import { formatQor, parseQor, type AnyMsg } from "./tx";

const DEVICE_HINT = "The device shows this as a SMART ACCOUNT transaction. It does not show the details: they are on this page, so check them here first. Right button approves, left button rejects.";
const TYPE_TEXT: Record<AccountType, string> = {
  session_based: "Session based: temporary keys with limits",
  multisig: "Multisig: several keys must agree",
  social_recovery: "Social recovery: guardians can restore access",
};

interface Chain {
  address: string;
  owner: string;
  account_type: string;
  spending_rules_count: string | number;
  authenticators_count: string | number;
  session_keys_count: string | number;
}
/** A rule as typed: amounts in QOR. */
interface Draft {
  id: string;
  enabled: boolean;
  perTx: string;
  daily: string;
  denoms: string;
  evm: string;
  rest: SpendingRule; // what the editor does not show (per-denom limits) is kept as it is
}
const toDraft = (r: SpendingRule): Draft => ({
  id: r.id, enabled: r.enabled, perTx: r.perTxLimit ? formatQor(r.perTxLimit) : "", daily: r.dailyLimit ? formatQor(r.dailyLimit) : "",
  denoms: r.allowedDenoms.join(", "), evm: r.allowedEvmCalls.join("\n"), rest: r,
});

interface Props {
  net: NetworkId;
  acct: DeviceAccount | null;
  why: string;
  onSubmit: (what: string, msgs: AnyMsg[], hint?: string) => Promise<boolean>;
}

export function SmartAccountCard({ net, acct, why, onSubmit }: Props) {
  const me = acct?.bech32 ?? "";
  const [state, setState] = useState<"loading" | "off" | "none" | "have" | "error">("loading");
  const [owned, setOwned] = useState<Chain[]>([]);
  const [pick, setPick] = useState(""); // address of the smart account being edited
  const [creating, setCreating] = useState(false);
  const chain = owned.find((c) => c.address === pick) ?? null;
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const [type, setType] = useState<AccountType>("session_based");
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    if (!me) return;
    setState("loading");
    try {
      const cfg = await chainJson(net, "/qorechain/abstractaccount/v1/config");
      if (cfg.status !== 200) throw new Error(`the ${net} node answered ${cfg.status}`);
      if (!cfg.json.config?.enabled) return setState("off");
      // the chain makes a new address for each smart account: the ones this account owns are found in the list
      const all = await chainJson(net, "/qorechain/abstractaccount/v1/accounts");
      if (all.status !== 200) throw new Error(`the ${net} node answered ${all.status}`);
      const mine: Chain[] = (all.json.accounts ?? []).filter((c: Chain) => c.owner === me);
      setOwned(mine);
      setCreating(mine.length === 0);
      setPick((p) => (mine.some((c) => c.address === p) ? p : mine[0]?.address ?? ""));
      setState(mine.length ? "have" : "none");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setState("error");
    }
  }, [net, me]);
  useEffect(() => void load(), [load]);
  // the rules of the account picked
  useEffect(() => {
    if (!pick) return setDrafts([]);
    let live = true;
    void chainJson(net, `/qorechain/abstractaccount/v1/accounts/${pick}/spending_rules`).then((r) => {
      if (live) setDrafts(r.status === 200 ? rulesFromJson(r.json).map(toDraft) : []);
    });
    return () => void (live = false);
  }, [net, pick, owned]);

  const set = (i: number, p: Partial<Draft>) => setDrafts((d) => d.map((x, j) => (j === i ? { ...x, ...p } : x)));

  function rules(): SpendingRule[] | string {
    const seen = new Set<string>();
    const out: SpendingRule[] = [];
    for (const [i, d] of drafts.entries()) {
      const n = `Rule ${i + 1}`;
      const id = d.id.trim();
      if (!id) return `${n} needs a name.`;
      if (seen.has(id)) return `Two rules are called "${id}".`;
      seen.add(id);
      const perTx = d.perTx.trim() ? parseQor(d.perTx) : 0n;
      const daily = d.daily.trim() ? parseQor(d.daily) : 0n;
      if (perTx === null || daily === null) return `${n}: limits are amounts in QOR, like 1.5.`;
      if (daily && perTx > daily) return `${n}: the per transaction limit is above the daily limit.`;
      const evm = d.evm.split("\n").map((s) => s.trim()).filter(Boolean);
      for (const c of evm) if (!/^0x[0-9a-fA-F]{40}(\/0x[0-9a-fA-F]{8})?$/.test(c)) return `${n}: "${c}" is not 0x<contract> or 0x<contract>/0x<selector>.`;
      out.push({ ...d.rest, id, enabled: d.enabled, perTxLimit: perTx, dailyLimit: daily, allowedDenoms: d.denoms.split(",").map((s) => s.trim()).filter(Boolean), allowedEvmCalls: evm });
    }
    return out;
  }

  async function create() {
    if (await onSubmit("Create smart account", [createAbstractAccountMsg(me, type)], DEVICE_HINT)) void load();
  }
  async function save() {
    const r = rules();
    if (typeof r === "string") return setError(r);
    setError("");
    if (await onSubmit("Save spending rules", [updateSpendingRulesMsg(me, pick, r)], DEVICE_HINT)) void load();
  }

  return (
    <section className="card">
      <h2>Smart account</h2>
      <p className="muted small">
        A smart account adds rules to your account that the chain enforces: a limit per transaction, a limit per day, which coins may move.
        The rules govern what linked keys and relayers may spend for this account. They do not change what you can send yourself with the device.
        Creating the account and changing the rules are ordinary transactions your device signs; it shows SMART ACCOUNT.
      </p>
      {state === "loading" && <p className="muted small">Asking the {net} node…</p>}
      {state === "off" && <p className="muted small">Smart accounts are switched off on this network.</p>}
      {state === "error" && <p className="bad small">{error}</p>}
      {(state === "none" || (state === "have" && creating)) && (
        <div className="form">
          <label>
            Type
            <select value={type} onChange={(e) => setType(e.target.value as AccountType)}>
              {ACCOUNT_TYPES.map((t) => <option key={t} value={t}>{TYPE_TEXT[t]}</option>)}
            </select>
          </label>
          <button className="primary" disabled={!!why} onClick={() => void create()}>Create smart account</button>
          {why && <p className="muted small">{why}</p>}
          {state === "have" && <button type="button" className="link" onClick={() => setCreating(false)}>Back to my smart accounts</button>}
        </div>
      )}
      {state === "have" && !creating && chain && (
        <>
          <p className="small">
            <label>Smart account
              <select value={pick} onChange={(e) => setPick(e.target.value)}>
                {owned.map((c) => <option key={c.address} value={c.address}>{c.address.slice(0, 14)}…{c.address.slice(-6)} · {c.account_type}</option>)}
              </select>
            </label>{" "}
            <button type="button" className="link" onClick={() => setCreating(true)}>Create another</button>
          </p>
          <p className="small">
            <span className="pill">{TYPE_TEXT[chain.account_type as AccountType] ?? chain.account_type}</span>{" "}
            <span className="muted">linked keys: {String(chain.authenticators_count)} · session keys: {String(chain.session_keys_count)}</span>
          </p>
          <h3>Spending rules ({drafts.length} of {MAX_RULES})</h3>
          {drafts.length === 0 && <p className="muted small">No rules yet: nothing limits what linked keys can spend.</p>}
          {drafts.map((d, i) => (
            <fieldset key={i} className="rule">
              <label>Name<input value={d.id} onChange={(e) => set(i, { id: e.target.value })} autoComplete="off" spellCheck={false} /></label>
              <label className="row"><input type="checkbox" checked={d.enabled} onChange={(e) => set(i, { enabled: e.target.checked })} /> On</label>
              <label>Per transaction limit (QOR, empty = none)<input className="mono" value={d.perTx} onChange={(e) => set(i, { perTx: e.target.value })} inputMode="decimal" autoComplete="off" /></label>
              <label>Daily limit (QOR, empty = none)<input className="mono" value={d.daily} onChange={(e) => set(i, { daily: e.target.value })} inputMode="decimal" autoComplete="off" /></label>
              <label>Coins allowed (comma separated, empty = any)<input className="mono" value={d.denoms} onChange={(e) => set(i, { denoms: e.target.value })} placeholder="uqor" autoComplete="off" spellCheck={false} /></label>
              <label>EVM calls allowed (one per line: 0x… or 0x…/0xselector)<textarea className="mono" rows={2} value={d.evm} onChange={(e) => set(i, { evm: e.target.value })} spellCheck={false} /></label>
              <button type="button" className="link" onClick={() => setDrafts((x) => x.filter((_, j) => j !== i))}>Remove rule</button>
            </fieldset>
          ))}
          {error && <p className="bad small">{error}</p>}
          <div className="buttons">
            <button type="button" disabled={drafts.length >= MAX_RULES} onClick={() => setDrafts((x) => [...x, toDraft({ id: `rule${x.length + 1}`, enabled: true, dailyLimit: 0n, perTxLimit: 0n, allowedDenoms: ["uqor"], allowedEvmCalls: [], denomLimits: [] })])}>Add rule</button>
            <button type="button" className="primary" disabled={!!why} onClick={() => void save()}>Save rules on the chain</button>
          </div>
          {why && <p className="muted small">{why}</p>}
        </>
      )}
    </section>
  );
}
