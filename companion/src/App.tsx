import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fromHex, toHex } from "@qorechain/qoresign";
import { bridgeUp, device, DeviceError, fundDevnet, loadNetworks, type DeviceAccount, type Security } from "./bridge";
import {
  broadcast, gasFor, getAccount, getBalance, history, nodeChainId, pqStatus, signBytes, waitForTx,
  type Activity, type ChainAccount,
} from "./chain";
import { DEFAULT_NETWORKS, NETWORK_IDS, type NetworkId, type NetworkInfo } from "./networks";
import { buildTx, DEFAULT_GAS, fee, formatQor, isQorAddress, parseQor, registerPqKeyMsg, sendMsg, UQOR } from "./tx";

const short = (s: string, a = 10, b = 6) => (s.length > a + b + 2 ? `${s.slice(0, a)}…${s.slice(-b)}` : s);
const ACCOUNTS = 8;

function saved<T extends string>(key: string, allowed: readonly T[], dflt: T): T {
  try {
    const v = localStorage.getItem(key) as T | null;
    return v && allowed.includes(v) ? v : dflt;
  } catch {
    return dflt;
  }
}

function explain(e: unknown): string {
  if (e instanceof DeviceError) {
    if (e.rejected) return "Rejected on the device: nothing was signed.";
    if (e.refused) return `The device refused to sign: ${e.message}`;
    return e.message;
  }
  return e instanceof Error ? e.message : String(e);
}

type Wait = null | { what: string; hint: string };

