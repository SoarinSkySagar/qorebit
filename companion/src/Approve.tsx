import { useEffect, useState } from "react";
import { fromHex, parseSiwqMessage, verifySiwq, type SiwqFields, type SiwqSignature } from "@qorechain/qoresign";
import { bridgeUp, device, DeviceError, loadNetworks, ORIGIN_BLOCKED } from "./bridge";
import { broadcast, gasFor, getAccount, getBalance, nodeChainId, pqStatus, signBytes, waitForTx } from "./chain";
import { DEFAULT_NETWORKS, type NetworkId } from "./networks";
import { describeSiteTx, type Described } from "./siteTx";
import { buildTx, DEFAULT_GAS, fee, formatQor, type KeyType } from "./tx";
import { toHex } from "@qorechain/qoresign";
import { ext } from "./ext";

/** What a website asked for (the extension's background keeps it until this page answers). */
interface Pending {
  origin: string;
  method: "qor_requestAccounts" | "qor_signIn" | "qor_sendTransaction";
  params: any[];
}
interface State {
  address: string;
  index: number;
  accountName: string;
  net: string;
  chainId: string;
}
const MAX_STATEMENT = 75;
const MAX_DOMAIN = 44;
const b64 = (bytes: Uint8Array) => btoa(Array.from(bytes, (b) => String.fromCharCode(b)).join(""));
const chrome_ = ext;
const send = (m: Record<string, unknown>) => chrome_.runtime.sendMessage(m);

