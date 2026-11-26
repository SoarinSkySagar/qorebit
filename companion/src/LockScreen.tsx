import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import {
  changePassword, hasPassword, isUnlocked, lockAvailable, lockNow, onLockChange, passwordProblem, removePassword, setPassword, strength, unlock, MIN_PASSWORD,
} from "./lock";

type Phase = "loading" | "setup" | "locked" | "open";

/** Shows the wallet only when the extension's password has been entered in this browser session. On the website (no extension) it does nothing. */
export function LockGate({ children }: { children: ReactNode }) {
  const [phase, setPhase] = useState<Phase>(lockAvailable() ? "loading" : "open");
  useEffect(() => {
    if (!lockAvailable()) return;
    let live = true;
    const check = async () => {
      const [has, open] = await Promise.all([hasPassword(), isUnlocked()]);
      if (live) setPhase(!has ? "setup" : open ? "open" : "locked");
    };
    void check();
    const off = onLockChange(() => void check()); // locked or unlocked in another window too
    return () => { live = false; off(); };
  }, []);
  if (phase === "open") return <>{children}</>;
  if (phase === "loading") return <div className="lock" />;
  return <LockScreen setup={phase === "setup"} onOpen={() => setPhase("open")} />;
}

function Mark() {
  return <div className="lock-mark" aria-hidden><span /></div>;
}

/** A password field with a show / hide button. It keeps the keyboard focus when asked to (`focus`). */
function PasswordField(p: {
  label: string; value: string; onChange: (v: string) => void; focus?: boolean; autoComplete: string; invalid?: boolean; id: string;
}) {
  const ref = useRef<HTMLInputElement>(null);
  const [show, setShow] = useState(false);
  useEffect(() => {
    if (!p.focus) return;
    // the side panel and the popup do not always have the keyboard when they open: ask now, after the first paint, and whenever the window gets focus
    const f = () => ref.current?.focus({ preventScroll: true });
    f();
    const t = [setTimeout(f, 50), setTimeout(f, 250)];
    window.addEventListener("focus", f);
    return () => { t.forEach(clearTimeout); window.removeEventListener("focus", f); };
  }, [p.focus]);
  return (
    <label className="pw" htmlFor={p.id}>
      <span>{p.label}</span>
      <span className={`pw-box ${p.invalid ? "bad" : ""}`}>
        <input
          id={p.id} ref={ref} type={show ? "text" : "password"} value={p.value} onChange={(e) => p.onChange(e.target.value)}
          autoFocus={p.focus} autoComplete={p.autoComplete} spellCheck={false} autoCapitalize="off" aria-invalid={p.invalid || undefined}
        />
        <button type="button" className="pw-eye" tabIndex={-1} onClick={() => setShow(!show)} aria-label={show ? "Hide password" : "Show password"} title={show ? "Hide" : "Show"}>
          {show ? "Hide" : "Show"}
        </button>
      </span>
    </label>
  );
}

function Strength({ value }: { value: string }) {
  const n = value ? Math.max(1, strength(value)) : 0;
  const words = ["", "Weak", "Fair", "Good", "Strong"];
  return (
    <div className="strength" aria-live="polite">
      <span className={`bars s${n}`}>{[1, 2, 3, 4].map((i) => <i key={i} className={i <= n ? "on" : ""} />)}</span>
      <span className="muted small">{words[n]}</span>
    </div>
  );
}

