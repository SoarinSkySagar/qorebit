// Write an emulator state file holding a chosen wallet, so tests know the 24 words.
//   node tools/make-emu-state.mjs <state file> <32-byte entropy as 64 hex digits>
// Layout (firmware/dabao-app-qorebit/src/storage.rs, HostFile): slots of 32 bytes from SEED_SLOT: slot 0
// the entropy, slot 1 "QORE0001" + the first 24 bytes of sha256(entropy); then 14 slots, then the
// PIN attempt counter (u32 LE).
import { sha256 } from "@noble/hashes/sha2.js";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

const [file, hex] = process.argv.slice(2);
if (!file || !/^[0-9a-f]{64}$/i.test(hex ?? "")) {
  console.error("usage: node tools/make-emu-state.mjs <state file> <64 hex digits>");
  process.exit(2);
}
const entropy = Buffer.from(hex, "hex");
const data = Buffer.alloc(32 * 14 + 4);
entropy.copy(data, 0);
Buffer.from("QORE0001").copy(data, 32);
Buffer.from(sha256(entropy).slice(0, 24)).copy(data, 40);
mkdirSync(dirname(file), { recursive: true });
writeFileSync(file, data);