/** The approval window a site's request opens (`index.html#/approve/<id>`): connect, or sign in with the device. */
export function Approve({ id }: { id: string }) {
  const [req, setReq] = useState<Pending | null | undefined>(undefined);
  const [state, setState] = useState<State | null>(null);
  const [ready, setReady] = useState<string | null>("Checking the device…");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    void send({ type: "qorebit:get", id }).then((r: Pending | null) => setReq(r ?? null));
    void chrome_.storage.local.get("qorebit.state").then((r: any) => setState(r["qorebit.state"] ?? null));
  }, [id]);
  useEffect(() => {
    let live = true;
    (async () => {
      if (!(await bridgeUp())) return live && setReady("The bridge is not running: start it (make web or make board-panel).");
      try {
        const sec = await device.security();
        if (live) setReady(sec.pinSet ? null : "Set the signing PIN in the wallet first.");
      } catch (e) {
        if (live) setReady(e instanceof DeviceError && e.code === 8 ? null : e instanceof DeviceError && e.code === ORIGIN_BLOCKED ? `The bridge does not know this wallet yet: start it with QOREBIT_EXTENSION_ORIGINS=${location.origin}` : e instanceof Error ? e.message : String(e));
      }
    })();
    return () => void (live = false);
  }, []);

  const host = req ? new URL(req.origin).host : "";
  const reject = (message = "the user rejected the request") => void send({ type: "qorebit:fail", id, code: 4001, message });

  let fields: SiwqFields | null = null;
  const problems: string[] = [];
  if (req?.method === "qor_signIn") {
    try {
      fields = parseSiwqMessage(req.params[0]?.message ?? "");
    } catch (e) {
      problems.push(`This is not a valid sign-in message: ${e instanceof Error ? e.message : String(e)}`);
    }
    if (state && state.index >= 8) problems.push("Sign-in is for native accounts: switch to a native account in the wallet first.");
    if (fields) {
      if (fields.domain !== host) problems.push(`The message is for ${fields.domain}, but it was asked for by ${host}. A site can only ask for its own sign-in.`);
      if (state && fields.address !== state.address) problems.push(`The message is for ${fields.address}, not your selected account (${state.accountName}). Switch account in the wallet, then try again.`);
      if (fields.resources?.length) problems.push("It lists resources: the device cannot show those, so it refuses.");
      if (fields.statement && fields.statement.length > MAX_STATEMENT) problems.push(`The statement is over ${MAX_STATEMENT} characters: the device cannot show it in full.`);
      if (fields.domain.length > MAX_DOMAIN) problems.push("The site name is too long for the device to show in full.");
    }
  }

  async function connectNow() {
    if (!state) return;
    await send({ type: "qorebit:done", id, result: { address: state.address } });
  }

  async function signNow() {
    if (!state || !fields || !req) return;
    setBusy(true);
    setError("");
    try {
      const message = req.params[0].message as string;
      const r = await device.signSiwq(state.index, message);
      const acct = await device.account(state.index);
      const signature: SiwqSignature = {
        signatureType: "ml-dsa-87-hybrid",
        classical: { keyType: "secp256k1", publicKey: b64(fromHex(acct.publicKey)), signature: b64(fromHex(r.classicalSignature)) },
        pq: { algorithmId: 1, signature: b64(fromHex(r.pqSignature)) },
      };
      // what the site will check, checked here first
      const pk = fromHex((await device.pqPublicKey(state.index)).publicKey);
      verifySiwq({ message, signature, domain: fields.domain, nonce: fields.nonce, chainId: fields.chainId, pqPublicKey: pk });
      await send({ type: "qorebit:done", id, result: signature });
    } catch (e) {
      if (e instanceof DeviceError && e.rejected) return void reject("rejected on the device");
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  if (req?.method === "qor_sendTransaction" && state) return <SendTx id={id} req={req} state={state} ready={ready} />;
  if (req === undefined) return <div className="approve"><p className="muted">Loading…</p></div>;
  if (req === null) return <div className="approve"><h1>Nothing to approve</h1><p className="muted">This request has ended. You can close this window.</p></div>;

  return (
    <div className="approve">
      <div className="brand" style={{ display: "inline-flex" }}><span className="mark" aria-hidden />QOREBIT</div>
      <h1 style={{ fontSize: 20, margin: "14px 0 0" }}>{req.method === "qor_requestAccounts" ? "Connect to this site?" : "Sign in with QoreChain"}</h1>
      <div className="site">
        <span className="globe">{host.slice(0, 1).toUpperCase()}</span>
        <span>
          <b className="mono">{host}</b>
          <span className="muted small block">{req.origin.startsWith("http://") ? "Not encrypted (http)" : "wants to use your wallet"}</span>
        </span>
      </div>
      {!state && <div className="notice bad">Open the Qorebit wallet once (the toolbar button) so it knows your account, then try again.</div>}
      {state && (
        <dl className="siwq">
          <dt>Account</dt><dd>{state.accountName}</dd>
          <dt>Address</dt><dd className="mono small break">{state.address}</dd>
          <dt>Network</dt><dd className="mono">{state.chainId}</dd>
          {fields?.statement && (<><dt>Statement</dt><dd>{fields.statement}</dd></>)}
          {fields && (<><dt>Expires</dt><dd className="mono">{fields.expirationTime ?? "10 minutes after it was issued"}</dd></>)}
        </dl>
      )}
      {req.method === "qor_requestAccounts" && <p className="muted small">It will see your address and the network. It cannot move funds or sign anything without you approving each request on the device.</p>}
      {req.method === "qor_signIn" && <p className="muted small">The device will show the site and the statement. Compare them with the ones above, then press the right button.</p>}
      {problems.map((p) => <div key={p} className="notice bad">{p}</div>)}
      {ready && <div className="notice warn">{ready}</div>}
      {error && <div className="notice bad">{error}</div>}
      {busy && <div className="notice warn">Check the device: press the right button to approve (left rejects), then your PIN.</div>}
      <div className="buttons">
        <button className="ghost" onClick={() => reject()}>Reject</button>
        {req.method === "qor_requestAccounts"
          ? <button className="primary" disabled={!state} onClick={() => void connectNow()}>Connect</button>
          : <button className="primary" disabled={!state || !fields || problems.length > 0 || !!ready || busy} onClick={() => void signNow()}>{busy ? "Waiting for the device…" : "Approve on the device"}</button>}
      </div>
    </div>
  );
}


/** `qor_sendTransaction`: show what the site wants to send, the fee and the checks; on approval the device signs it and the wallet broadcasts it. */
function SendTx({ id, req, state, ready }: { id: string; req: Pending; state: State; ready: string | null }) {
  const host = new URL(req.origin).host;
  const [prep, setPrep] = useState<{ fee: bigint; balance: bigint; problems: string[]; estimated: boolean } | null>(null);
  const [phase, setPhase] = useState<"idle" | "signing" | "broadcasting">("idle");
  const [error, setError] = useState("");

  let described: Described | null = null;
  let parseError = "";
  try {
    described = describeSiteTx(req.params, state.address);
  } catch (e) {
    parseError = e instanceof Error ? e.message : String(e);
  }
  const fail = (code: number, message: string) => void send({ type: "qorebit:fail", id, code, message });
  useEffect(() => {
    if (parseError) fail(-32602, parseError);
  }, [parseError]); // eslint-disable-line react-hooks/exhaustive-deps

  /** What the chain says about this account now: the node's chain id, the account, its post-quantum key, the sequence and the fee. */
  async function check(d: Described) {
    const nets = await loadNetworks();
    const net = (state.net in nets ? state.net : "testnet") as NetworkId;
    const info = nets[net] ?? DEFAULT_NETWORKS[net];
    const problems: string[] = [];
    const reported = await nodeChainId(net);
    if (reported !== info.chainId) throw new Error(`the ${info.name} node reports chain "${reported}", expected "${info.chainId}": nothing was signed`);
    const acct = await device.account(state.index);
    if (acct.bech32 !== state.address) throw new Error("the device's account does not match the wallet's selected account: open the wallet and select it again");
    const chainAcct = await getAccount(net, state.address);
    if (!chainAcct) throw new Error(`this account has no funds on ${info.name} yet`);
    if (!(await pqStatus(net, state.address)).registered) problems.push("This account's post-quantum key is not registered. Open the wallet and register it first.");
    const keyType: KeyType = state.index >= 8 ? "eth_secp256k1" : "secp256k1";
    const pub = fromHex(acct.publicKey);
    const probe = buildTx({ messages: [d.msg], memo: d.memo, publicKey: pub, sequence: chainAcct.sequence, gas: DEFAULT_GAS, keyType });
    const g = await gasFor(net, probe.body, probe.authInfo);
    let gas = g.gas;
    if (d.msg.typeUrl.startsWith("/qorechain.crossvm.")) gas = gas * 2n > 1_500_000n ? gas * 2n : 1_500_000n;
    const balance = await getBalance(net, state.address);
    if (balance < d.uqorSpend + fee(gas)) problems.push(`Not enough QOR: ${formatQor(d.uqorSpend)} plus the fee of ${formatQor(fee(gas))}, and the balance is ${formatQor(balance)}.`);
    return { net, info, acct, chainAcct, keyType, pub, gas, fee: fee(gas), balance, problems, estimated: g.simulated };
  }

  useEffect(() => {
    if (!described) return;
    let live = true;
    check(described).then(
      (c) => live && setPrep({ fee: c.fee, balance: c.balance, problems: c.problems, estimated: c.estimated }),
      (e) => live && setPrep({ fee: 0n, balance: 0n, problems: [e instanceof Error ? e.message : String(e)], estimated: false }),
    );
    return () => void (live = false);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  async function approve() {
    if (!described) return;
    setError("");
    try {
      setPhase("signing");
      const c = await check(described); // fresh: the sequence and the fee as they are now
      if (c.problems.length) throw new Error(c.problems[0]);
      const { body, authInfo } = buildTx({ messages: [described.msg], memo: described.memo, publicKey: c.pub, sequence: c.chainAcct.sequence, gas: c.gas, keyType: c.keyType });
      const signed = await device.signTx({
        account: state.index, accountNumber: c.chainAcct.accountNumber, chainId: c.info.chainId, body: toHex(body), authInfo: toHex(authInfo),
        signBytes: await signBytes(c.net, c.info.chainId),
      });
      setPhase("broadcasting");
      const r = await broadcast(c.net, fromHex(signed.txBytes));
      if (r.code !== 0) throw new Error(`the chain refused it (code ${r.code}): ${r.log}`);
      const done = await waitForTx(c.net, r.txhash);
      if (done.code !== 0) return fail(-32000, `the transaction failed on chain: ${done.rawLog}`);
      await send({ type: "qorebit:done", id, result: { hash: r.txhash, height: done.height } });
    } catch (e) {
      if (e instanceof DeviceError && e.rejected) return fail(4001, "rejected on the device");
      setError(e instanceof DeviceError && e.refused ? `The device refused to sign: ${e.message}` : e instanceof Error ? e.message : String(e));
      setPhase("idle");
    }
  }

  const problems = prep?.problems ?? [];
  return (
    <div className="approve">
      <div className="brand" style={{ display: "inline-flex" }}><span className="mark" aria-hidden />QOREBIT</div>
      <h1 style={{ fontSize: 20, margin: "14px 0 0" }}>{described ? `${described.title}?` : "Transaction request"}</h1>
      <div className="site">
        <span className="globe">{host.slice(0, 1).toUpperCase()}</span>
        <span>
          <b className="mono">{host}</b>
          <span className="muted small block">{req.origin.startsWith("http://") ? "Not encrypted (http) · asks you to sign a transaction" : "asks you to sign a transaction"}</span>
        </span>
      </div>
      {parseError && <div className="notice bad">This request is not valid: {parseError}</div>}
      {described && (
        <dl className="siwq">
          <dt>From</dt><dd>{state.accountName}</dd>
          {described.rows.map(([k, v]) => (<><dt key={k}>{k}</dt><dd className="mono small break">{v}</dd></>))}
          {described.memo && (<><dt>Memo</dt><dd>{described.memo}</dd></>)}
          <dt>Network</dt><dd className="mono">{state.chainId}</dd>
          <dt>Fee</dt><dd className="mono">{prep ? `${prep.estimated ? "" : "about "}${formatQor(prep.fee)} QOR` : "…"}</dd>
        </dl>
      )}
      {described?.warnings.map((w) => <div key={w} className="notice warn">{w}</div>)}
      {state.chainId === "qorechain-vladi" && <div className="notice warn"><b>Mainnet.</b> Real funds.</div>}
      {problems.map((p) => <div key={p} className="notice bad">{p}</div>)}
      {ready && <div className="notice warn">{ready}</div>}
      {error && <div className="notice bad">{error}</div>}
      {phase === "signing" && <div className="notice warn">Check the device: it shows this transaction. Right button approves (left rejects), then your PIN.</div>}
      {phase === "broadcasting" && <div className="notice warn">Sending it to the network…</div>}
      {!parseError && <p className="muted small">The device reads the transaction itself and shows it. Compare it with the details above before you approve.</p>}
      <div className="buttons">
        <button className="ghost" onClick={() => fail(4001, "the user rejected the request")} disabled={phase === "broadcasting"}>Reject</button>
        <button className="primary" disabled={!described || !prep || problems.length > 0 || !!ready || phase !== "idle"} onClick={() => void approve()}>
          {phase === "signing" ? "Waiting for the device…" : phase === "broadcasting" ? "Sending…" : !prep && described ? "Checking…" : "Approve on the device"}
        </button>
      </div>
    </div>
  );
}
