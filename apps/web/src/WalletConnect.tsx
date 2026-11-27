import { useCallback, useEffect, useState } from "react";
import { MsgSend } from "cosmjs-types/cosmos/bank/v1beta1/tx";

// Two ways to connect, side by side, and to send one hybrid-signed (secp256k1 + ML-DSA-87) transaction:
//
//  * QoreX      window.qorex.native (Keplr-style: enable, getKey, signAndBroadcast). QoreX shows the decoded message, adds the ML-DSA-87
//               layer only the wallet can make, and broadcasts. Detected by window.qorex, never window.keplr.
//               https://docs.qorechain.io/qorex/dapp-integration
//  * Qorebit    window.qorechain (the Qorebit browser extension, whose keys live on a hardware wallet: the board shows the transaction
//               and makes both signatures). qor_requestAccounts / qor_sendTransaction.

const EXPLORER = "https://explore.qore.network";

interface QorexNative {
  capabilities?: string[];
  enable(chainId: string): Promise<void>;
  getKey(chainId: string): Promise<{ bech32Address: string; name?: string; algo?: string }>;
  signAndBroadcast(req: {
    chainId: string;
    messages: { typeUrl: string; value: Uint8Array }[];
    memo?: string;
    gasLimit?: number;
  }): Promise<{ txHash: string; code: number; height?: number | string; shield?: string }>;
}

interface QorechainProvider {
  request(args: { method: string; params?: unknown[] }): Promise<any>;
}

declare global {
  interface Window {
    qorex?: { native?: QorexNative };
    qorechain?: QorechainProvider;
  }
}

type WalletId = "qorex" | "qorebit";

interface Sent {
  wallet: WalletId;
  hash: string;
  note: string;
}

const short = (s: string, a = 10, b = 6): string => (s.length > a + b + 1 ? `${s.slice(0, a)}…${s.slice(-b)}` : s);

/** QoreX injects window.qorex at page start; the extension for Qorebit injects window.qorechain. Look again a few times in case they are late. */
function useDetected(): { qorex: boolean; qorebit: boolean } {
  const read = () => ({
    qorex: typeof window.qorex?.native?.enable === "function" && typeof window.qorex?.native?.signAndBroadcast === "function",
    qorebit: typeof window.qorechain?.request === "function",
  });
  const [found, setFound] = useState(read);
  useEffect(() => {
    let n = 0;
    const t = window.setInterval(() => {
      setFound(read());
      if (++n >= 10) window.clearInterval(t);
    }, 500);
    return () => window.clearInterval(t);
  }, []);
  return found;
}

