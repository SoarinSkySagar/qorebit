import test from "node:test";
import assert from "node:assert/strict";

// a mock of the extension's storage (local survives, session is cleared when the browser closes)
const mk = () => { const d = {}; return { get: async (k) => ({ [k]: d[k] }), set: async (o) => void Object.assign(d, o), remove: async (k) => void delete d[k], d }; };
const local = mk();
let session = mk();
globalThis.chrome = { storage: { local, get session() { return session; } } };
const L = await import("../src/lock.ts");

test("no password at first; setting one unlocks and stores only a salted hash", async () => {
  assert.equal(await L.hasPassword(), false);
  assert.equal(L.passwordProblem("short"), "Use at least 8 characters.");
  assert.equal(L.passwordProblem("aaaaaaaaaa") !== "", true);
  await L.setPassword("correct horse");
  assert.equal(await L.hasPassword(), true);
  assert.equal(await L.isUnlocked(), true);
  const stored = JSON.stringify(local.d);
  assert.equal(stored.includes("correct horse"), false);
  assert.equal(local.d["qorebit.lock"].iter, 600000);
});

test("a new browser session is locked; the right password opens it; a wrong one does not", async () => {
  session = mk(); // the browser was closed
  assert.equal(await L.isUnlocked(), false);
  assert.equal((await L.unlock("wrong password")).ok, false);
  assert.equal(await L.isUnlocked(), false);
  assert.equal((await L.unlock("correct horse")).ok, true);
  assert.equal(await L.isUnlocked(), true);
  await L.lockNow();
  assert.equal(await L.isUnlocked(), false);
});

test("five wrong tries start a wait that grows, and the right password waits too", async () => {
  let r;
  for (let i = 0; i < 5; i++) r = await L.unlock("nope nope nope");
  assert.equal(r.ok, false);
  assert.ok(r.wait >= 2, "waits after the fifth miss");
  const during = await L.unlock("correct horse");
  assert.equal(during.ok, false, "even the right password waits");
  assert.ok(during.wait > 0);
  local.d["qorebit.lock.throttle"].until = 0; // time passes
  assert.equal((await L.unlock("correct horse")).ok, true);
});

test("changing the password needs the current one; removing it leaves no lock", async () => {
  assert.equal((await L.changePassword("not it at all", "brand new pass")).ok, false);
  assert.equal((await L.changePassword("correct horse", "brand new pass")).ok, true);
  session = mk();
  assert.equal((await L.unlock("correct horse")).ok, false);
  assert.equal((await L.unlock("brand new pass")).ok, true);
  await L.removePassword();
  assert.equal(await L.hasPassword(), false);
  assert.equal(await L.isUnlocked(), false);
});
