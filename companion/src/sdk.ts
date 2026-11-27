// QoreChain's own SDK (@qorechain/sdk, the library `create-qorechain-dapp` starts a dApp with), pointed at the bridge's proxy:
// QoreChain's public endpoints send no CORS headers, so the browser reaches them through the bridge. Read-only: nothing here signs
// (every signature is made on the device, see tx.ts).
import { createClient, explorerAddressUrl, fetchMinGasPriceUqor, fetchPqcStatus, getLatestBlock, type QoreChainClient } from "@qorechain/sdk";
import { DEFAULT_NETWORKS, restBase, type NetworkId } from "./networks";

const SDK_NETWORK = { mainnet: "mainnet", testnet: "testnet", devnet: "localnet" } as const;

/** An SDK client for one of the wallet's networks, reading through the bridge. */
export function qoreClient(net: NetworkId): QoreChainClient {
  return createClient({ network: SDK_NETWORK[net], chainId: DEFAULT_NETWORKS[net].chainId, endpoints: { rest: restBase(net) } });
}

export interface ChainStatus {
  height: string;
  blockTime: string | null;
  chainId: string | null;
  /** The chain's minimum gas price, uqor per gas (exact decimal string). */
  minGasPrice: string | null;
  /** Whether this account's ML-DSA-87 key is registered; null when the chain could not say (never read as "no"). */
  pqRegistered: boolean | null;
  explorerAddress: string | null;
}

export async function chainStatus(net: NetworkId, address: string | null): Promise<ChainStatus> {
  const client = qoreClient(net);
  const rest = restBase(net);
  const [block, gas, pq] = await Promise.allSettled([
    getLatestBlock(client.rest),
    fetchMinGasPriceUqor({ rest }),
    address ? fetchPqcStatus({ rest, address }) : Promise.reject(new Error("no address")),
  ]);
  if (block.status === "rejected") throw block.reason;
  const header = block.value.block?.header;
  const explorer = DEFAULT_NETWORKS[net].explorer;
  return {
    height: header?.height ?? "?",
    blockTime: header?.time ?? null,
    chainId: header?.chain_id ?? null,
    minGasPrice: gas.status === "fulfilled" ? gas.value : null,
    pqRegistered: pq.status === "fulfilled" ? pq.value.registered : null,
    explorerAddress: explorer && address ? explorerAddressUrl({ explorerUrl: explorer }, address) : null,
  };
}