export function WalletConnect({ chainId }: { chainId: string }): JSX.Element {
  const found = useDetected();
  const [wallet, setWallet] = useState<WalletId | null>(null);
  const [account, setAccount] = useState<string | null>(null);
  const [to, setTo] = useState("");
  const [amount, setAmount] = useState("0.001");
  const [memo, setMemo] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sent, setSent] = useState<Sent | null>(null);

  const fail = (e: unknown): void => setError(e instanceof Error ? e.message : typeof e === "object" && e && "message" in e ? String((e as { message: unknown }).message) : String(e));

  const connect = useCallback(
    async (w: WalletId): Promise<void> => {
      setError(null);
      setSent(null);
      setBusy(true);
      try {
        if (w === "qorex") {
          const native = window.qorex!.native!;
          await native.enable(chainId); // one approval per site; it only reveals the public address
          const key = await native.getKey(chainId);
          setAccount(key.bech32Address);
        } else {
          const [addr] = await window.qorechain!.request({ method: "qor_requestAccounts" });
          setAccount(addr);
        }
        setWallet(w);
      } catch (e) {
        fail(e);
      } finally {
        setBusy(false);
      }
    },
    [chainId],
  );

  function disconnect(): void {
    setWallet(null);
    setAccount(null);
    setSent(null);
    setError(null);
  }

  async function send(): Promise<void> {
    if (!wallet || !account) return;
    setError(null);
    setSent(null);
    const uqor = Math.round(Number(amount) * 1e6);
    if (!Number.isFinite(uqor) || uqor <= 0) return setError("Enter an amount in QOR, for example 0.001.");
    if (!/^qor1[0-9a-z]{38,}$/.test(to.trim())) return setError("Enter the receiver's qor1… address.");
    setBusy(true);
    try {
      if (wallet === "qorex") {
        const msg = MsgSend.fromPartial({ fromAddress: account, toAddress: to.trim(), amount: [{ denom: "uqor", amount: String(uqor) }] });
        const res = await window.qorex!.native!.signAndBroadcast({
          chainId, // must be the network QoreX is on (switch it in the wallet; a page cannot)
          messages: [{ typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: MsgSend.encode(msg).finish() }],
          memo,
        });
        if (res.code !== 0) throw new Error(`The chain refused the transaction (code ${res.code}).`);
        setSent({ wallet, hash: res.txHash, note: res.shield ? `signature layer: ${res.shield}` : "" });
      } else {
        const res = await window.qorechain!.request({
          method: "qor_sendTransaction",
          params: [{ messages: [{ typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: { toAddress: to.trim(), amount: [{ denom: "uqor", amount: String(uqor) }] } }], memo }],
        });
        const hash = typeof res === "string" ? res : (res?.txhash ?? res?.txHash ?? res?.hash ?? "");
        setSent({ wallet, hash, note: "approved on the Qorebit board" });
      }
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }

  const box = { border: "1px solid #ccc", borderRadius: 8, padding: "1rem", marginTop: "1.5rem" } as const;
  const names: Record<WalletId, string> = { qorex: "QoreX", qorebit: "Qorebit" };

  return (
    <section style={box}>
      <h2 style={{ marginTop: 0 }}>Connect a wallet</h2>
      <p style={{ color: "#555", marginTop: 0 }}>
        Pick QoreX or Qorebit. Either one shows you the transaction and adds both signatures, secp256k1 and ML-DSA-87; this page never holds a key.
      </p>

      {!wallet && (
        <div style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap" }}>
          <button onClick={() => void connect("qorex")} disabled={!found.qorex || busy} title={found.qorex ? "" : "QoreX not found: install the QoreX extension"}>
            Connect QoreX
          </button>
          <button onClick={() => void connect("qorebit")} disabled={!found.qorebit || busy} title={found.qorebit ? "" : "Qorebit extension not found"}>
            Connect Qorebit
          </button>
          <span style={{ color: "#555", alignSelf: "center", fontSize: 14 }}>
            {found.qorex ? "QoreX found" : "QoreX not found"} · {found.qorebit ? "Qorebit found" : "Qorebit not found"}
          </span>
        </div>
      )}

      {wallet && account && (
        <>
          <p>
            Connected with <b>{names[wallet]}</b> as <code title={account}>{short(account, 12, 8)}</code>{" "}
            <button onClick={disconnect}>Disconnect</button>
          </p>
          <h3>Send a transaction</h3>
          <div style={{ display: "grid", gap: "0.5rem" }}>
            <input value={to} onChange={(e) => setTo(e.target.value)} placeholder="Receiver qor1…" aria-label="Receiver address" style={{ padding: "0.5rem" }} />
            <input value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="Amount in QOR" aria-label="Amount in QOR" style={{ padding: "0.5rem" }} />
            <input value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Memo (optional)" aria-label="Memo" style={{ padding: "0.5rem" }} />
            <button onClick={() => void send()} disabled={busy}>
              {busy ? "Waiting for the wallet…" : `Send with ${names[wallet]}`}
            </button>
          </div>
          <p style={{ color: "#555", fontSize: 14 }}>
            Network: <code>{chainId}</code>. Check the amount and address in {names[wallet]} before you approve.
          </p>
        </>
      )}

      {sent && (
        <p role="status" style={{ background: "#e8f5e9", padding: "0.75rem", borderRadius: 6 }}>
          Sent with {names[sent.wallet]}. {sent.note && <>({sent.note}) </>}
          {sent.hash ? (
            <>
              Transaction{" "}
              <a href={`${EXPLORER}/tx/${sent.hash}`} target="_blank" rel="noreferrer noopener">
                <code>{short(sent.hash, 10, 8)}</code> in the explorer ↗
              </a>
            </>
          ) : (
            "The wallet did not return a hash."
          )}
        </p>
      )}
      {error && (
        <p style={{ color: "#b00020" }} role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
