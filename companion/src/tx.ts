/** Building the unsigned transaction the device is asked to read and sign. */
import { TxBody, AuthInfo, SignerInfo, Fee, ModeInfo } from "cosmjs-types/cosmos/tx/v1beta1/tx";
import { SignMode } from "cosmjs-types/cosmos/tx/signing/v1beta1/signing";
import { PubKey } from "cosmjs-types/cosmos/crypto/secp256k1/keys";
import { MsgSend } from "cosmjs-types/cosmos/bank/v1beta1/tx";
import { appendExtension, encodePqcHybridSignature, encodeTxRaw, PQC_HYBRID_TYPE_URL } from "@qorechain/qoresign";

export interface AnyMsg {
  typeUrl: string;
  value: Uint8Array;
}

export const UQOR = 1_000_000n;
/** 0.1 uqor per gas is the network's floor. */
export const GAS_PRICE_NUM = 1n; // numerator over 10
export const DEFAULT_GAS = 500_000n;

/** "1.5" QOR -> 1500000 uqor; null for anything that is not a plain amount with at most 6 decimals. */
export function parseQor(s: string): bigint | null {
  const m = /^(\d{1,15})(?:\.(\d{1,6}))?$/.exec(s.trim());
  if (!m) return null;
  return BigInt(m[1]) * UQOR + BigInt((m[2] ?? "").padEnd(6, "0") || "0");
}

export function formatQor(uqor: bigint, maxDecimals = 6): string {
  const whole = uqor / UQOR;
  const frac = (uqor % UQOR).toString().padStart(6, "0").slice(0, maxDecimals).replace(/0+$/, "");
  return frac ? `${whole.toLocaleString("en-US")}.${frac}` : whole.toLocaleString("en-US");
}

const BECH32 = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
function bech32Polymod(values: number[]): number {
  const gen = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
  let chk = 1;
  for (const v of values) {
    const top = chk >>> 25;
    chk = ((chk & 0x1ffffff) << 5) ^ v;
    for (let i = 0; i < 5; i++) if ((top >>> i) & 1) chk ^= gen[i];
  }
  return chk >>> 0;
}

/** A `qor1…` address with a good bech32 checksum: 20 bytes (an account) or 32 bytes (a multisig's own account). */
export function isQorAddress(s: string): boolean {
  if (s !== s.toLowerCase()) return false;
  const i = s.lastIndexOf("1");
  if (i !== 3 || s.slice(0, i) !== "qor") return false;
  const data = [...s.slice(i + 1)].map((c) => BECH32.indexOf(c));
  if (data.length < 7 || data.some((v) => v < 0)) return false;
  const hrp = [...s.slice(0, i)].map((c) => c.charCodeAt(0));
  if (bech32Polymod([...hrp.map((c) => c >> 5), 0, ...hrp.map((c) => c & 31), ...data]) !== 1) return false;
  const bytes = Math.floor(((data.length - 6) * 5) / 8);
  return bytes === 20 || bytes === 32;
}

export const sendMsg = (from: string, to: string, uqor: bigint): AnyMsg => ({
  typeUrl: "/cosmos.bank.v1beta1.MsgSend",
  value: MsgSend.encode(MsgSend.fromPartial({ fromAddress: from, toAddress: to, amount: [{ denom: "uqor", amount: uqor.toString() }] })).finish(),
});

// ---- MsgRegisterPQCKeyV2 (field 1 account, 2 ML-DSA-87 public key, 3 algorithm, 4 secp256k1 key, 5 "hybrid", 6 proof)
const varint = (n: number | bigint): number[] => {
  const o: number[] = [];
  let v = BigInt(n);
  do {
    let b = Number(v & 0x7fn);
    v >>= 7n;
    if (v) b |= 0x80;
    o.push(b);
  } while (v);
  return o;
};
const lenField = (field: number, bytes: Uint8Array): number[] => (bytes.length ? [...varint((field << 3) | 2), ...varint(bytes.length), ...bytes] : []);

export function registerPqKeyMsg(account: string, pqPublicKey: Uint8Array, classicalPublicKey: Uint8Array, proof: Uint8Array): AnyMsg {
  const enc = new TextEncoder();
  return {
    typeUrl: "/qorechain.pqc.v1.MsgRegisterPQCKeyV2",
    value: Uint8Array.from([
      ...lenField(1, enc.encode(account)),
      ...lenField(2, pqPublicKey),
      ...varint((3 << 3) | 0),
      ...varint(1),
      ...lenField(4, classicalPublicKey),
      ...lenField(5, enc.encode("hybrid")),
      ...lenField(6, proof),
    ]),
  };
}

export function fee(gas: bigint): bigint {
  return (gas * GAS_PRICE_NUM + 9n) / 10n; // ceil(gas * 0.1)
}

/** The unsigned body and auth info: one signer (`publicKey`, 33 bytes) at `sequence`, SIGN_MODE_DIRECT. */
/** An EVM-lane account (a coin-type-60 key) signs with the `eth_secp256k1` key type: another public-key type in the auth info. */
export type KeyType = "secp256k1" | "eth_secp256k1";
export const PUBKEY_TYPE_URL: Record<KeyType, string> = {
  secp256k1: "/cosmos.crypto.secp256k1.PubKey",
  eth_secp256k1: "/cosmos.evm.crypto.v1.ethsecp256k1.PubKey",
};

export function buildTx(a: { messages: AnyMsg[]; memo?: string; publicKey: Uint8Array; sequence: bigint; gas: bigint; keyType?: KeyType }) {
  const body = TxBody.encode(TxBody.fromPartial({ messages: a.messages, memo: a.memo ?? "" })).finish();
  const authInfo = AuthInfo.encode(
    AuthInfo.fromPartial({
      signerInfos: [
        SignerInfo.fromPartial({
          publicKey: { typeUrl: PUBKEY_TYPE_URL[a.keyType ?? "secp256k1"], value: PubKey.encode({ key: a.publicKey }).finish() },
          modeInfo: ModeInfo.fromPartial({ single: { mode: SignMode.SIGN_MODE_DIRECT } }),
          sequence: a.sequence,
        }),
      ],
      fee: Fee.fromPartial({ amount: [{ denom: "uqor", amount: fee(a.gas).toString() }], gasLimit: a.gas }),
    }),
  ).finish();
  return { body, authInfo };
}

/** The same transaction with a placeholder post-quantum layer and signature, for the node to simulate
 *  (it charges the ML-DSA-87 verification to the gas used, so the layer has to be there). */
export function simulationTx(body: Uint8Array, authInfo: Uint8Array): Uint8Array {
  const ext = encodePqcHybridSignature({ algorithmId: 1, signature: new Uint8Array(4627) });
  return encodeTxRaw({
    bodyBytes: appendExtension(body, { typeUrl: PQC_HYBRID_TYPE_URL, value: ext }),
    authInfoBytes: authInfo,
    signatures: [new Uint8Array(64)],
  });
}
