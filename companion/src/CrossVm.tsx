import { useEffect, useState } from "react";
import type { DeviceAccount } from "./bridge";
import { chainJson } from "./chain";
import type { NetworkId } from "./networks";
import { crossVmCallMsg, hexToBytes, isHex, qorToEvmHex, type TargetVm } from "./crossVmMsg";
import { isQorAddress, parseQor, type AnyMsg } from "./tx";

const DEVICE_HINT = "The device shows this as a CROSS-VM transaction. It does not show the details: they are on this page, so check them here first. Right button approves, left button rejects.";
const isEvm20 = (s: string) => /^0x[0-9a-fA-F]{40}$/.test(s.trim());
const normQor = (s: string) => {
  const t = s.trim().replace(/\s+/g, "");
  return t === t.toUpperCase() ? t.toLowerCase() : t;
};
const short = (s: string, a = 10, b = 6) => (s.length > a + b + 2 ? `${s.slice(0, a)}…${s.slice(-b)}` : s);

interface Props {
  net: NetworkId;
  acct: DeviceAccount | null;
  /** The selected account's index: 0..8 native, 8..16 EVM lane. */
  index: number;
  why: string;
  onSubmit: (what: string, msgs: AnyMsg[], hint?: string) => Promise<boolean>;
  /** The last transaction this page sent (hash), so its cross-VM events can be shown. */
  lastTx: { hash: string; at: number } | null;
}

interface Result {
  hash: string;
  events: { request?: Record<string, string>; response?: Record<string, string> };
}

