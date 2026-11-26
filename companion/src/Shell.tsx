import { useEffect, useRef, useState, type ReactNode } from "react";
import { NETWORK_IDS, type NetworkId, type NetworkInfo } from "./networks";

/** The screens of the wallet. */
export type View = "home" | "send" | "receive" | "tokens" | "activity" | "apps" | "multisig" | "smart" | "crossvm" | "signin" | "settings";
export const ACCOUNTS = 8; // native accounts 1..8; EVM-lane accounts are indices 8..16

const PATHS: Record<string, ReactNode> = {
  home: <><path d="M3 11l9-8 9 8" /><path d="M5 10v10h14V10" /></>,
  activity: <><path d="M3 12h4l3-8 4 16 3-8h4" /></>,
  apps: <><rect x="3" y="3" width="7" height="7" rx="2" /><rect x="14" y="3" width="7" height="7" rx="2" /><rect x="3" y="14" width="7" height="7" rx="2" /><rect x="14" y="14" width="7" height="7" rx="2" /></>,
  settings: <><circle cx="12" cy="12" r="3" /><path d="M19 12a7 7 0 0 0-.1-1.2l2-1.5-2-3.4-2.3.9a7 7 0 0 0-2-1.2L14.2 3h-4l-.4 2.6a7 7 0 0 0-2 1.2l-2.3-.9-2 3.4 2 1.5A7 7 0 0 0 5 12c0 .4 0 .8.1 1.2l-2 1.5 2 3.4 2.3-.9a7 7 0 0 0 2 1.2l.4 2.6h4l.4-2.6a7 7 0 0 0 2-1.2l2.3.9 2-3.4-2-1.5c.1-.4.1-.8.1-1.2z" /></>,
  send: <><path d="M5 19L19 5" /><path d="M8 5h11v11" /></>,
  receive: <><path d="M19 5L5 19" /><path d="M16 19H5V8" /></>,
  tokens: <><circle cx="12" cy="12" r="9" /><path d="M9 9.5c0-1 1.3-1.8 3-1.8s3 .8 3 1.8-1.3 1.6-3 2-3 1-3 2.1 1.3 1.8 3 1.8 3-.8 3-1.8" /></>,
  multisig: <><circle cx="8" cy="9" r="3" /><circle cx="17" cy="10" r="2.5" /><path d="M2.5 19c.6-3 2.8-4.5 5.5-4.5s4.9 1.5 5.5 4.5" /><path d="M15 15c2.8-.5 5.2.6 6 4" /></>,
  smart: <><path d="M12 3l8 3v6c0 4.5-3.2 7.8-8 9-4.8-1.2-8-4.5-8-9V6z" /><path d="M9 12l2 2 4-4" /></>,
  crossvm: <><path d="M4 8h13" /><path d="M14 4l4 4-4 4" /><path d="M20 16H7" /><path d="M10 12l-4 4 4 4" /></>,
  signin: <><rect x="4" y="3" width="16" height="18" rx="3" /><circle cx="12" cy="10" r="2.5" /><path d="M7.5 17c.8-2 2.4-3 4.5-3s3.7 1 4.5 3" /></>,
  back: <><path d="M15 5l-7 7 7 7" /></>,
  copy: <><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M5 15V6a2 2 0 0 1 2-2h8" /></>,
};
export function Icon({ name }: { name: string }) {
  return <svg viewBox="0 0 24 24" aria-hidden="true">{PATHS[name]}</svg>;
}

export const accountName = (index: number) => (index >= ACCOUNTS ? `EVM ${index - ACCOUNTS + 1}` : `Account ${index + 1}`);
export const short = (s: string, a = 10, b = 6) => (s.length > a + b + 2 ? `${s.slice(0, a)}…${s.slice(-b)}` : s);

/** A dropdown: a button that opens a popover; closes on a click outside, on Escape and on a choice. */
function useMenu() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false); };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", down);
    document.addEventListener("keydown", key);
    return () => { document.removeEventListener("mousedown", down); document.removeEventListener("keydown", key); };
  }, [open]);
  return { open, setOpen, ref };
}

