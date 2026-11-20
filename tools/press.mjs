#!/usr/bin/env node
// Plays the device's one button for scripted demos and tests: answers every screen the device shows
// (DISPLAY_INPUT through a bridge's raw port): a hold on approvals and for 1-bits of the PIN, a tap for
// 0-bits. Emulator only (the screen is the laptop's anyway).
//   node tools/press.mjs [--bridge 7620] [--pin 178] [--reject]
import net from "node:net";

const arg = (n, d) => { const i = process.argv.indexOf(n); return i < 0 ? d : Number(process.argv[i + 1]); };
const BRIDGE = arg("--bridge", 7620), PIN = arg("--pin", 0b1011_0010), HOLD = 1000;
const reject = process.argv.includes("--reject");
const frame = (cmd, payload = Buffer.alloc(0)) => new Promise((resolve, reject) => {
  const s = net.createConnection(BRIDGE, "127.0.0.1");
  let buf = Buffer.alloc(0);
  s.on("data", (d) => { buf = Buffer.concat([buf, d]); if (buf.length >= 4 && buf.length >= 4 + buf.readUInt32BE(0)) { s.end(); resolve(buf.subarray(6, 4 + buf.readUInt32BE(0))); } });
  s.on("error", reject);
  const body = Buffer.concat([Buffer.from([0x51, cmd]), payload]);
  const h = Buffer.alloc(4); h.writeUInt32BE(body.length); s.write(Buffer.concat([h, body]));
});
let last = -1, bits = 0;
for (;;) {
  await new Promise((r) => setTimeout(r, 100));
  let p;
  try { p = await frame(0x30); } catch { continue; }
  const id = p.readUInt32BE(4), kind = p[8];
  if (id === last || kind === 0 || kind === 2) continue;
  last = id;
  let held = HOLD;
  if (kind === 1) { held = reject ? 0 : HOLD; console.log(`approval -> ${reject ? "tap (reject)" : "hold (approve)"}`); }
  else if (kind === 3) { held = (PIN >> (7 - bits % 8)) & 1 ? HOLD : 0; bits++; }
  const b = Buffer.alloc(8); b.writeUInt32BE(id, 0); b.writeUInt32BE(held, 4);
  await frame(0x31, b);
}