export function CrossVmCard({ net, acct, index, why, onSubmit, lastTx }: Props) {
  const me = acct?.bech32 ?? "";
  const evmLane = index >= 8;
  const [vm, setVm] = useState<TargetVm>("cosmwasm");
  const [contract, setContract] = useState("");
  const [payload, setPayload] = useState("");
  const [funds, setFunds] = useState("");
  const [busy, setBusy] = useState(false);
  const [mine, setMine] = useState<number | null>(null);
  const [result, setResult] = useState<Result | null>(null);

  // the events of the transaction this card sent
  useEffect(() => {
    if (!lastTx || mine === null || lastTx.at < mine) return;
    let live = true;
    void chainJson(net, `/cosmos/tx/v1beta1/txs/${lastTx.hash}`).then((r) => {
      if (!live || r.status !== 200) return;
      const attrs = (type: string) => {
        const ev = (r.json.tx_response?.events ?? []).find((e: any) => e.type === type);
        return ev ? Object.fromEntries((ev.attributes ?? []).map((a: any) => [a.key, String(a.value)])) : undefined;
      };
      setResult({ hash: lastTx.hash, events: { request: attrs("crossvm_request"), response: attrs("crossvm_response") } });
    });
    return () => void (live = false);
  }, [net, lastTx, mine]);

  const c = contract.trim();
  const contractOk = vm === "evm" ? isEvm20(c) : isQorAddress(normQor(c));
  let bytes: Uint8Array | null = null;
  let payloadErr = "";
  if (payload.trim()) {
    if (vm === "evm") {
      if (isHex(payload) && payload.trim().replace(/^0x/, "").length >= 8) bytes = hexToBytes(payload);
      else payloadErr = "The calldata is hex (at least a 4-byte selector).";
    } else {
      try {
        JSON.parse(payload);
        bytes = new TextEncoder().encode(payload.trim());
      } catch {
        payloadErr = "The payload is not valid JSON.";
      }
    }
  }
  const uqor = funds.trim() ? parseQor(funds) : 0n;
  const need = !c ? "Enter the contract." : !contractOk ? (vm === "evm" ? "An EVM contract is 0x followed by 40 hex digits." : "A native contract is a qor1… address.") : !payload.trim() ? "Enter the payload." : payloadErr || (uqor === null ? "Funds are an amount of QOR." : vm === "evm" && uqor > 0n ? "Funds can only go to a native (CosmWasm) contract." : "");
  const can = !why && !busy && !need && !!bytes && !!me;

  function example(kind: "cw20" | "erc20") {
    setResult(null);
    if (kind === "cw20") {
      setVm("cosmwasm");
      if (net === "testnet") setContract("qor14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9s93uuqt");
      setPayload(JSON.stringify({ increase_allowance: { spender: net === "testnet" ? "qor14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9s93uuqt" : "", amount: "1" } }));
    } else {
      setVm("evm");
      if (net === "testnet") setContract("0x97143B771D34ddb9130b807b135Ae675A5a963CA");
      // approve(spender = this account's 0x form, 1)
      let spender = "0".repeat(40);
      try {
        spender = qorToEvmHex(me).slice(2);
      } catch {
        /* a 32-byte account has none */
      }
      setPayload("0x095ea7b3" + spender.padStart(64, "0") + "1".padStart(64, "0"));
    }
  }

  async function go(e: React.FormEvent) {
    e.preventDefault();
    if (!can || !bytes) return;
    const msg = crossVmCallMsg({ sender: me, targetVm: vm, targetContract: vm === "evm" ? c : normQor(c), payload: bytes, funds: uqor && uqor > 0n ? [{ denom: "uqor", amount: uqor.toString() }] : [] });
    setBusy(true);
    setResult(null);
    setMine(Date.now());
    try {
      await onSubmit(`Cross-VM call to ${vm === "evm" ? "an EVM" : "a native"} contract`, [msg], DEVICE_HINT);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card">
      <h2>Cross-VM calls</h2>
      <p className="muted small">
        QoreChain runs several virtual machines on one chain. One transaction from your account can call a contract on another one. From an
        <b> EVM account</b> (pick one in the account menu at the top) this is <b>EVM to Native</b>: the call starts on the EVM side and runs a native CosmWasm contract.
        From a native account it goes the other way. The device signs it like any transaction and shows CROSS-VM.
      </p>
      <p className="small">
        <span className="pill">{evmLane ? "EVM account: EVM → target" : "Native account: native → target"}</span>
        {evmLane && acct && <span className="muted small mono"> {qorToEvmHex(acct.bech32)}</span>}
      </p>
      <form onSubmit={go}>
        <label>
          Target
          <select value={vm} onChange={(e) => { setVm(e.target.value as TargetVm); setResult(null); }}>
            <option value="cosmwasm">A native contract (CosmWasm)</option>
            <option value="evm">An EVM contract (Solidity)</option>
          </select>
        </label>
        <label>Contract<input className="mono" value={contract} onChange={(e) => setContract(e.target.value)} placeholder={vm === "evm" ? "0x…" : "qor1…"} spellCheck={false} autoComplete="off" /></label>
        <label>{vm === "evm" ? "Calldata (hex)" : "Execute message (JSON)"}<textarea className="mono" rows={3} value={payload} onChange={(e) => setPayload(e.target.value)} placeholder={vm === "evm" ? "0xa9059cbb…" : '{"ping":{}}'} spellCheck={false} /></label>
        {vm === "cosmwasm" && <label>Funds to attach (QOR, optional)<input className="mono" value={funds} onChange={(e) => setFunds(e.target.value)} placeholder="0" inputMode="decimal" autoComplete="off" /></label>}
        {can && bytes && (
          <dl className="siwq">
            <dt>From</dt><dd>{evmLane ? "your EVM account (EVM lane)" : "your native account"} <span className="mono small">{short(me, 12, 6)}</span></dd>
            <dt>Calls</dt><dd>{vm === "evm" ? "an EVM contract" : "a native contract"} <span className="mono small break">{vm === "evm" ? c : normQor(c)}</span></dd>
            <dt>Payload</dt><dd className="mono small break">{vm === "evm" ? payload.trim() : payload.trim()}</dd>
            {uqor && uqor > 0n ? (<><dt>Funds</dt><dd className="mono">{funds} QOR</dd></>) : null}
          </dl>
        )}
        <div className="row">
          <button className="primary" type="submit" disabled={!can}>Call with the device</button>
          <button className="ghost" type="button" onClick={() => example("cw20")} title="CW20: let your own account spend 1 unit (harmless)">Example: native call</button>
          <button className="ghost" type="button" onClick={() => example("erc20")} title="ERC-20 approve: let your own account spend 1 unit (harmless)">Example: EVM call</button>
          <span className={`small ${can ? "muted" : "warn-text"}`}>{can ? "The call runs in this transaction; if it fails, nothing changes." : why || need}</span>
        </div>
      </form>
      {result && (
        <div className="siwq-result">
          <div className={`notice ${result.events.response?.status === "executed" ? "good" : "bad"}`}>
            {result.events.request ? (
              <>Cross-VM: <b>{result.events.request.source_vm}</b> → <b>{result.events.request.target_vm}</b>, {result.events.response?.status ?? "no response event"}.</>
            ) : "The transaction landed, but no cross-VM event was found in it."}
          </div>
          <div className="muted small mono break">{result.hash}</div>
        </div>
      )}
    </section>
  );
}