export function App() {
  const [networks, setNetworks] = useState<Record<NetworkId, NetworkInfo>>(DEFAULT_NETWORKS);
  const [net, setNet] = useState<NetworkId>(() => saved("qorebit.net", NETWORK_IDS, "devnet"));
  const [index, setIndex] = useState(0);
  const [bridge, setBridge] = useState<boolean | null>(null);
  const [security, setSecurity] = useState<Security | null>(null);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  const [acct, setAcct] = useState<DeviceAccount | null>(null);
  const [balance, setBalance] = useState<bigint | null>(null);
  const [chainAcct, setChainAcct] = useState<ChainAccount | null | undefined>(undefined);
  const [pqRegistered, setPqRegistered] = useState<boolean | null>(null);
  const [activity, setActivity] = useState<Activity[] | null>(null);
  const [chainError, setChainError] = useState<string | null>(null);
  const [wait, setWait] = useState<Wait>(null);
  const [notice, setNotice] = useState<{ tone: "good" | "bad" | "warn"; text: string } | null>(null);
  const info = networks[net];
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true; // StrictMode mounts, unmounts and mounts again in development
    return () => void (alive.current = false);
  }, []);
  useEffect(() => {
    loadNetworks().then(setNetworks);
  }, []);
  useEffect(() => {
    try {
      localStorage.setItem("qorebit.net", net);
    } catch {
      /* private mode */
    }
  }, [net]);

  // the bridge and the device
  const loadDevice = useCallback(async () => {
    const up = await bridgeUp();
    setBridge(up);
    if (!up) return;
    try {
      setSecurity(await device.security());
      setDeviceError(null);
    } catch (e) {
      setDeviceError(explain(e));
    }
  }, []);
  useEffect(() => {
    void loadDevice();
    const t = setInterval(() => void loadDevice(), 5000);
    return () => clearInterval(t);
  }, [loadDevice]);

  // the selected account's keys, from the device
  useEffect(() => {
    setAcct(null);
    if (!security) return;
    device.account(index).then(setAcct, (e) => setDeviceError(explain(e)));
  }, [index, security === null]); // eslint-disable-line react-hooks/exhaustive-deps

  // the chain's view of it
  const refresh = useCallback(async () => {
    if (!acct) return;
    try {
      const [b, a, pq, act] = await Promise.all([
        getBalance(net, acct.bech32),
        getAccount(net, acct.bech32),
        pqStatus(net, acct.bech32).then((s) => s.registered),
        history(net, acct.bech32).catch(() => []),
      ]);
      if (!alive.current) return;
      setBalance(b);
      setChainAcct(a);
      setPqRegistered(pq);
      setActivity(act);
      setChainError(null);
      // the device's home screen shows the same balance, address and network
      device.home(index, b, info.chainId, acct.bech32).catch(() => undefined);
    } catch (e) {
      if (alive.current) setChainError(explain(e));
    }
  }, [acct, net, index, info.chainId]);
  useEffect(() => {
    setBalance(null);
    setChainAcct(undefined);
    setPqRegistered(null);
    setActivity(null);
    setChainError(null);
    void refresh();
    const t = setInterval(() => void refresh(), 6000);
    return () => clearInterval(t);
  }, [refresh]);

  /** Refuse to sign for a node that is on another chain than the network selected. */
  const ensureChain = useCallback(async () => {
    const reported = await nodeChainId(net);
    if (reported !== info.chainId) throw new Error(`the ${info.name} node reports chain "${reported}", expected "${info.chainId}": nothing was signed`);
  }, [net, info]);

  const needAccount = () => {
    if (!acct) throw new Error("the device has not answered yet");
    return acct;
  };

  async function registerKey() {
    setNotice(null);
    try {
      const a = needAccount();
      if (!chainAcct) throw new Error("this account does not exist on chain yet: send it some QOR first");
      await ensureChain();
      setWait({ what: "Prove your post-quantum key", hint: "Check the device: hold to approve, then your PIN." });
      const [proof, pk] = await Promise.all([device.registerProof(index, info.chainId), device.pqPublicKey(index)]);
      const classical = fromHex(a.publicKey);
      const msg = registerPqKeyMsg(a.bech32, fromHex(pk.publicKey), classical, fromHex(proof.proof));
      const { body, authInfo } = buildTx({ messages: [msg], publicKey: classical, sequence: chainAcct.sequence, gas: DEFAULT_GAS });
      setWait({ what: "Register the key", hint: "The device shows the registration. Hold to approve, then your PIN." });
      const signed = await device.signTx({
        account: index, accountNumber: chainAcct.accountNumber, chainId: info.chainId, body: toHex(body), authInfo: toHex(authInfo),
        signBytes: await signBytes(net, info.chainId), classicalOnly: true,
      });
      setWait({ what: "Broadcasting", hint: "" });
      const r = await broadcast(net, fromHex(signed.txBytes));
      if (r.code !== 0) throw new Error(`the chain refused it (code ${r.code}): ${r.log}`);
      const done = await waitForTx(net, r.txhash);
      if (done.code !== 0) throw new Error(`it failed on chain: ${done.rawLog}`);
      setNotice({ tone: "good", text: "Post-quantum key registered. Transfers from this account are now accepted." });
      await refresh();
    } catch (e) {
      setNotice({ tone: "bad", text: explain(e) });
    } finally {
      setWait(null);
    }
  }

  async function send(to: string, amount: bigint, memo: string) {
    setNotice(null);
    try {
      const a = needAccount();
      if (!chainAcct) throw new Error("this account does not exist on chain yet");
      if (!pqRegistered) throw new Error("register the post-quantum key first: the chain refuses a transfer without it");
      await ensureChain();
      const classical = fromHex(a.publicKey);
      const msgs = [sendMsg(a.bech32, to, amount)];
      // gas: simulated by the node (with a placeholder post-quantum layer), then the real fee
      const probe = buildTx({ messages: msgs, memo, publicKey: classical, sequence: chainAcct.sequence, gas: DEFAULT_GAS });
      const { gas } = await gasFor(net, probe.body, probe.authInfo);
      const { body, authInfo } = buildTx({ messages: msgs, memo, publicKey: classical, sequence: chainAcct.sequence, gas });
      if ((balance ?? 0n) < amount + fee(gas)) throw new Error(`not enough QOR: ${formatQor(amount)} plus the fee of ${formatQor(fee(gas))}`);
      setWait({ what: `Send ${formatQor(amount)} QOR`, hint: "Check the device screen: the amount, the address and the network. Hold the button to approve, tap to reject." });
      const signed = await device.signTx({
        account: index, accountNumber: chainAcct.accountNumber, chainId: info.chainId, body: toHex(body), authInfo: toHex(authInfo),
        signBytes: await signBytes(net, info.chainId),
      });
      setWait({ what: "Broadcasting", hint: "" });
      const r = await broadcast(net, fromHex(signed.txBytes));
      if (r.code !== 0) throw new Error(`the chain refused it (code ${r.code}): ${r.log}`);
      const done = await waitForTx(net, r.txhash);
      if (done.code !== 0) throw new Error(`it failed on chain: ${done.rawLog}`);
      setNotice({ tone: "good", text: `Sent ${formatQor(amount)} QOR. Transaction ${short(r.txhash, 10, 8)} in block ${done.height}.` });
      await refresh();
      return true;
    } catch (e) {
      setNotice({ tone: "bad", text: explain(e) });
      return false;
    } finally {
      setWait(null);
    }
  }

  async function deviceAction(what: string, hint: string, run: () => Promise<unknown>, done: string) {
    setNotice(null);
    setWait({ what, hint });
    try {
      await run();
      setNotice({ tone: "good", text: done });
      await loadDevice();
    } catch (e) {
      setNotice({ tone: "bad", text: explain(e) });
    } finally {
      setWait(null);
    }
  }

  async function faucet() {
    if (!acct) return;
    setNotice(null);
    setWait({ what: "Requesting test QOR", hint: "From the local devnet's faucet." });
    try {
      await fundDevnet(acct.bech32, 1000n * UQOR);
      setNotice({ tone: "good", text: "1,000 test QOR sent." });
      await refresh();
    } catch (e) {
      setNotice({ tone: "bad", text: explain(e) });
    } finally {
      setWait(null);
    }
  }

  const ready = bridge && security && !deviceError;

  return (
    <div className={`app ${info.real ? "is-real" : ""}`}>
      <Header net={net} networks={networks} onNet={(n) => { setNotice(null); setNet(n); }} bridge={bridge} security={security} />
      {info.real && (
        <div className="banner real" role="status">
          <b>Mainnet.</b> Real funds. The device screen says MAINNET on every transaction: read it before you hold the button.
        </div>
      )}
      <main>
        {bridge === false && <BridgeDown />}
        {bridge && deviceError && <DeviceDown message={deviceError} />}
        {ready && !security.pinSet && <PinSetup onSet={() => deviceAction("Set a signing PIN", "On the device: eight presses, tap = 0, hold = 1. Then once more to confirm.", device.setPin, "Signing PIN set.")} />}
        {notice && <div className={`notice ${notice.tone}`} role="status">{notice.text}</div>}
        {ready && (
          <>
            <AccountPicker index={index} onIndex={setIndex} />
            <div className="grid">
              <div className="col">
                <BalanceCard
                  info={info} acct={acct} balance={balance} chainAcct={chainAcct} pqRegistered={pqRegistered} chainError={chainError}
                  onRegister={registerKey} onFaucet={faucet} busy={!!wait}
                />
                <SendCard disabled={!!wait || !security.pinSet || !pqRegistered || !chainAcct} balance={balance} info={info} onSend={send} why={
                  !security.pinSet ? "Set the signing PIN first." : !chainAcct ? "This account has no funds on this network yet." : !pqRegistered ? "Register the post-quantum key first." : ""
                } />
              </div>
              <div className="col">
                <ActivityCard activity={activity} />
                <DeviceCard security={security} busy={!!wait} act={deviceAction} />
              </div>
            </div>
          </>
        )}
      </main>
      {wait && <WaitOverlay wait={wait} />}
      <footer>
        Qorebit wallet · QoreChain hybrid signatures: secp256k1 + ML-DSA-87 · keys never leave the device · a research prototype, unaudited
      </footer>
    </div>
  );
}