export function AccountMenu({ index, onIndex, address }: { index: number; onIndex: (i: number) => void; address: string | null }) {
  const { open, setOpen, ref } = useMenu();
  const evm = index >= ACCOUNTS;
  const group = (title: string, base: number, label: string) => (
    <>
      <div className="group">{title}</div>
      {Array.from({ length: ACCOUNTS }, (_, i) => (
        <button key={base + i} role="option" aria-selected={base + i === index} onClick={() => { onIndex(base + i); setOpen(false); }}>
          <span className={`avatar ${base ? "evm" : ""}`}>{i + 1}</span>
          <span>{label} {i + 1}</span>
        </button>
      ))}
    </>
  );
  return (
    <div className="menu grow" ref={ref}>
      <button className="trigger" aria-haspopup="listbox" aria-expanded={open} onClick={() => setOpen(!open)} title="Switch account">
        <span className={`avatar ${evm ? "evm" : ""}`}>{(index % ACCOUNTS) + 1}</span>
        <span className="txt">
          <span className="t1 block">{accountName(index)}</span>
          <span className="t2 block mono">{address ? short(address, 9, 5) : "…"}</span>
        </span>
        <span className="chev" aria-hidden />
      </button>
      {open && (
        <div className="popover" role="listbox" aria-label="Account">
          {group("Native accounts", 0, "Account")}
          {group("EVM accounts (0x address too)", ACCOUNTS, "EVM")}
        </div>
      )}
    </div>
  );
}

export function NetworkMenu({ net, networks, onNet }: { net: NetworkId; networks: Record<NetworkId, NetworkInfo>; onNet: (n: NetworkId) => void }) {
  const { open, setOpen, ref } = useMenu();
  return (
    <div className="menu" ref={ref}>
      <button className={`trigger net-pill ${net}`} aria-haspopup="listbox" aria-expanded={open} onClick={() => setOpen(!open)} title={networks[net].chainId}>
        <span className="dot ok" aria-hidden />
        <span className="txt"><span className="t1 block">{networks[net].name}</span></span>
        <span className="chev" aria-hidden />
      </button>
      {open && (
        <div className="popover right" role="listbox" aria-label="Network">
          {NETWORK_IDS.map((id) => (
            <button key={id} role="option" aria-selected={id === net} onClick={() => { onNet(id); setOpen(false); }}>
              <span className={`net-pill ${id}`}><span className="dot ok" aria-hidden /></span>
              <span><span className="block">{networks[id].name}</span><span className="sub mono">{networks[id].chainId}</span></span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function TabBar({ view, onView }: { view: View; onView: (v: View) => void }) {
  const tabs: [View, string, string][] = [["home", "Home", "home"], ["activity", "Activity", "activity"], ["apps", "Apps", "apps"], ["settings", "Device", "settings"]];
  const group: Partial<Record<View, View>> = { send: "home", receive: "home", tokens: "home", multisig: "apps", smart: "apps", crossvm: "apps", signin: "apps" };
  const current = group[view] ?? view;
  return (
    <div className="tabbar">
      <nav aria-label="Main">
        {tabs.map(([v, label, icon]) => (
          <button key={v} onClick={() => onView(v)} aria-current={current === v ? "page" : undefined}>
            <Icon name={icon} />
            {label}
          </button>
        ))}
      </nav>
    </div>
  );
}

export function ScreenHead({ title, onBack }: { title: string; onBack?: () => void }) {
  return (
    <div className="screen-head">
      {onBack && <button className="back" onClick={onBack} aria-label="Back"><Icon name="back" /></button>}
      <h1>{title}</h1>
    </div>
  );
}

export function Tile({ icon, title, text, onClick }: { icon: string; title: string; text: string; onClick: () => void }) {
  return (
    <button className="tile" onClick={onClick}>
      <span className="ico"><Icon name={icon} /></span>
      <span><b>{title}</b><span className="d">{text}</span></span>
      <span className="chev" aria-hidden />
    </button>
  );
}
