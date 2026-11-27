import { useEffect, useState } from "react";
import type { ChainStatus } from "./sdk";
import type { NetworkId } from "./networks";

/** A small read-only panel on Home: the chain's latest block, its fee floor and this account's post-quantum key, read with @qorechain/sdk. */
export function ChainStatusCard({ net, address }: { net: NetworkId; address: string | null }) {
  const [s, setS] = useState<ChainStatus | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setS(null);
    setErr(null);
    // the SDK is large: load it only when the panel is shown
    const load = () => import("./sdk").then((m) => m.chainStatus(net, address)).then((v) => live && (setS(v), setErr(null)), (e) => live && setErr(e instanceof Error ? e.message : String(e)));
    load();
    const t = setInterval(() => { if (!document.hidden) load(); }, 15000);
    return () => { live = false; clearInterval(t); };
  }, [net, address]);
  return (
    <section className="card" aria-label="Chain status">
      <div className="recent-head"><h2>Chain status</h2><span className="muted small">read with @qorechain/sdk</span></div>
      {err && <p className="muted">Could not read the chain: {err}</p>}
      {!err && !s && <p className="muted">Reading the chain…</p>}
      {s && (
        <dl className="kv">
          <div><dt>Latest block</dt><dd className="mono">{s.height}{s.chainId ? ` · ${s.chainId}` : ""}</dd></div>
          <div><dt>Fee floor</dt><dd className="mono">{s.minGasPrice ? `${s.minGasPrice} uqor / gas` : "unknown"}</dd></div>
          <div><dt>Post-quantum key</dt><dd>{s.pqRegistered === null ? "unknown" : s.pqRegistered ? "registered" : "not registered yet"}</dd></div>
          {s.explorerAddress && <div><dt>Explorer</dt><dd><a href={s.explorerAddress} target="_blank" rel="noreferrer noopener">this account</a></dd></div>}
        </dl>
      )}
    </section>
  );
}