function Header(p: { net: NetworkId; networks: Record<NetworkId, NetworkInfo>; onNet: (n: NetworkId) => void; bridge: boolean | null; security: Security | null }) {
  return (
    <header>
      <div className="brand"><span className="mark" aria-hidden />QOREBIT</div>
      <div className="seg" role="radiogroup" aria-label="Network">
        {NETWORK_IDS.map((id) => (
          <button key={id} role="radio" aria-checked={p.net === id} className={`${p.net === id ? "on" : ""} ${id}`} onClick={() => p.onNet(id)} title={p.networks[id].chainId}>
            {p.networks[id].name}
          </button>
        ))}
      </div>
      <div className="chip-row">
        <span className="chain-id mono">{p.networks[p.net].chainId}</span>
        <span className={`dot ${p.bridge && p.security ? "ok" : "bad"}`} aria-hidden />
        <span className="muted small">{p.bridge === null ? "…" : !p.bridge ? "no bridge" : p.security ? (p.security.chip ? "Dabao" : "emulator") : "no device"}</span>
      </div>
    </header>
  );
}

function BridgeDown() {
  return (
    <section className="card center">
      <h2>The bridge is not running</h2>
      <p className="muted">The wallet talks to the device and to the networks through a small local program.</p>
      <pre className="code">make web          # the board, a window for its screen, and this site
make web-emu      # the same on the emulator</pre>
    </section>
  );
}

