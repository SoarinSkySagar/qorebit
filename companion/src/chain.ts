/** Reading a QoreChain network and broadcasting to it, through the bridge's proxy. */
import { fetchPqKeyStatus, resolveSignBytesVersion, simulateTx, hasExactSimulation, broadcastTx } from "@qorechain/qoresign/network";
import { restBase, type NetworkId } from "./networks";
import { DEFAULT_GAS, simulationTx } from "./tx";

const base = (net: NetworkId) => restBase(net);

async function getJson(net: NetworkId, path: string): Promise<{ status: number; json: any }> {
  const r = await fetch(`${base(net)}${path}`);
  let json: any = {};
  try {
    json = await r.json();
  } catch {
    /* not JSON */
  }
  return { status: r.status, json };
}

export interface ChainAccount {
  accountNumber: bigint;
  sequence: bigint;
}

/** `null` when the account does not exist on chain yet (it has never received anything). */
export async function getAccount(net: NetworkId, address: string): Promise<ChainAccount | null> {
  const { status, json } = await getJson(net, `/cosmos/auth/v1beta1/accounts/${address}`);
  if (status === 404 || (status !== 200 && /not found/i.test(JSON.stringify(json)))) return null;
  if (status !== 200) throw new Error(`the ${net} node answered ${status}: ${json?.message ?? "unknown error"}`);
  const a = json.account ?? {};
  const b = a.base_account ?? a;
  return { accountNumber: BigInt(b.account_number ?? 0), sequence: BigInt(b.sequence ?? 0) };
}

export async function getBalance(net: NetworkId, address: string): Promise<bigint> {
  const { status, json } = await getJson(net, `/cosmos/bank/v1beta1/balances/${address}`);
  if (status !== 200) throw new Error(`the ${net} node answered ${status}`);
  return BigInt((json.balances ?? []).find((c: any) => c.denom === "uqor")?.amount ?? 0);
}

export async function pqStatus(net: NetworkId, address: string) {
  return fetchPqKeyStatus({ rest: base(net), address });
}

/** The hybrid sign-bytes version the network verifies now ("v2" on current QoreChain networks). */
export async function signBytes(net: NetworkId, chainId: string): Promise<"v1" | "v2"> {
  return resolveSignBytesVersion({ rest: base(net), chainId });
}

/** The chain id the node reports: the wallet refuses to sign for a node that is on another chain. */
export async function nodeChainId(net: NetworkId): Promise<string> {
  const { status, json } = await getJson(net, "/cosmos/base/tendermint/v1beta1/node_info");
  if (status !== 200) throw new Error(`the ${net} node answered ${status}`);
  return json.default_node_info?.network as string;
}

/** Gas to sign with: simulated by the node, or a safe default when it can't be. */
export async function gasFor(net: NetworkId, body: Uint8Array, authInfo: Uint8Array): Promise<{ gas: bigint; simulated: boolean }> {
  try {
    const exact = net === "devnet" ? true : await hasExactSimulation({ rest: base(net) });
    const r = await simulateTx({ rest: base(net), txBytes: simulationTx(body, authInfo), exactSimulation: exact });
    return { gas: r.suggestedGasLimit, simulated: true };
  } catch {
    return { gas: DEFAULT_GAS, simulated: false };
  }
}

export async function broadcast(net: NetworkId, txBytes: Uint8Array) {
  return broadcastTx({ rest: base(net), txBytes });
}

export interface TxResult {
  code: number;
  rawLog: string;
  height: string;
}

/** Wait for the transaction to be included (the networks make a block every few seconds). */
export async function waitForTx(net: NetworkId, hash: string, seconds = 60): Promise<TxResult> {
  for (let i = 0; i < seconds * 2; i++) {
    const { status, json } = await getJson(net, `/cosmos/tx/v1beta1/txs/${hash}`);
    if (status === 200 && json.tx_response) {
      const t = json.tx_response;
      return { code: Number(t.code ?? 0), rawLog: String(t.raw_log ?? ""), height: String(t.height ?? "") };
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error("the transaction was not included in time: it may still arrive; check the activity list");
}

export interface Activity {
  hash: string;
  height: number;
  kind: "sent" | "received" | "other";
  amount: bigint | null;
  peer: string | null;
  ok: boolean;
  time: string;
}

/** Recent transfers and other transactions of an account (two event queries, merged). */
export async function history(net: NetworkId, address: string, limit = 12): Promise<Activity[]> {
  const q = (query: string) =>
    getJson(net, `/cosmos/tx/v1beta1/txs?query=${encodeURIComponent(query)}&limit=${limit}&order_by=ORDER_BY_DESC`).then((r) => (r.status === 200 ? r.json : { tx_responses: [] }));
  const [sent, got] = await Promise.all([q(`message.sender='${address}'`), q(`transfer.recipient='${address}'`)]);
  const seen = new Map<string, Activity>();
  for (const t of [...(sent.tx_responses ?? []), ...(got.tx_responses ?? [])]) {
    if (seen.has(t.txhash)) continue;
    const msg = (t.tx?.body?.messages ?? [])[0] ?? {};
    const mine = msg.from_address === address || msg.sender === address || msg.delegator_address === address;
    const coin = (msg.amount ?? [])[0] ?? msg.amount;
    seen.set(t.txhash, {
      hash: t.txhash,
      height: Number(t.height),
      kind: msg["@type"] === "/cosmos.bank.v1beta1.MsgSend" ? (mine ? "sent" : "received") : mine ? "other" : "received",
      amount: coin && coin.denom === "uqor" ? BigInt(coin.amount) : null,
      peer: msg.to_address && mine ? msg.to_address : msg.from_address && !mine ? msg.from_address : null,
      ok: Number(t.code ?? 0) === 0,
      time: t.timestamp ?? "",
    });
  }
  return [...seen.values()].sort((a, b) => b.height - a.height).slice(0, limit);
}