function LockScreen({ setup, onOpen }: { setup: boolean; onOpen: () => void }) {
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [shake, setShake] = useState(0);
  const [wait, setWait] = useState(0);

  // the wait after too many wrong tries, counted down
  useEffect(() => {
    if (wait <= 0) return;
    const t = setTimeout(() => setWait(wait - 1), 1000);
    return () => clearTimeout(t);
  }, [wait]);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (busy || !pw) return;
    setError("");
    if (setup) {
      const problem = passwordProblem(pw) || (pw !== pw2 ? "The two passwords are not the same." : "");
      if (problem) return void (setError(problem), setShake((n) => n + 1));
      setBusy(true);
      try {
        await setPassword(pw);
        onOpen();
      } finally {
        setBusy(false);
      }
      return;
    }
    setBusy(true);
    const r = await unlock(pw);
    setBusy(false);
    if (r.ok) return onOpen();
    setPw("");
    setShake((n) => n + 1);
    if (r.wait) setWait(r.wait);
    setError(r.wait ? "" : "That is not the password.");
  }

  return (
    <div className="lock">
      <form className="lock-card" onSubmit={submit} key={shake} data-shake={shake > 0 || undefined}>
        <Mark />
        <h1>{setup ? "Create a password" : "Welcome back"}</h1>
        <p className="muted lock-sub">
          {setup
            ? "It locks this wallet in your browser. Your keys stay on the device, behind its own PIN."
            : "Enter your password to open Qorebit."}
        </p>
        <PasswordField id="pw" label={setup ? "Password" : "Password"} value={pw} onChange={(v) => { setPw(v); setError(""); }} focus autoComplete={setup ? "new-password" : "current-password"} invalid={!!error} />
        {setup && <Strength value={pw} />}
        {setup && <PasswordField id="pw2" label="Repeat the password" value={pw2} onChange={(v) => { setPw2(v); setError(""); }} autoComplete="new-password" invalid={!!error && pw !== pw2} />}
        <div className={`lock-msg ${error || wait > 0 ? "err" : ""}`} role="alert">
          {wait > 0 ? `Too many tries. Wait ${wait}s.` : error || (setup ? `At least ${MIN_PASSWORD} characters. Longer is better.` : "")}
        </div>
        <button className="primary block-btn" type="submit" disabled={busy || !pw || wait > 0 || (setup && !pw2)}>
          {busy ? "…" : setup ? "Create password" : "Unlock"}
        </button>
        {!setup && (
          <details className="lock-forgot">
            <summary>Forgot the password?</summary>
            <p className="muted small">
              Remove the extension in <span className="mono">chrome://extensions</span> and load it again, then create a new password.
              Nothing is lost: your keys and accounts are on the device.
            </p>
          </details>
        )}
      </form>
    </div>
  );
}

/** In the Device screen: lock now, change the password, or remove it. */
export function PasswordCard() {
  const [mode, setMode] = useState<"idle" | "change" | "remove">("idle");
  const [cur, setCur] = useState("");
  const [next, setNext] = useState("");
  const [msg, setMsg] = useState<{ tone: "good" | "bad"; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  if (!lockAvailable()) return null;

  const done = (text: string) => { setMode("idle"); setCur(""); setNext(""); setMsg({ tone: "good", text }); };
  async function go(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setMsg(null);
    try {
      if (mode === "change") {
        const problem = passwordProblem(next);
        if (problem) return setMsg({ tone: "bad", text: problem });
        const r = await changePassword(cur, next);
        return r.ok ? done("Password changed.") : setMsg({ tone: "bad", text: r.wait ? `Too many tries. Wait ${r.wait}s.` : "That is not the current password." });
      }
      const r = await unlock(cur);
      if (!r.ok) return setMsg({ tone: "bad", text: r.wait ? `Too many tries. Wait ${r.wait}s.` : "That is not the current password." });
      await removePassword(); // the gate then asks for a new one
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="card">
      <h2>Wallet password</h2>
      <p className="muted small">The password locks this wallet in your browser until you close the browser. It does not protect the keys: the device does.</p>
      {msg && <div className={`notice ${msg.tone}`}>{msg.text}</div>}
      {mode === "idle" ? (
        <div className="actions">
          <button className="primary" onClick={() => void lockNow()}>Lock now</button>
          <button className="ghost" onClick={() => setMode("change")}>Change password</button>
          <button className="ghost danger" onClick={() => setMode("remove")}>Remove password</button>
        </div>
      ) : (
        <form onSubmit={go}>
          <PasswordField id="cur" label="Current password" value={cur} onChange={setCur} focus autoComplete="current-password" />
          {mode === "change" && <PasswordField id="new" label="New password" value={next} onChange={setNext} autoComplete="new-password" />}
          {mode === "change" && <Strength value={next} />}
          <div className="row">
            <button className="primary" type="submit" disabled={busy || !cur || (mode === "change" && !next)}>{mode === "change" ? "Change password" : "Remove password"}</button>
            <button className="ghost" type="button" onClick={() => { setMode("idle"); setCur(""); setNext(""); setMsg(null); }}>Cancel</button>
          </div>
        </form>
      )}
    </section>
  );
}
