/** The extension's password lock. It only gates the wallet's pages (the keys are on the device, behind its PIN): a password a person at
 *  your computer must know to open the wallet. The password is never stored: only a salted PBKDF2-SHA-256 hash (600,000 rounds) in the
 *  extension's local storage. "Unlocked" is a flag in the browser's session storage, so it lasts until the browser is closed. */

const LOCK = "qorebit.lock"; // chrome.storage.local: { salt, hash, iter } and { fails, until } for the throttle
const THROTTLE = "qorebit.lock.throttle";
const OPEN = "qorebit.unlocked"; // chrome.storage.session
export const MIN_PASSWORD = 8;
const ITER = 600_000;

interface Store {
  get(key: string): Promise<Record<string, any>>;
  set(o: Record<string, unknown>): Promise<void>;
  remove(key: string): Promise<void>;
}
const c = () => (globalThis as any).browser ?? (globalThis as any).chrome; // Firefox or Chrome
/** The extension's storage areas, or null on the website (no lock there). */
function areas(): { local: Store; session: Store } | null {
  const s = c()?.storage;
  return s?.local && s?.session ? { local: s.local, session: s.session } : null;
}
export const lockAvailable = () => areas() !== null;

const b64 = (b: Uint8Array) => btoa(Array.from(b, (x) => String.fromCharCode(x)).join(""));
const unb64 = (s: string): Uint8Array<ArrayBuffer> => Uint8Array.from(atob(s), (ch) => ch.charCodeAt(0));
async function derive(password: string, salt: Uint8Array<ArrayBuffer>, iter: number): Promise<Uint8Array> {
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(password.normalize("NFKC")), "PBKDF2", false, ["deriveBits"]);
  return new Uint8Array(await crypto.subtle.deriveBits({ name: "PBKDF2", salt, iterations: iter, hash: "SHA-256" }, key, 256));
}
const same = (a: Uint8Array, b: Uint8Array) => a.length === b.length && a.reduce((d, x, i) => d | (x ^ b[i]), 0) === 0;

export async function hasPassword(): Promise<boolean> {
  const a = areas();
  return !!a && !!(await a.local.get(LOCK))[LOCK];
}
export async function isUnlocked(): Promise<boolean> {
  const a = areas();
  return !!a && !!(await a.session.get(OPEN))[OPEN];
}
export const markUnlocked = () => areas()!.session.set({ [OPEN]: true });
export const lockNow = () => areas()?.session.remove(OPEN) ?? Promise.resolve();

export function passwordProblem(p: string): string {
  if (p.length < MIN_PASSWORD) return `Use at least ${MIN_PASSWORD} characters.`;
  if (/^(.)\1+$/.test(p)) return "Use more than one repeated character.";
  return "";
}
/** 0..4, a rough guide only (length and variety). */
export function strength(p: string): number {
  let n = 0;
  if (p.length >= MIN_PASSWORD) n++;
  if (p.length >= 12) n++;
  if (/[a-z]/.test(p) && /[A-Z]/.test(p)) n++;
  if (/\d/.test(p) && /[^A-Za-z0-9]/.test(p)) n++;
  return n;
}

export async function setPassword(password: string): Promise<void> {
  const a = areas()!;
  const problem = passwordProblem(password);
  if (problem) throw new Error(problem);
  const salt = crypto.getRandomValues(new Uint8Array(new ArrayBuffer(16)));
  const hash = await derive(password, salt, ITER);
  await a.local.set({ [LOCK]: { salt: b64(salt), hash: b64(hash), iter: ITER } });
  await a.local.remove(THROTTLE);
  await a.session.set({ [OPEN]: true });
}

export async function removePassword(): Promise<void> {
  const a = areas()!;
  await a.local.remove(LOCK);
  await a.local.remove(THROTTLE);
  await a.session.remove(OPEN);
}

export type Attempt = { ok: true } | { ok: false; wait: number };
/** Check a password. After 5 wrong tries each further one waits longer (2, 4, 8… seconds, up to 5 minutes). */
export async function unlock(password: string): Promise<Attempt> {
  const a = areas()!;
  const th = ((await a.local.get(THROTTLE))[THROTTLE] ?? { fails: 0, until: 0 }) as { fails: number; until: number };
  const now = Date.now();
  if (th.until > now) return { ok: false, wait: Math.ceil((th.until - now) / 1000) };
  const rec = (await a.local.get(LOCK))[LOCK] as { salt: string; hash: string; iter: number } | undefined;
  if (!rec) return { ok: true };
  const ok = same(await derive(password, unb64(rec.salt), rec.iter), unb64(rec.hash));
  if (ok) {
    await a.local.remove(THROTTLE);
    await a.session.set({ [OPEN]: true });
    return { ok: true };
  }
  const fails = th.fails + 1;
  const wait = fails >= 5 ? Math.min(300, 2 ** (fails - 4)) : 0;
  await a.local.set({ [THROTTLE]: { fails, until: wait ? Date.now() + wait * 1000 : 0 } });
  return { ok: false, wait };
}

export async function changePassword(current: string, next: string): Promise<Attempt> {
  const r = await unlock(current);
  if (!r.ok) return r;
  await setPassword(next);
  return { ok: true };
}

/** Calls `f` when the lock state changes (in this page or another one: the side panel and the approval window share it). */
export function onLockChange(f: () => void): () => void {
  const ev = c()?.storage?.onChanged;
  if (!ev) return () => undefined;
  const h = (changes: Record<string, unknown>, area: string) => {
    if ((area === "session" && OPEN in changes) || (area === "local" && LOCK in changes)) f();
  };
  ev.addListener(h);
  return () => ev.removeListener(h);
}
