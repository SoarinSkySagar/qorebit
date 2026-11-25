import { useCallback, useEffect, useMemo, useState } from "react";
import { getBalance } from "./chain";
import type { DeviceAccount } from "./bridge";
import type { NetworkId, NetworkInfo } from "./networks";
import {
  MAX_MEMBERS, VOTE_NAME, canExecute, createMultisigMsg, durationText, execMsg, isOpen, multisigsOf, proposalsOf, proposePaymentMsg,
  statusText, voteMsg, type Multisig, type Proposal, type VoteOption,
} from "./multisig";
import { formatQor, isQorAddress, parseQor, type AnyMsg } from "./tx";

const short = (s: string, a = 10, b = 6) => (s.length > a + b + 2 ? `${s.slice(0, a)}…${s.slice(-b)}` : s);
const norm = (s: string) => {
  const t = s.trim().replace(/\s+/g, "");
  return t === t.toUpperCase() ? t.toLowerCase() : t;
};

interface Props {
  net: NetworkId;
  info: NetworkInfo;
  acct: DeviceAccount | null;
  /** Why nothing can be signed now ("" when it can). */
  why: string;
  /** Build, sign on the device, broadcast and wait: true when it landed on chain. */
  onSubmit: (what: string, msgs: AnyMsg[]) => Promise<boolean>;
}

export function MultisigCard({ net, info, acct, why, onSubmit }: Props) {
  const me = acct?.bech32 ?? "";
  const [list, setList] = useState<Multisig[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sel, setSel] = useState<string>("");
  const [proposals, setProposals] = useState<Proposal[] | null>(null);
  const [balance, setBalance] = useState<bigint | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    if (!me) return;
    try {
      const l = await multisigsOf(net, me);
      setList(l);
      setError(null);
      setSel((s) => (l.some((m) => m.policy === s) ? s : l[0]?.policy ?? ""));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [net, me]);

  useEffect(() => {
    setList(null);
    setSel("");
    setProposals(null);
    void load();
    const t = setInterval(() => void load(), 12000);
    return () => clearInterval(t);
  }, [load]);

  const current = list?.find((m) => m.policy === sel) ?? null;
  const loadDetail = useCallback(async () => {
    if (!current) return;
    try {
      const [p, b] = await Promise.all([proposalsOf(net, current.policy, me), getBalance(net, current.policy)]);
      setProposals(p);
      setBalance(b);
    } catch {
      /* the next poll tries again */
    }
  }, [net, me, current?.policy]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    setProposals(null);
    setBalance(null);
    void loadDetail();
    const t = setInterval(() => void loadDetail(), 8000);
    return () => clearInterval(t);
  }, [loadDetail]);

  async function run(what: string, msgs: AnyMsg[]) {
    setBusy(true);
    try {
      const ok = await onSubmit(what, msgs);
      if (ok) await Promise.all([load(), loadDetail()]);
      return ok;
    } finally {
      setBusy(false);
    }
  }
  const off = !!why || busy;

  return (
    <section className="card">
      <h2>Multisig</h2>
      <p className="muted small">
        A shared account that needs several members to agree. Moving its funds takes a proposal, enough yes votes, then a run. Each step is
        approved on a member's own device. Only the chain's own group accounts are used: nothing is held by this app.
      </p>
      {error && <div className="field-error">{error}</div>}
      {list === null && !error && <p className="muted">Loading…</p>}
      {list && list.length === 0 && <p className="muted">You are not in a multisig on this network yet. Create one below.</p>}
      {list && list.length > 0 && (
        <div className="msig-tabs" role="tablist" aria-label="Your multisigs">
          {list.map((m) => (
            <button key={m.policy} className={`ghost ${m.policy === sel ? "on" : ""}`} role="tab" aria-selected={m.policy === sel} onClick={() => setSel(m.policy)}>
              {m.name || short(m.policy, 8, 4)} · {m.threshold} of {m.members.length}
            </button>
          ))}
        </div>
      )}
      {current && <Detail m={current} me={me} balance={balance} proposals={proposals} off={off} why={why} run={run} />}
      <Create me={me} off={off} why={why} info={info} run={run} />
    </section>
  );
}

