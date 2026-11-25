/** Token amounts and EVM addresses: pure helpers, no network. */
import { keccak_256 } from "@noble/hashes/sha3.js";

export const isEvmAddress = (s: string): boolean => /^0x[0-9a-fA-F]{40}$/.test(s.trim());

/** The EIP-55 mixed-case form of a 20-byte hex address: the casing QoreChain's `erc20:0x…` denoms use. */
export function eip55(address: string): string {
  const a = address.trim().toLowerCase().replace(/^0x/, "");
  const hash = Array.from(keccak_256(new TextEncoder().encode(a)), (b) => b.toString(16).padStart(2, "0")).join("");
  let out = "0x";
  for (let i = 0; i < a.length; i++) out += parseInt(hash[i], 16) >= 8 ? a[i].toUpperCase() : a[i];
  return out;
}

/** 1234500 with 6 decimals -> "1.2345" (trailing zeros dropped). */
export function formatUnits(amount: bigint, decimals: number, maxFraction = decimals): string {
  const neg = amount < 0n;
  const v = neg ? -amount : amount;
  const base = 10n ** BigInt(decimals);
  const whole = (v / base).toLocaleString("en-US");
  let frac = decimals ? (v % base).toString().padStart(decimals, "0").slice(0, maxFraction).replace(/0+$/, "") : "";
  return `${neg ? "-" : ""}${whole}${frac ? "." + frac : ""}`;
}

/** "1.5" with 6 decimals -> 1500000n. Plain decimals with at most `decimals` places, nothing else. */
export function parseUnits(s: string, decimals: number): bigint | null {
  const m = /^(\d{1,30})(?:\.(\d{1,36}))?$/.exec(s.trim());
  if (!m) return null;
  const frac = m[2] ?? "";
  if (frac.length > decimals) return null;
  return BigInt(m[1]) * 10n ** BigInt(decimals) + BigInt(frac.padEnd(decimals, "0") || "0");
}

/** "erc20:0x97143B771D34ddb9130b807b135Ae675A5a963CA" -> "erc20:0x9714..63CA", short enough for a line. */
export function shortDenom(denom: string): string {
  return denom.length > 20 ? `${denom.slice(0, denom.startsWith("erc20:") ? 12 : 8)}..${denom.slice(-4)}` : denom;
}
