import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fromHex, toHex } from "@qorechain/qoresign";
import { bridgeStatus, device, DeviceError, fundDevnet, loadNetworks, type DeviceAccount, type Security } from "./bridge";
import {
  broadcast, gasFor, getAccount, getBalance, history, nodeChainId, pqStatus, signBytes, waitForTx,
  type Activity, type ChainAccount,
} from "./chain";
import { MultisigCard } from "./Multisig";
import { TokensCard } from "./Tokens";
import { SignInCard } from "./SignIn";
import { CrossVmCard } from "./CrossVm";
import { SmartAccountCard } from "./SmartAccount";
import { qorToEvmHex } from "./crossVmMsg";
import { ext } from "./ext";
import { PasswordCard } from "./LockScreen";
import { lockAvailable, lockNow } from "./lock";
import { ACCOUNTS, AccountMenu, Icon, NetworkMenu, ScreenHead, TabBar, Tile, accountName, type View } from "./Shell";
import { DEFAULT_NETWORKS, NETWORK_IDS, type NetworkId, type NetworkInfo } from "./networks";
import { buildTx, DEFAULT_GAS, fee, formatQor, isQorAddress, parseQor, registerPqKeyMsg, sendMsg, UQOR, type AnyMsg, type KeyType } from "./tx";

