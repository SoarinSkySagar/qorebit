import { useCallback, useEffect, useMemo, useState } from "react";
import type { DeviceAccount } from "./bridge";
import type { NetworkId, NetworkInfo } from "./networks";
import { DEFAULT_TOKEN_DENOMS, balancesOf, formatUnits, lookupToken, networkTokens, parseUnits, saveDenoms, savedDenoms, sendTokenMsg, shortDenom, tokenByDenom, type Token } from "./tokens";
import { isQorAddress, type AnyMsg } from "./tx";

const normAddr = (s: string) => {
  const t = s.trim().replace(/\s+/g, "");
  return t === t.toUpperCase() ? t.toLowerCase() : t;
};

interface Props {
  net: NetworkId;
  info: NetworkInfo;
  acct: DeviceAccount | null;
  /** The account's QOR (for the network fee). */
  qor: bigint | null;
  /** Why nothing can be signed now ("" when it can). */
  why: string;
  onSubmit: (what: string, msgs: AnyMsg[], hint?: string) => Promise<boolean>;
}

export function TokensCard({ net, acct, qor, why, onSubmit }: Props) {
  const me = acct?.bech32 ?? "";
  const defaults = DEFAULT_TOKEN_DENOMS[net] ?? [];
  const [added, setAdded] = useState<string[]>(() => savedDenoms(net));
  const [meta, setMeta] = useState<Record<string, Token | null>>({});
  const [bal, setBal] = useState<Record<string, bigint> | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<string>("");
  const [busy, setBusy] = useState(false);

  const denoms = useMemo(() => [...defaults, ...added.filter((d) => !defaults.includes(d))], [defaults, added]);

  // the list is per network
  useEffect(() => {
    setAdded(savedDenoms(net));
    setMeta({});
    setBal(null);
    setOpen("");
  }, [net]);

  // what the chain says about each listed token
  useEffect(() => {
    let live = true;
    for (const d of denoms) {
      if (d in meta) continue;
      tokenByDenom(net, d).then(
        (t) => live && setMeta((m) => ({ ...m, [d]: t })),
        () => live && setMeta((m) => ({ ...m, [d]: null })),
      );
    }
    return () => void (live = false);
  }, [net, denoms]); // eslint-disable-line react-hooks/exhaustive-deps

  const loadBalances = useCallback(async () => {
    if (!me) return;
    try {
      setBal(await balancesOf(net, me));
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [net, me]);
  useEffect(() => {
    void loadBalances();
    const t = setInterval(() => void loadBalances(), 8000);
    return () => clearInterval(t);
  }, [loadBalances]);

  function add(t: Token) {
    setMeta((m) => ({ ...m, [t.denom]: t }));
    if (defaults.includes(t.denom) || added.includes(t.denom)) return;
    const next = [...added, t.denom];
    setAdded(next);
    saveDenoms(net, next);
  }
  function remove(d: string) {
    const next = added.filter((x) => x !== d);
    setAdded(next);
    saveDenoms(net, next);
    if (open === d) setOpen("");
  }

  async function send(t: Token, to: string, units: bigint) {
    setBusy(true);
    try {
      const shown = formatUnits(units, t.decimals);
      const ok = await onSubmit(
        `Send ${shown} ${t.symbol}`,
        [sendTokenMsg(me, to, t.denom, units)],
        `Check the device: it shows the amount in the token's smallest unit (${shown} ${t.symbol} is ${units.toString()}) and the coin ${shortDenom(t.denom)}. Right button approves, left button rejects.`,
      );
      if (ok) await loadBalances();
      return ok;
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card">
      <h2>Tokens</h2>
      <p className="muted small">
        Tokens on this network that you hold or have added. Sending one is a normal transfer approved on the device; the network fee is paid in QOR
        ({qor === null ? "…" : `${formatUnits(qor, 6, 4)} QOR`} available).
      </p>
      {error && <div className="field-error">{error}</div>}
      <ul className="tokens">
        {denoms.map((d) => {
          const t = meta[d];
          const b = bal?.[d] ?? 0n;
          return (
            <li key={d}>
              <div className="row between">
                <div>
                  <b>{t ? t.symbol : "…"}</b> {t?.name && <span className="muted small">{t.name}</span>}
                  <div className="muted small mono" title={d}>{shortDenom(d)}</div>
                </div>
                <div className="tok-right">
                  <span className="mono">{t && bal ? formatUnits(b, t.decimals) : "…"}</span>
                  <button className="ghost" disabled={!t || !!why || busy || b === 0n} onClick={() => setOpen(open === d ? "" : d)} title={why || (b === 0n ? "You hold none of this token." : "")}>Send</button>
                  {added.includes(d) && !defaults.includes(d) && <button className="ghost" aria-label={`Remove ${t?.symbol ?? d} from the list`} onClick={() => remove(d)}>✕</button>}
                </div>
              </div>
              {t === null && <div className="field-error">The chain has no coin with this denom on this network.</div>}
              {open === d && t && <SendForm t={t} balance={b} qor={qor} disabled={!!why || busy} onSend={send} />}
            </li>
          );
        })}
      </ul>
      <AddToken net={net} onAdd={add} listed={denoms} />
    </section>
  );
}

function SendForm(p: { t: Token; balance: bigint; qor: bigint | null; disabled: boolean; onSend: (t: Token, to: string, units: bigint) => Promise<boolean> }) {
  const [to, setTo] = useState("");
  const [amount, setAmount] = useState("");
  const dest = normAddr(to);
  const units = parseUnits(amount, p.t.decimals);
  const toOk = isQorAddress(dest);
  const problem = !to ? "Enter who gets it." : !toOk ? "That address is not valid." : !amount ? "Enter an amount." : units === null || units === 0n ? `An amount in ${p.t.symbol}, up to ${p.t.decimals} decimals.` : units > p.balance ? "More than you hold." : p.qor !== null && p.qor < 30_000n ? "You need a little QOR for the network fee." : "";
  const can = !p.disabled && !problem && units !== null;
  return (
    <form
      className="tok-send"
      onSubmit={async (e) => {
        e.preventDefault();
        if (can && units !== null && (await p.onSend(p.t, dest, units))) {
          setTo("");
          setAmount("");
        }
      }}
    >
      <label>To<input className="mono" value={to} onChange={(e) => setTo(e.target.value)} placeholder="qor1…" spellCheck={false} autoComplete="off" /></label>
      <label>
        Amount ({p.t.symbol})
        <input className="mono" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.0" inputMode="decimal" autoComplete="off" />
      </label>
      <div className="row">
        <button className="primary" type="submit" disabled={!can}>Send with the device</button>
        <button className="ghost" type="button" onClick={() => setAmount(formatUnits(p.balance, p.t.decimals).replace(/,/g, ""))}>All</button>
        <span className={`small ${can ? "muted" : "warn-text"}`}>{can ? `The device shows ${units?.toString()} (smallest unit).` : problem}</span>
      </div>
    </form>
  );
}

function AddToken(p: { net: NetworkId; listed: string[]; onAdd: (t: Token) => void }) {
  const [input, setInput] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const [all, setAll] = useState<Token[] | null>(null);

  async function go(e: React.FormEvent) {
    e.preventDefault();
    setErr(null);
    setWorking(true);
    try {
      const t = await lookupToken(p.net, input);
      if (p.listed.includes(t.denom)) setErr(`${t.symbol} is already in your list.`);
      else {
        p.onAdd(t);
        setInput("");
      }
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setWorking(false);
    }
  }
  async function browse(open: boolean) {
    if (open && all === null) {
      try {
        setAll(await networkTokens(p.net));
      } catch (x) {
        setErr(x instanceof Error ? x.message : String(x));
      }
    }
  }
  return (
    <details className="msig-create" onToggle={(e) => void browse((e.target as HTMLDetailsElement).open)}>
      <summary>Add a token</summary>
      <form onSubmit={go}>
        <label>
          Contract address (0x…) or coin denom
          <input className="mono" value={input} onChange={(e) => setInput(e.target.value)} placeholder="0x97143B771D34ddb9130b807b135Ae675A5a963CA" spellCheck={false} autoComplete="off" />
        </label>
        <div className="row">
          <button className="primary" type="submit" disabled={working || !input.trim()}>{working ? "Looking it up…" : "Add"}</button>
          <span className={`small ${err ? "warn-text" : "muted"}`}>{err ?? "The chain is asked about it first: only a coin that exists is added."}</span>
        </div>
      </form>
      <h3 className="small-h">Tokens this network lists</h3>
      {all === null && <p className="muted small">Loading…</p>}
      {all && all.length === 0 && <p className="muted small">None.</p>}
      <ul className="tokens">
        {all?.map((t) => (
          <li key={t.denom}>
            <div className="row between">
              <div><b>{t.symbol}</b> {t.name && <span className="muted small">{t.name}</span>}<div className="muted small mono" title={t.denom}>{shortDenom(t.denom)} · {t.decimals} decimals</div></div>
              <button className="ghost" disabled={p.listed.includes(t.denom)} onClick={() => p.onAdd(t)}>{p.listed.includes(t.denom) ? "Listed" : "Add"}</button>
            </div>
          </li>
        ))}
      </ul>
    </details>
  );
}