function DeviceDown({ message }: { message: string }) {
  return (
    <section className="card center">
      <h2>No device answers</h2>
      <p className="muted">{message}</p>
      <p className="muted small">Plug the Dabao in (or start the emulator). The page checks again every few seconds.</p>
    </section>
  );
}

function ButtonLegend() {
  return (
    <div className="legend" aria-label="The device's one button">
      <div><b>TAP</b><span>= 0 · reject</span></div>
      <div><b>HOLD</b><span>= 1 · approve</span></div>
    </div>
  );
}

function PinSetup({ onSet }: { onSet: () => void }) {
  return (
    <section className="card accent">
      <h2>Set your signing PIN</h2>
      <p>The device asks for it after every approval. It is eight presses of the one button: a tap is 0, a hold is 1. Five wrong tries lock signing until you enter your 24 recovery words on the device.</p>
      <ButtonLegend />
      <button className="primary" onClick={onSet}>Set the PIN on the device</button>
    </section>
  );
}

function AccountPicker({ index, onIndex }: { index: number; onIndex: (i: number) => void }) {
  return (
    <nav className="accounts" aria-label="Account">
      {Array.from({ length: ACCOUNTS }, (_, i) => (
        <button key={i} className={i === index ? "on" : ""} onClick={() => onIndex(i)} aria-pressed={i === index}>
          Account {i + 1}
        </button>
      ))}
    </nav>
  );
}

function Copy({ text, label = "Copy" }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  return (
    <button className="ghost" onClick={() => navigator.clipboard?.writeText(text).then(() => { setDone(true); setTimeout(() => setDone(false), 1200); })}>
      {done ? "Copied" : label}
    </button>
  );
}

