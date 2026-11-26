import { useEffect, useState } from "react";
import { fromHex, parseSiwqMessage, verifySiwq, type SiwqFields, type SiwqSignature } from "@qorechain/qoresign";
import { bridgeUp, device, DeviceError } from "./bridge";
import { ext } from "./ext";

/** What a website asked for (the extension's background keeps it until this page answers). */
interface Pending {
  origin: string;
  method: "qor_requestAccounts" | "qor_signIn";
  params: { message?: string }[];
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
        if (live) setReady(e instanceof DeviceError && e.code === 8 ? null : e instanceof Error ? e.message : String(e));
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
      const message = req.params[0].message!;
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
