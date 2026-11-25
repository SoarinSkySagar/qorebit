import { useMemo, useState } from "react";
import { createSiwqMessage, parseSiwqMessage, verifySiwq, fromHex, type SiwqFields, type SiwqSignature } from "@qorechain/qoresign";
import { device, type DeviceAccount } from "./bridge";

const b64 = (bytes: Uint8Array) => btoa(Array.from(bytes, (b) => String.fromCharCode(b)).join(""));
const nonce = () => b64(crypto.getRandomValues(new Uint8Array(12))).replace(/[^A-Za-z0-9]/g, "").slice(0, 12).padEnd(12, "a");
// the device shows the statement and the site in full, so it takes short ones
const MAX_STATEMENT = 75;
const MAX_DOMAIN = 44;

interface Props {
  acct: DeviceAccount | null;
  index: number;
  chainId: string;
  /** Why nothing can be signed now ("" when it can). */
  why: string;
  /** Run the device's sign-in (it shows an approval screen); null when it was rejected or failed. */
  onSign: (message: string) => Promise<{ classicalSignature: string; pqSignature: string } | null>;
}

export function SignInCard({ acct, index, chainId, why, onSign }: Props) {
  const [text, setText] = useState("");
  const [result, setResult] = useState<{ json: string; verified: string | null; problem: string | null } | null>(null);
  const [busy, setBusy] = useState(false);

  const parsed = useMemo((): { fields: SiwqFields | null; error: string | null } => {
    if (!text.trim()) return { fields: null, error: null };
    try {
      return { fields: parseSiwqMessage(text), error: null };
    } catch (e) {
      return { fields: null, error: e instanceof Error ? e.message : String(e) };
    }
  }, [text]);
  const f = parsed.fields;
  const problems: string[] = [];
  if (f && acct && f.address !== acct.bech32) problems.push(`This sign-in is for ${f.address}, not the selected account (Account ${index + 1}). Pick the right account first.`);
  if (f?.resources?.length) problems.push("It lists resources: the device refuses those, because its screen cannot show them.");
  if (f?.statement && f.statement.length > MAX_STATEMENT) problems.push(`The statement is over ${MAX_STATEMENT} characters: the device cannot show it in full. Ask the site for a shorter one.`);
  if (f && f.domain.length > MAX_DOMAIN) problems.push("The site name is too long for the device to show in full.");
  const can = !!f && !why && !busy && problems.length === 0;

  function makeTest() {
    if (!acct) return;
    const now = new Date();
    setResult(null);
    setText(
      createSiwqMessage({
        domain: "qorebit.local",
        address: acct.bech32,
        statement: "Test sign-in to Qorebit",
        uri: "https://qorebit.local/login",
        chainId,
        nonce: nonce(),
        issuedAt: now.toISOString(),
        expirationTime: new Date(now.getTime() + 10 * 60_000).toISOString(),
      }),
    );
  }

  async function sign() {
    if (!f || !acct) return;
    setBusy(true);
    setResult(null);
    try {
      const r = await onSign(text);
      if (!r) return;
      const signature: SiwqSignature = {
        signatureType: "ml-dsa-87-hybrid",
        classical: { keyType: "secp256k1", publicKey: b64(fromHex(acct.publicKey)), signature: b64(fromHex(r.classicalSignature)) },
        pq: { algorithmId: 1, signature: b64(fromHex(r.pqSignature)) },
      };
      // check what came back, as the website will (both layers, the domain, the nonce, the chain, the time)
      let verified: string | null = null;
      let problem: string | null = null;
      try {
        const pk = fromHex((await device.pqPublicKey(index)).publicKey);
        verifySiwq({ message: text, signature, domain: f.domain, nonce: f.nonce, chainId: f.chainId, pqPublicKey: pk });
        verified = "Verified here: the secp256k1 and the ML-DSA-87 signatures both check out for this message.";
      } catch (e) {
        problem = `The signature came back but did not verify here: ${e instanceof Error ? e.message : String(e)}`;
      }
      setResult({ json: JSON.stringify(signature, null, 2), verified, problem });
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card">
      <h2>Sign in with QoreChain</h2>
      <p className="muted small">
        Log in to a website with your account instead of a password. The site gives you a message; the device shows the site, the network
        and the statement, and signs it with both of the account's keys. Nothing is sent from here: you paste the signature back into the site.
      </p>
      <label>
        The site's sign-in message
        <textarea className="mono" rows={7} value={text} onChange={(e) => { setText(e.target.value); setResult(null); }} placeholder={"app.example.com wants you to sign in with your QoreChain account:\nqor1…"} spellCheck={false} />
      </label>
      {parsed.error && <div className="field-error">Not a valid sign-in message: {parsed.error}</div>}
      {f && (
        <dl className="siwq">
          <dt>Site</dt><dd className="mono">{f.scheme ? `${f.scheme}://` : ""}{f.domain}</dd>
          <dt>Account</dt><dd className="mono small break">{f.address}</dd>
          <dt>Network</dt><dd className="mono">{f.chainId}</dd>
          <dt>Expires</dt><dd className="mono">{f.expirationTime ?? "10 minutes after it was issued"}</dd>
          {f.statement && (<><dt>Statement</dt><dd>{f.statement}</dd></>)}
        </dl>
      )}
      {problems.map((x) => <div key={x} className="field-error">{x}</div>)}
      <div className="row">
        <button className="primary" disabled={!can} onClick={() => void sign()}>Sign in with the device</button>
        <button className="ghost" disabled={!acct || busy} onClick={makeTest}>Make a test message</button>
        <span className={`small ${can || !f ? "muted" : "warn-text"}`}>{why || (f ? "Compare the site and the statement on the device screen with the ones above." : "Paste a message, or make a test one.")}</span>
      </div>
      {result && (
        <div className="siwq-result">
          {result.verified && <div className="notice good">{result.verified}</div>}
          {result.problem && <div className="notice bad">{result.problem}</div>}
          <label>
            The signature: paste this into the site
            <textarea className="mono" rows={9} readOnly value={result.json} onFocus={(e) => e.currentTarget.select()} />
          </label>
          <button className="ghost" onClick={() => void navigator.clipboard?.writeText(result.json).catch(() => undefined)}>Copy the signature</button>
        </div>
      )}
    </section>
  );
}