function BalanceCard(p: {
  info: NetworkInfo; acct: DeviceAccount | null; balance: bigint | null; chainAcct: ChainAccount | null | undefined; pqRegistered: boolean | null;
  chainError: string | null; onRegister: () => void; onFaucet: () => void; busy: boolean;
}) {
  const groups = p.acct ? p.acct.bech32.match(/.{1,4}/g)! : [];
  return (
    <section className="card">
      <div className="label">Balance · {p.info.name}</div>
      <div className="balance mono">
        {p.balance === null ? "—" : formatQor(p.balance, 4)} <span className="unit">QOR</span>
      </div>
      <div className="label">Address (check it on the device)</div>
      <div className="address mono" aria-label={p.acct?.bech32}>
        {groups.map((g, i) => <span key={i} className={i % 2 ? "alt" : ""}>{g}</span>)}
      </div>
      <div className="row">
        {p.acct && <Copy text={p.acct.bech32} label="Copy address" />}
        <span className={`pill ${p.pqRegistered ? "good" : p.pqRegistered === false ? "warn" : ""}`}>
          {p.pqRegistered === null ? "post-quantum key: …" : p.pqRegistered ? "post-quantum key registered" : "post-quantum key not registered"}
        </span>
      </div>
      {p.chainError && <p className="notice bad">{p.chainError}</p>}
      {p.chainAcct === null && (
        <div className="hint">
          <p>This address has not received anything on {p.info.name} yet, so the chain does not know it.</p>
          {p.info.faucet === "bridge" && <button className="primary" disabled={p.busy} onClick={p.onFaucet}>Get 1,000 test QOR</button>}
          {p.info.faucet && p.info.faucet !== "bridge" && <a className="btn" href={p.info.faucet} target="_blank" rel="noopener noreferrer">Open the {p.info.name} faucet</a>}
          {!p.info.faucet && <p className="muted small">Send QOR to the address above from another wallet.</p>}
        </div>
      )}
      {p.chainAcct && p.pqRegistered === false && (
        <div className="hint">
          <p>QoreChain refuses a transfer until the account's post-quantum key is registered. The device proves the key and signs the registration (classical-only, once). It costs a small fee.</p>
          <button className="primary" disabled={p.busy} onClick={p.onRegister}>Register the post-quantum key</button>
        </div>
      )}
      {p.chainAcct && p.pqRegistered && p.info.faucet === "bridge" && (
        <div className="row"><button className="ghost" disabled={p.busy} onClick={p.onFaucet}>Get 1,000 test QOR</button></div>
      )}
    </section>
  );
}

function SendCard(p: { disabled: boolean; why: string; balance: bigint | null; info: NetworkInfo; onSend: (to: string, amount: bigint, memo: string) => Promise<boolean | undefined> }) {
  const [to, setTo] = useState("");
  const [amount, setAmount] = useState("");
  const [memo, setMemo] = useState("");
  const parsed = useMemo(() => parseQor(amount), [amount]);
  const toOk = isQorAddress(to.trim());
  const problem = !to ? "" : !toOk ? "Not a valid qor1… address." : "";
  const amountProblem = !amount ? "" : parsed === null || parsed === 0n ? "An amount in QOR, up to 6 decimals." : p.balance !== null && parsed > p.balance ? "More than the balance." : "";
  const can = !p.disabled && toOk && parsed !== null && parsed > 0n && !amountProblem;
  return (
    <section className="card">
      <h2>Send</h2>
      <form onSubmit={async (e) => { e.preventDefault(); if (can && (await p.onSend(to.trim(), parsed!, memo))) { setTo(""); setAmount(""); setMemo(""); } }}>
        <label>To<input className="mono" value={to} onChange={(e) => setTo(e.target.value)} placeholder="qor1…" spellCheck={false} autoComplete="off" /></label>
        {problem && <div className="field-error">{problem}</div>}
        <label>Amount (QOR)<input className="mono" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.0" inputMode="decimal" autoComplete="off" /></label>
        {amountProblem && <div className="field-error">{amountProblem}</div>}
        <label>Memo (optional)<input value={memo} maxLength={120} onChange={(e) => setMemo(e.target.value)} autoComplete="off" /></label>
        <div className="row">
          <button className="primary" type="submit" disabled={!can}>Review on the device</button>
          <span className="muted small">{p.why || (p.info.real ? "Mainnet: real funds." : "Network fee about 0.02 QOR.")}</span>
        </div>
      </form>
      <ButtonLegend />
    </section>
  );
}