const short = (s: string, a = 10, b = 6) => (s.length > a + b + 2 ? `${s.slice(0, a)}…${s.slice(-b)}` : s);
const keyTypeOf = (index: number): KeyType => (index >= ACCOUNTS ? "eth_secp256k1" : "secp256k1");

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
  const [index, setIndexState] = useState(() => { try { const n = Number(localStorage.getItem("qorebit.index")); return Number.isInteger(n) && n >= 0 && n < 2 * ACCOUNTS ? n : 0; } catch { return 0; } });
  const [view, setView] = useState<View>("home");
  const setIndex = (i: number) => { setIndexState(i); setNotice(null); try { localStorage.setItem("qorebit.index", String(i)); } catch { /* private mode */ } };
  const [bridge, setBridge] = useState<boolean | null>(null);
  const [originBlocked, setOriginBlocked] = useState(false);
  const [security, setSecurity] = useState<Security | null>(null);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  const [lastTx, setLastTx] = useState<{ hash: string; at: number } | null>(null);
  const [deviceBusy, setDeviceBusy] = useState(false); // the device is waiting for a button press
  const [acct, setAcct] = useState<DeviceAccount | null>(null);
  const [acctFor, setAcctFor] = useState(-1); // the account index `acct` belongs to
  const [balance, setBalance] = useState<bigint | null>(null);
  const [chainAcct, setChainAcct] = useState<ChainAccount | null | undefined>(undefined);
  const [pqRegistered, setPqRegistered] = useState<boolean | null>(null);
  const [activity, setActivity] = useState<Activity[] | null>(null);
  const [chainError, setChainError] = useState<string | null>(null);
  const [wait, setWait] = useState<Wait>(null);
  const [notice, setNotice] = useState<{ tone: "good" | "bad" | "warn"; text: string } | null>(null);
  const info = networks[net];
  const alive = useRef(true);
  const generation = useRef(0); // bumped whenever the account or network changes: older answers are dropped

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
    const status = await bridgeStatus();
    const up = status === "up";
    setOriginBlocked(status === "origin");
    setBridge(up);
    if (!up) return;
    try {
      setSecurity(await device.security());
      setDeviceError(null);
      setDeviceBusy(false);
    } catch (e) {
      // the device is busy with an approval or a PIN: keep what is shown, this poll will be answered next time
      if (e instanceof DeviceError && e.code === 8) {
        setDeviceBusy(true);
        return;
      }
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
    device.account(index).then((a) => { setAcct(a); setAcctFor(index); }, (e) => setDeviceError(explain(e)));
  }, [index, security === null]); // eslint-disable-line react-hooks/exhaustive-deps

  // the chain's view of it
  const refresh = useCallback(async () => {
    if (!acct || acctFor !== index) return; // right after a switch `acct` is still the previous account
    const mine = generation.current;
    try {
      const [b, a, pq, act] = await Promise.all([
        getBalance(net, acct.bech32),
        getAccount(net, acct.bech32),
        pqStatus(net, acct.bech32).then((s) => s.registered),
        history(net, acct.bech32).catch(() => []),
      ]);
      if (!alive.current || mine !== generation.current) return; // the account or network changed meanwhile
      setBalance(b);
      setChainAcct(a);
      setPqRegistered(pq);
      setActivity(act);
      setChainError(null);
      // the device's home screen shows the same balance, address and network
      // only the tab you are looking at: another open tab on a different account would overwrite it
      if (!document.hidden) device.home(index, b, info.chainId, acct.bech32).catch(() => undefined);
    } catch (e) {
      if (alive.current && mine === generation.current) setChainError(explain(e));
    }
  }, [acct, acctFor, net, index, info.chainId]);
  useEffect(() => {
    generation.current++;
    setBalance(null);
    setChainAcct(undefined);
    setPqRegistered(null);
    setActivity(null);
    setChainError(null);
    void refresh();
    const t = setInterval(() => void refresh(), 6000);
    const onShow = () => { if (!document.hidden) void refresh(); };
    document.addEventListener("visibilitychange", onShow);
    return () => { clearInterval(t); document.removeEventListener("visibilitychange", onShow); };
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
      setWait({ what: "Register the key", hint: "Checking the network…" });
      await ensureChain();
      setWait({ what: "Prove your post-quantum key", hint: "Check the device: press the right button to approve, then your PIN." });
      // one after the other: the device runs one job at a time
      const proof = await device.registerProof(index, info.chainId);
      const pk = await device.pqPublicKey(index);
      const classical = fromHex(a.publicKey);
      const msg = registerPqKeyMsg(a.bech32, fromHex(pk.publicKey), classical, fromHex(proof.proof));
      const { body, authInfo } = buildTx({ messages: [msg], publicKey: classical, sequence: chainAcct.sequence, gas: DEFAULT_GAS, keyType: keyTypeOf(index) });
      setWait({ what: "Register the key", hint: "The device shows the registration. Press the right button to approve, then your PIN." });
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
      // feedback at once: the checks below talk to the network and can take a few seconds
      setWait({ what: `Send ${formatQor(amount)} QOR`, hint: "Preparing the transaction: checking the network and the fee…" });
      await ensureChain();
      const classical = fromHex(a.publicKey);
      const msgs = [sendMsg(a.bech32, to, amount)];
      // gas: simulated by the node (with a placeholder post-quantum layer), then the real fee
      const probe = buildTx({ messages: msgs, memo, publicKey: classical, sequence: chainAcct.sequence, gas: DEFAULT_GAS, keyType: keyTypeOf(index) });
      const { gas } = await gasFor(net, probe.body, probe.authInfo);
      const { body, authInfo } = buildTx({ messages: msgs, memo, publicKey: classical, sequence: chainAcct.sequence, gas, keyType: keyTypeOf(index) });
      if ((balance ?? 0n) < amount + fee(gas)) throw new Error(`not enough QOR: ${formatQor(amount)} plus the fee of ${formatQor(fee(gas))}`);
      setWait({ what: `Send ${formatQor(amount)} QOR`, hint: "Check the device screen: the amount, the address and the network. Right button approves, left button rejects." });
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

  /** One transaction with these messages from the selected account: fee from the node, the device reads and
   *  signs it, the network takes it. True when it was included and succeeded. (Used by the multisig card.) */
  async function submit(what: string, msgs: AnyMsg[], hint = "Check the device screen and compare it with what you asked for. Right button approves, left button rejects."): Promise<boolean> {
    setNotice(null);
    try {
      const a = needAccount();
      if (!chainAcct) throw new Error("this account does not exist on chain yet");
      if (!pqRegistered) throw new Error("register the post-quantum key first: the chain refuses a transaction without it");
      setWait({ what, hint: "Preparing the transaction: checking the network and the fee…" });
      await ensureChain();
      const classical = fromHex(a.publicKey);
      const probe = buildTx({ messages: msgs, publicKey: classical, sequence: chainAcct.sequence, gas: DEFAULT_GAS, keyType: keyTypeOf(index) });
      let { gas } = await gasFor(net, probe.body, probe.authInfo);
      // a cross-VM call runs a second virtual machine: the simulation can fall short, so leave headroom
      if (msgs.some((m) => String(m.typeUrl).startsWith("/qorechain.crossvm."))) gas = gas * 2n > 1_500_000n ? gas * 2n : 1_500_000n;
      const { body, authInfo } = buildTx({ messages: msgs, publicKey: classical, sequence: chainAcct.sequence, gas, keyType: keyTypeOf(index) });
      if ((balance ?? 0n) < fee(gas)) throw new Error(`not enough QOR for the network fee of ${formatQor(fee(gas))}`);
      setWait({ what, hint });
      const signed = await device.signTx({
        account: index, accountNumber: chainAcct.accountNumber, chainId: info.chainId, body: toHex(body), authInfo: toHex(authInfo),
        signBytes: await signBytes(net, info.chainId),
      });
      setWait({ what: "Broadcasting", hint: "" });
      const r = await broadcast(net, fromHex(signed.txBytes));
      if (r.code !== 0) throw new Error(`the chain refused it (code ${r.code}): ${r.log}`);
      const done = await waitForTx(net, r.txhash);
      if (done.code !== 0) throw new Error(`it failed on chain: ${done.rawLog}`);
      setLastTx({ hash: r.txhash, at: Date.now() });
      setNotice({ tone: "good", text: `${what}: done. Transaction ${short(r.txhash, 10, 8)} in block ${done.height}.` });
      await refresh();
      return true;
    } catch (e) {
      setNotice({ tone: "bad", text: explain(e) });
      return false;
    } finally {
      setWait(null);
    }
  }

  /** Sign In With QoreChain: the device shows the site and the statement and signs both layers. */
  async function signIn(message: string) {
    setNotice(null);
    try {
      needAccount();
      setWait({ what: "Sign in with QoreChain", hint: "Check the device screen: the site, the network and the statement. Right button approves, left button rejects." });
      const r = await device.signSiwq(index, message);
      setNotice({ tone: "good", text: "Signed on the device. Paste the signature into the site." });
      return r;
    } catch (e) {
      setNotice({ tone: "bad", text: explain(e) });
      return null;
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
  const why = !!wait ? "Working…" : !security?.pinSet ? "Set the signing PIN first." : !chainAcct ? "This account has no funds on this network yet." : !pqRegistered ? "Register the post-quantum key first." : "";
  const go = (v: View) => { setNotice(null); setView(v); };
  const back = () => go(view === "multisig" || view === "smart" || view === "crossvm" || view === "signin" ? "apps" : "home");
  const evmHex = acct && index >= ACCOUNTS ? qorToEvmHex(acct.bech32) : null;

  // the extension's provider (websites asking for the account) reads which account and network are selected
  useEffect(() => {
    if (!acct || acctFor !== index) return;
    try {
      ext?.storage?.local?.set({ "qorebit.state": { address: acct.bech32, index, accountName: accountName(index), net, chainId: info.chainId } });
    } catch { /* not in the extension */ }
  }, [acct, acctFor, index, net, info.chainId]);

  return (
    <div className={`app ${info.real ? "is-real" : ""}`}>
      <header className="top">
        <div className="brand" title="Qorebit"><span className="mark" aria-hidden />QOREBIT</div>
        <AccountMenu index={index} onIndex={setIndex} address={acct?.bech32 ?? null} />
        <NetworkMenu net={net} networks={networks} onNet={(n) => { setNotice(null); setNet(n); }} />
        {lockAvailable() && (
          <button className="iconbtn" onClick={() => void lockNow()} title="Lock the wallet" aria-label="Lock the wallet"><Icon name="lock" /></button>
        )}
        <span className="status" title={bridge === null ? "…" : !bridge ? "bridge not running" : security ? (security.chip ? "Dabao connected" : "emulator connected") : "no device"}>
          <span className={`dot ${bridge && security ? "ok" : "bad"}`} aria-hidden />
        </span>
      </header>
      <main>
        {info.real && (
          <div className="banner real" role="status">
            <b>Mainnet.</b> Real funds. The device screen says MAINNET on every transaction: read it before you press the right button.
          </div>
        )}
        {bridge === false && (originBlocked ? <OriginBlocked /> : <BridgeDown />)}
        {bridge && deviceError && <DeviceDown message={deviceError} />}
        {bridge && deviceBusy && !deviceError && !security && (
          <section className="card center">
            <h2>The device is waiting for you</h2>
            <p className="muted">Answer on the device (right button approves, left button rejects), then this page carries on.</p>
          </section>
        )}
        {ready && !security.pinSet && <PinSetup onSet={() => deviceAction("Set a signing PIN", "On the device: eight presses, left = 0, right = 1. Then once more to confirm.", device.setPin, "Signing PIN set.")} />}
        {notice && <div className={`notice ${notice.tone}`} role="status">{notice.text}</div>}
        {ready && (
          <div className="screen">
            {view === "home" && (
              <>
                <Hero info={info} acct={acct} balance={balance} evmHex={evmHex} />
                <div className="quick">
                  <button onClick={() => go("send")}><span className="ico"><Icon name="send" /></span>Send</button>
                  <button onClick={() => go("receive")}><span className="ico"><Icon name="receive" /></span>Receive</button>
                  <button onClick={() => go("tokens")}><span className="ico"><Icon name="tokens" /></span>Tokens</button>
                  <button onClick={() => go("apps")}><span className="ico"><Icon name="apps" /></span>Apps</button>
                </div>
                <AccountStatus
                  info={info} chainAcct={chainAcct} pqRegistered={pqRegistered} chainError={chainError}
                  onRegister={registerKey} onFaucet={faucet} busy={!!wait}
                />
                <div className="recent-head"><h2>Recent activity</h2><button className="linkbtn" onClick={() => go("activity")}>See all</button></div>
                <ActivityCard activity={activity?.slice(0, 4) ?? null} bare />
              </>
            )}
            {view === "send" && (
              <>
                <ScreenHead title="Send" onBack={back} />
                <SendCard disabled={!!wait || !security.pinSet || !pqRegistered || !chainAcct} balance={balance} info={info} onSend={send} why={
                  !security.pinSet ? "Set the signing PIN first." : !chainAcct ? "This account has no funds on this network yet." : !pqRegistered ? "Register the post-quantum key first." : ""
                } />
              </>
            )}
            {view === "receive" && (
              <>
                <ScreenHead title="Receive" onBack={back} />
                <ReceiveCard info={info} acct={acct} evmHex={evmHex} />
              </>
            )}
            {view === "tokens" && (
              <>
                <ScreenHead title="Tokens" onBack={back} />
                <TokensCard net={net} info={info} acct={acct} qor={balance} onSubmit={submit} why={why} />
              </>
            )}
            {view === "activity" && (
              <>
                <ScreenHead title="Activity" />
                <ActivityCard activity={activity} />
              </>
            )}
            {view === "apps" && (
              <>
                <ScreenHead title="Apps" />
                <div className="tiles">
                  <Tile icon="signin" title="Sign in with QoreChain" text="Log in to websites with this account. The extension does it for you." onClick={() => go("signin")} />
                  <Tile icon="multisig" title="Multisig" text="Shared accounts: N of M must agree. Create, propose, vote, run." onClick={() => go("multisig")} />
                  <Tile icon="smart" title="Smart account" text="Spending rules the chain enforces for linked keys." onClick={() => go("smart")} />
                  <Tile icon="crossvm" title="Cross-VM calls" text="Call a contract on another VM. EVM to Native from an EVM account." onClick={() => go("crossvm")} />
                </div>
              </>
            )}
            {view === "signin" && (
              <>
                <ScreenHead title="Sign in with QoreChain" onBack={back} />
                <SignInCard acct={acct} index={index} chainId={info.chainId} onSign={signIn} why={!!wait ? "Working…" : !security.pinSet ? "Set the signing PIN first." : ""} />
              </>
            )}
            {view === "multisig" && (
              <>
                <ScreenHead title="Multisig" onBack={back} />
                <MultisigCard net={net} info={info} acct={acct} onSubmit={submit} why={why} />
              </>
            )}
            {view === "smart" && (
              <>
                <ScreenHead title="Smart account" onBack={back} />
                <SmartAccountCard net={net} acct={acct} onSubmit={submit} why={why} />
              </>
            )}
            {view === "crossvm" && (
              <>
                <ScreenHead title="Cross-VM calls" onBack={back} />
                <CrossVmCard net={net} acct={acct} index={index} lastTx={lastTx} onSubmit={submit} why={why} />
              </>
            )}
            {view === "settings" && (
              <>
                <ScreenHead title="Device" />
                <PasswordCard />
                <DeviceCard security={security} busy={!!wait} act={deviceAction} />
                <p className="foot">Qorebit wallet · hybrid signatures: secp256k1 + ML-DSA-87 · keys never leave the device · a research prototype, unaudited</p>
              </>
            )}
          </div>
        )}
      </main>
      {ready && <TabBar view={view} onView={go} />}
      {wait && <WaitOverlay wait={wait} />}
    </div>
  );
}

/** The bridge runs but refuses this extension: Firefox gives every install its own address, which the bridge has to be told. */
function OriginBlocked() {
  const origin = location.origin;
  return (
    <section className="card center">
      <h2>The bridge does not know this wallet yet</h2>
      <p className="muted">For safety the bridge only talks to the extensions it was told about. Start it with this wallet's address:</p>
      <pre className="code">{`QOREBIT_EXTENSION_ORIGINS=${origin} make board-panel`}</pre>
      <p className="muted small">(or with <span className="mono">make web-emu</span> and the same prefix). Firefox gives each install its own address, so this is needed once per install.</p>
      <Copy text={`QOREBIT_EXTENSION_ORIGINS=${origin}`} label="Copy" />
    </section>
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
    <div className="legend" aria-label="The device's two buttons">
      <div><b>LEFT</b><span>= 0 · reject</span></div>
      <div><b>RIGHT</b><span>= 1 · approve</span></div>
    </div>
  );
}

function PinSetup({ onSet }: { onSet: () => void }) {
  return (
    <section className="card accent">
      <h2>Set your signing PIN</h2>
      <p>The device asks for it after every approval. It is eight presses of the two buttons: left is 0, right is 1. Five wrong tries lock signing until you enter your 24 recovery words on the device.</p>
      <ButtonLegend />
      <button className="primary" onClick={onSet}>Set the PIN on the device</button>
    </section>
  );
}

function Copy({ text, label = "Copy", chip }: { text: string; label?: string; chip?: boolean }) {
  const [done, setDone] = useState(false);
  return (
    <button className={chip ? "chip-btn" : "ghost"} onClick={() => navigator.clipboard?.writeText(text).then(() => { setDone(true); setTimeout(() => setDone(false), 1200); })}>
      {done ? "Copied" : label}
    </button>
  );
}

function Hero(p: { info: NetworkInfo; acct: DeviceAccount | null; balance: bigint | null; evmHex: string | null }) {
  return (
    <section className="hero">
      <div className="label">Balance · {p.info.name}</div>
      <div className="balance mono">
        {p.balance === null ? "—" : formatQor(p.balance, 4)} <span className="unit">QOR</span>
      </div>
      <div className="addr-line">
        <span className="mono" aria-label={p.acct?.bech32}>{p.acct ? short(p.acct.bech32, 12, 8) : "…"}</span>
        {p.acct && <Copy text={p.acct.bech32} label="Copy" chip />}
        {p.evmHex && <Copy text={p.evmHex} label="Copy 0x" chip />}
      </div>
    </section>
  );
}

/** What this account still needs on this network: funds, then its post-quantum key. */
function AccountStatus(p: {
  info: NetworkInfo; chainAcct: ChainAccount | null | undefined; pqRegistered: boolean | null; chainError: string | null;
  onRegister: () => void; onFaucet: () => void; busy: boolean;
}) {
  return (
    <>
      {p.chainError && <p className="notice bad">{p.chainError}</p>}
      {p.chainAcct === null && (
        <div className="hint">
          <p>This address has not received anything on {p.info.name} yet, so the chain does not know it.</p>
          {p.info.faucet === "bridge" && <button className="primary" disabled={p.busy} onClick={p.onFaucet}>Get 1,000 test QOR</button>}
          {p.info.faucet && p.info.faucet !== "bridge" && <a className="btn" href={p.info.faucet} target="_blank" rel="noopener noreferrer">Open the {p.info.name} faucet</a>}
          {!p.info.faucet && <p className="muted small">Send QOR to your address from another wallet (Receive shows it).</p>}
        </div>
      )}
      {p.chainAcct && p.pqRegistered === false && (
        <div className="hint">
          <p>QoreChain refuses a transfer until the account's post-quantum key is registered. The device proves the key and signs the registration (classical-only, once). It costs a small fee.</p>
          <button className="primary" disabled={p.busy} onClick={p.onRegister}>Register the post-quantum key</button>
        </div>
      )}
      {p.chainAcct && p.pqRegistered && p.info.faucet === "bridge" && (
        <div className="row" style={{ marginBottom: 10 }}><button className="ghost" disabled={p.busy} onClick={p.onFaucet}>Get 1,000 test QOR</button></div>
      )}
    </>
  );
}

function ReceiveCard(p: { info: NetworkInfo; acct: DeviceAccount | null; evmHex: string | null }) {
  const groups = p.acct ? p.acct.bech32.match(/.{1,4}/g)! : [];
  return (
    <section className="card receive">
      <div className="label">Your address on {p.info.name} (check it on the device)</div>
      <div className="address mono" aria-label={p.acct?.bech32}>
        {groups.map((g, i) => <span key={i} className={i % 2 ? "alt" : ""}>{g}</span>)}
      </div>
      <div className="row" style={{ justifyContent: "center" }}>
        {p.acct && <Copy text={p.acct.bech32} label="Copy address" />}
      </div>
      {p.evmHex && (
        <>
          <div className="label" style={{ marginTop: 16 }}>The same account as an EVM address</div>
          <div className="mono small break">{p.evmHex}</div>
          <div className="row" style={{ justifyContent: "center", marginTop: 8 }}><Copy text={p.evmHex} label="Copy 0x address" /></div>
        </>
      )}
      <p className="muted small" style={{ marginTop: 14 }}>Only send QOR and QoreChain tokens to this address, on the network shown above.</p>
    </section>
  );
}

function SendCard(p: { disabled: boolean; why: string; balance: bigint | null; info: NetworkInfo; onSend: (to: string, amount: bigint, memo: string) => Promise<boolean | undefined> }) {
  const [to, setTo] = useState("");
  const [amount, setAmount] = useState("");
  const [memo, setMemo] = useState("");
  const parsed = useMemo(() => parseQor(amount), [amount]);
  // a pasted address may have spaces around it or be in capitals (valid bech32, only the case differs)
  const addr = to.trim().replace(/\s+/g, "");
  const norm = addr === addr.toUpperCase() ? addr.toLowerCase() : addr;
  const toOk = isQorAddress(norm);
  const problem = !to ? "" : !toOk ? "Not a valid qor1… address (check every character; no spaces)." : "";
  const amountProblem = !amount ? "" : parsed === null || parsed === 0n ? "An amount in QOR, up to 6 decimals." : p.balance !== null && parsed > p.balance ? "More than the balance." : "";
  const can = !p.disabled && toOk && parsed !== null && parsed > 0n && !amountProblem;
  // why the button is off, so it never just looks dead
  const needs = p.why || (!to ? "Enter the recipient's qor1… address." : !toOk ? "The address is not valid." : !amount ? "Enter an amount." : amountProblem || "");
  return (
    <section className="card">
      <h2>Send</h2>
      <form onSubmit={async (e) => { e.preventDefault(); if (can && (await p.onSend(norm, parsed!, memo))) { setTo(""); setAmount(""); setMemo(""); } }}>
        <label>To<input className="mono" value={to} onChange={(e) => setTo(e.target.value)} placeholder="qor1…" spellCheck={false} autoComplete="off" /></label>
        {problem && <div className="field-error">{problem}</div>}
        <label>Amount (QOR)<input className="mono" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.0" inputMode="decimal" autoComplete="off" /></label>
        {amountProblem && <div className="field-error">{amountProblem}</div>}
        <label>Memo (optional)<input value={memo} maxLength={120} onChange={(e) => setMemo(e.target.value)} autoComplete="off" /></label>
        <div className="row">
          <button className="primary" type="submit" disabled={!can}>Review on the device</button>
          <span className={`small ${can ? "muted" : "warn-text"}`}>{can ? (p.info.real ? "Mainnet: real funds." : "Network fee about 0.02 QOR.") : needs}</span>
        </div>
      </form>
      <ButtonLegend />
    </section>
  );
}

function ActivityCard({ activity, bare }: { activity: Activity[] | null; bare?: boolean }) {
  return (
    <section className="card">
      {!bare && <h2>Activity</h2>}
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
        <button className="ghost" disabled={p.busy} onClick={() => p.act(s.pinSet ? "Change the PIN" : "Set a PIN", "On the device: left = 0, right = 1.", device.setPin, "PIN set.")}>{s.pinSet ? "Change PIN" : "Set PIN"}</button>
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
