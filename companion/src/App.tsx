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

