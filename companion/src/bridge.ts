/** The device, through the bridge (tools/qorebit_display.py): `POST /rpc {method, params}`. */
import { BRIDGE, DEFAULT_NETWORKS, type NetworkId, type NetworkInfo } from "./networks";

export class DeviceError extends Error {
  constructor(message: string, readonly code: number) {
    super(message);
  }
  /** The user said no on the device (a tap, or the 60 s timeout). */
  get rejected() {
    return this.code === 9;
  }
  /** The device read the transaction and will not sign it. */
  get refused() {
    return this.code === 11;
  }
}

export async function rpc<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
  let r: Response;
  try {
    r = await fetch(`${BRIDGE}/rpc`, { method: "POST", body: JSON.stringify({ method, params }) });
  } catch {
    throw new DeviceError("The bridge is not running (make web, or make bridge).", -2);
  }
  const j = (await r.json()) as { result?: T; error?: { code: number; message: string } };
  if (j.error) throw new DeviceError(j.error.message.replace(/^[a-z ]+: /, (m) => (m.length < 28 ? "" : m)), j.error.code);
  return j.result as T;
}

export async function bridgeUp(): Promise<boolean> {
  try {
    const r = await fetch(`${BRIDGE}/`);
    return r.ok;
  } catch {
    return false;
  }
}

export async function loadNetworks(): Promise<Record<NetworkId, NetworkInfo>> {
  try {
    const r = await fetch(`${BRIDGE}/networks`);
    if (r.ok) return { ...DEFAULT_NETWORKS, ...((await r.json()) as Record<NetworkId, NetworkInfo>) };
  } catch {
    /* the defaults */
  }
  return DEFAULT_NETWORKS;
}

export interface Security {
  chip: boolean;
  developerMode: boolean;
  paranoid: boolean;
  requirePq: boolean;
  possibleAttacks: number;
  seedStorage: "plain" | "wrapped" | "hostfile" | "unknown";
  pinSet: boolean;
  pinTriesLeft: number;
  passphrase: boolean;
  firmware: string;
}

export interface DeviceAccount {
  address: string;
  publicKey: string;
  bech32: string;
}

export const device = {
  security: () => rpc<Security>("device.security"),
  info: () => rpc<{ text: string }>("device.info"),
  account: (account: number) => rpc<DeviceAccount>("device.account", { account }),
  pqPublicKey: (account: number) => rpc<{ publicKey: string }>("device.pqPublicKey", { account }),
  registerProof: (account: number, chainId: string) => rpc<{ ms: number; proof: string }>("device.registerProof", { account, chainId }),
  signTx: (p: {
    account: number;
    accountNumber: bigint;
    chainId: string;
    body: string;
    authInfo: string;
    signBytes: "v1" | "v2";
    classicalOnly?: boolean;
    includePublicKey?: boolean;
  }) =>
    rpc<{ ms: number; txBytes: string; classicalSignature: string; pqSignature: string | null }>("device.signTx", {
      ...p,
      accountNumber: p.accountNumber.toString(),
    }),
  home: (account: number, balance: bigint, chainId: string, address: string) =>
    rpc<{ addressChecked: boolean | null }>("device.home", { account, balance: balance.toString(), chainId, address }),
  setPin: () => rpc("device.setPin"),
  resetPin: () => rpc("device.resetPin"),
  showPhrase: () => rpc("device.showPhrase"),
  restore: () => rpc("device.restore"),
  wipe: () => rpc("device.wipe"),
  passphrase: () => rpc<{ passphrase: boolean }>("device.passphrase"),
};

export async function fundDevnet(address: string, uqor: bigint): Promise<string> {
  const r = await fetch(`${BRIDGE}/devnet/fund`, { method: "POST", body: JSON.stringify({ address, amount: Number(uqor) }) });
  const j = (await r.json()) as { result?: { result: string }; error?: { message: string } };
  if (j.error) throw new Error(j.error.message);
  return j.result!.result;
}
