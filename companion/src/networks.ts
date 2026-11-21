/** The three networks the wallet can talk to, and how it reaches them. */

export type NetworkId = "mainnet" | "testnet" | "devnet";
export const NETWORK_IDS: NetworkId[] = ["mainnet", "testnet", "devnet"];

export interface NetworkInfo {
  name: string;
  chainId: string;
  explorer: string | null;
  /** A URL for a public faucet, "bridge" for the local devnet's, or none. */
  faucet: string | null;
  /** Real funds. */
  real: boolean;
}

/** Same as tools/networks.json (the bridge serves the authoritative copy at /networks). */
export const DEFAULT_NETWORKS: Record<NetworkId, NetworkInfo> = {
  mainnet: { name: "Mainnet", chainId: "qorechain-vladi", explorer: "https://explore.qore.network", faucet: null, real: true },
  testnet: { name: "Testnet", chainId: "qorechain-diana", explorer: "https://explore.qore.network", faucet: "https://dashboard.qorechain.io/faucet", real: false },
  devnet: { name: "Local devnet", chainId: "qorechain-local", explorer: null, faucet: "bridge", real: false },
};

export const BRIDGE: string = (import.meta.env.VITE_BRIDGE as string | undefined) ?? "http://127.0.0.1:7630";

/** QoreChain's public endpoints send no CORS headers, so the browser goes through the bridge. */
export const restBase = (net: NetworkId) => `${BRIDGE}/proxy/${net}/rest`;