function Detail(p: { m: Multisig; me: string; balance: bigint | null; proposals: Proposal[] | null; off: boolean; why: string; run: (what: string, msgs: AnyMsg[]) => Promise<boolean> }) {
  const { m } = p;
  const [to, setTo] = useState("");
  const [amount, setAmount] = useState("");
  const [title, setTitle] = useState("");
  const dest = norm(to);
  const uqor = parseQor(amount);
  const toOk = isQorAddress(dest);
  const can = !p.off && toOk && uqor !== null && uqor > 0n && title.trim().length > 0 && (p.balance === null || uqor <= p.balance);
  const need = !to ? "Enter who gets paid." : !toOk ? "That address is not valid." : !amount || uqor === null || uqor === 0n ? "Enter an amount." : p.balance !== null && uqor > p.balance ? "More than the multisig holds." : !title.trim() ? "Give the proposal a title." : "";
  const copy = () => void navigator.clipboard?.writeText(m.policy).catch(() => undefined);

  return (
    <div className="msig">
      <div className="msig-head">
        <div>
          <div className="label">Multisig address (send funds here)</div>
          <div className="mono small break">{m.policy}</div>
        </div>
        <button className="ghost" onClick={copy}>Copy address</button>
      </div>
      <div className="msig-facts">
        <span className="pill">{p.balance === null ? "balance …" : `${formatQor(p.balance, 4)} QOR`}</span>
        <span className="pill">{m.threshold} of {m.members.length} must vote yes</span>
        <span className="pill">voting {durationText(m.votingSeconds)}</span>
      </div>
      <div className="muted small">
        Members: {m.members.map((x, i) => <span key={x.address} className="mono">{i ? ", " : ""}{x.address === p.me ? "you" : short(x.address, 8, 4)}</span>)}
      </div>

      <h3>Propose a payment</h3>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (!can || uqor === null) return;
          if (await p.run(`Propose paying ${formatQor(uqor)} QOR`, [proposePaymentMsg({ policy: m.policy, proposer: p.me, to: dest, uqor, title: title.trim() })])) {
            setTo(""); setAmount(""); setTitle("");
          }
        }}
      >
        <label>Pay to<input className="mono" value={to} onChange={(e) => setTo(e.target.value)} placeholder="qor1…" spellCheck={false} autoComplete="off" /></label>
        <label>Amount (QOR)<input className="mono" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.0" inputMode="decimal" autoComplete="off" /></label>
        <label>Title<input value={title} maxLength={60} onChange={(e) => setTitle(e.target.value)} placeholder="What is this for?" autoComplete="off" /></label>
        <div className="row">
          <button className="primary" type="submit" disabled={!can}>Propose on the device</button>
          <span className={`small ${can ? "muted" : "warn-text"}`}>{can ? "Nothing moves until enough members vote yes." : p.why || need}</span>
        </div>
      </form>

      <h3>Proposals</h3>
      {p.proposals === null && <p className="muted">Loading…</p>}
      {p.proposals && p.proposals.length === 0 && <p className="muted">No proposals yet.</p>}
      <ul className="proposals">
        {p.proposals?.map((pr) => <ProposalRow key={pr.id} pr={pr} m={m} me={p.me} off={p.off} run={p.run} />)}
      </ul>
    </div>
  );
}