function ActivityCard({ activity }: { activity: Activity[] | null }) {
  return (
    <section className="card">
      <h2>Activity</h2>
      {activity === null && <p className="muted">Loading…</p>}
      {activity && activity.length === 0 && <p className="muted">Nothing yet on this network.</p>}
      <ul className="activity">
        {activity?.map((a) => (
          <li key={a.hash} className={a.ok ? "" : "failed"}>
            <span className={`kind ${a.kind}`}>{a.kind === "sent" ? "↑" : a.kind === "received" ? "↓" : "•"}</span>
            <span className="what">
              <b>{a.kind === "sent" ? "Sent" : a.kind === "received" ? "Received" : "Transaction"}</b>
              {a.peer && <span className="muted mono small"> {a.kind === "sent" ? "to" : "from"} {short(a.peer, 8, 6)}</span>}
              <span className="muted mono small block">{short(a.hash, 8, 6)} · block {a.height}{a.ok ? "" : " · failed"}</span>
            </span>
            <span className={`amt mono ${a.kind}`}>{a.amount !== null ? `${a.kind === "sent" ? "−" : a.kind === "received" ? "+" : ""}${formatQor(a.amount, 4)}` : ""}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function DeviceCard(p: { security: Security; busy: boolean; act: (what: string, hint: string, run: () => Promise<unknown>, done: string) => Promise<void> }) {
  const s = p.security;
  const chipNote = !s.chip ? "Emulator: nothing here is secret." : s.possibleAttacks > 0 ? `${s.possibleAttacks} glitch/tamper alarms recorded.` : s.developerMode ? "Developer-signed firmware: a reflash could read the seed." : "Secure boot.";
  return (
    <section className="card">
      <h2>Device</h2>
      <dl className="facts">
        <dt>Signing PIN</dt><dd>{s.pinSet ? `set · ${s.pinTriesLeft} tries left` : "not set"}</dd>
        <dt>Seed storage</dt><dd>{s.seedStorage === "wrapped" ? "wrapped by the chip" : s.seedStorage === "hostfile" ? "host file (emulator)" : s.seedStorage}</dd>
        <dt>Chip</dt><dd>{chipNote}</dd>
        <dt>Firmware</dt><dd className="mono">{s.firmware}</dd>
        {s.passphrase && (<><dt>Wallet</dt><dd>passphrase wallet in use</dd></>)}
      </dl>
      <div className="actions">
        <button className="ghost" disabled={p.busy} onClick={() => p.act(s.pinSet ? "Change the PIN" : "Set a PIN", "On the device: tap = 0, hold = 1.", device.setPin, "PIN set.")}>{s.pinSet ? "Change PIN" : "Set PIN"}</button>
        <button className="ghost" disabled={p.busy} onClick={() => p.act("Show the recovery words", "On the device: your PIN, then the 24 words. Nobody should see them.", device.showPhrase, "Recovery words shown on the device.")}>Show recovery words</button>
        <button className="ghost" disabled={p.busy} onClick={() => p.act("Forgot the PIN", "On the device: enter your 24 words (about 12 presses a word), then a new PIN.", device.resetPin, "PIN reset.")}>Forgot PIN</button>
        <button className="ghost" disabled={p.busy} onClick={() => p.act("Passphrase", "On the device: a passphrase opens a separate wallet until restart.", device.passphrase, "Passphrase set on the device.")}>Passphrase</button>
        <button className="ghost" disabled={p.busy} onClick={() => p.act("Restore a wallet", "On the device: enter 24 words. This replaces the wallet on the device.", device.restore, "Wallet restored.")}>Restore</button>
        <button className="ghost danger" disabled={p.busy} onClick={() => p.act("Erase the wallet", "The device asks twice. Without your 24 words the funds are lost.", device.wipe, "Wallet erased: a new one was made.")}>Erase</button>
      </div>
    </section>
  );
}

function WaitOverlay({ wait }: { wait: NonNullable<Wait> }) {
  return (
    <div className="overlay" role="alertdialog" aria-live="assertive" aria-label={wait.what}>
      <div className="sheet">
        <div className="spinner" aria-hidden />
        <h2>{wait.what}</h2>
        {wait.hint && <p>{wait.hint}</p>}
        {wait.hint && <ButtonLegend />}
      </div>
    </div>
  );
}