function ProposalRow(p: { pr: Proposal; m: Multisig; me: string; off: boolean; run: (what: string, msgs: AnyMsg[]) => Promise<boolean> }) {
  const { pr } = p;
  const open = isOpen(pr) && !canExecute(pr, p.m.threshold);
  const ready = canExecute(pr, p.m.threshold);
  const vote = (o: VoteOption) => p.run(`Vote ${VOTE_NAME[o]} on proposal #${pr.id}`, [voteMsg(BigInt(pr.id), p.me, o)]);
  return (
    <li>
      <div className="row between">
        <b>#{pr.id} {pr.title}</b>
        <span className={`pill ${ready ? "good" : open ? "warn" : ""}`}>{statusText(pr, p.m.threshold)}</span>
      </div>
      {pr.actions.map((a, i) => (
        <div key={i} className="muted small">
          {a.kind === "send" ? <>Pays <b>{formatQor(a.amount ?? 0n)} QOR</b> to <span className="mono">{short(a.to ?? "", 12, 8)}</span></> : <>Does: {a.text} (the device cannot show this one fully)</>}
        </div>
      ))}
      <div className="muted small">
        Yes {pr.tally.yes} · No {pr.tally.no} · Abstain {pr.tally.abstain}{pr.tally.veto ? ` · Veto ${pr.tally.veto}` : ""} · needs {p.m.threshold}
        {pr.voted ? ` · you voted ${VOTE_NAME[pr.voted]}` : ""}
      </div>
      {open && !pr.voted && (
        <div className="row">
          <button className="primary" disabled={p.off} onClick={() => void vote(1)}>Vote yes</button>
          <button className="ghost" disabled={p.off} onClick={() => void vote(3)}>No</button>
          <button className="ghost" disabled={p.off} onClick={() => void vote(2)}>Abstain</button>
        </div>
      )}
      {ready && (
        <div className="row">
          <button className="primary" disabled={p.off} onClick={() => void p.run(`Run proposal #${pr.id}`, [execMsg(BigInt(pr.id), p.me)])}>Run it (pays out)</button>
        </div>
      )}
      {(open && !pr.voted) || ready ? (
        <div className="muted small">The device shows the proposal number (#{pr.id}) and your choice. It cannot read the chain: check the number matches.</div>
      ) : null}
    </li>
  );
}

function Create(p: { me: string; off: boolean; why: string; info: NetworkInfo; run: (what: string, msgs: AnyMsg[]) => Promise<boolean> }) {
  const [name, setName] = useState("");
  const [others, setOthers] = useState("");
  const [threshold, setThreshold] = useState(2);
  const [hours, setHours] = useState(24);
  const addrs = useMemo(() => others.split(/[\s,;]+/).map(norm).filter(Boolean), [others]);
  const members = useMemo(() => [p.me, ...addrs.filter((a) => a !== p.me)], [p.me, addrs]);
  const bad = addrs.find((a) => !isQorAddress(a));
  const dup = new Set(addrs).size !== addrs.length || addrs.includes(p.me);
  const n = members.length;
  const th = Math.min(Math.max(threshold, 1), n);
  const need = !addrs.length ? "Add at least one other member." : bad ? `Not a valid address: ${short(bad, 12, 6)}` : dup ? "A member is listed twice (you are added for you)." : n > MAX_MEMBERS ? `The device shows every member: at most ${MAX_MEMBERS} including you.` : hours < 1 ? "Voting lasts at least 1 hour." : "";
  const can = !p.off && !need;
  return (
    <details className="msig-create">
      <summary>Create a multisig</summary>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (!can) return;
          if (await p.run(`Create a ${th} of ${n} multisig`, [createMultisigMsg({ admin: p.me, members, threshold: th, votingSeconds: Math.round(hours * 3600), name: name.trim() })])) {
            setName(""); setOthers("");
          }
        }}
      >
        <label>Name (optional)<input value={name} maxLength={30} onChange={(e) => setName(e.target.value)} placeholder="Team funds" autoComplete="off" /></label>
        <label>
          Other members (one address per line)
          <textarea className="mono" rows={3} value={others} onChange={(e) => setOthers(e.target.value)} placeholder={"qor1…\nqor1…"} spellCheck={false} />
        </label>
        <div className="row">
          <label>Approvals needed
            <select value={th} onChange={(e) => setThreshold(Number(e.target.value))}>
              {Array.from({ length: Math.max(n, 1) }, (_, i) => i + 1).map((v) => <option key={v} value={v}>{v} of {n}</option>)}
            </select>
          </label>
          <label>Voting time (hours)<input type="number" min={1} max={720} value={hours} onChange={(e) => setHours(Number(e.target.value))} /></label>
        </div>
        <div className="row">
          <button className="primary" type="submit" disabled={!can}>Create on the device</button>
          <span className={`small ${can ? "muted" : "warn-text"}`}>{can ? `You and ${n - 1} other${n - 1 === 1 ? "" : "s"}; every member has one vote.` : p.why || need}</span>
        </div>
      </form>
    </details>
  );
}
