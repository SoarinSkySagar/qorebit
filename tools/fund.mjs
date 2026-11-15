// Send test QOR from the local devnet's faucet account.
//   node tools/fund.mjs <qor1... address> [uqor, default 1,000 QOR]
//
// The faucet is a software wallet from a public mnemonic (devnet/devnet.sh creates the same account
// in qorechaind): it registers its post-quantum key on first use, then sends a hybrid-signed
// MsgSend, exactly what QoreChain wallets do. Local devnet only.
import {
  addressFromPublicKey, buildRegisterPqKeyMessage, deriveCanonicalPqKey, deriveClassicalKey, parseAddress, signHybrid,
} from "@qorechain/qoresign";
import { broadcastTx, fetchPqKeyStatus, resolveSignBytesVersion } from "@qorechain/qoresign/network";
import { accountInfo, buildTx, restOf, sendMsg, waitTx } from "./qorelib.mjs";

const CHAIN_ID = "qorechain-local";
const FAUCET_WORDS = Array(23).fill("abandon").concat("art").join(" ");

const [to, amount = "1000000000"] = process.argv.slice(2);
if (!to) {
  console.error("usage: node tools/fund.mjs <qor1... address> [uqor]");
  process.exit(2);
}
try { parseAddress(to); } catch { console.error(`not a qor1 address: ${to}`); process.exit(2); }
if (!/^[0-9]+$/.test(amount) || BigInt(amount) > 100_000_000_000n) { console.error("amount in uqor, at most 100,000 QOR"); process.exit(2); }

const rest = restOf("devnet");
const classicalKey = deriveClassicalKey({ mnemonic: FAUCET_WORDS });
const address = addressFromPublicKey(classicalKey.publicKey, "secp256k1").bech32;
const pqKey = deriveCanonicalPqKey({ mnemonic: FAUCET_WORDS, address });

async function submit({ messages, classicalOnly }) {
  const info = await accountInfo(rest, address);
  if (!info) throw new Error("the faucet account does not exist on chain: is the devnet running (make devnet)?");
  const { bodyBytes, authInfoBytes } = buildTx({ messages, publicKey: classicalKey.publicKey, sequence: info.sequence });
  const version = await resolveSignBytesVersion({ rest, chainId: CHAIN_ID });
  const { txBytes } = signHybrid({
    chainId: CHAIN_ID, accountNumber: info.accountNumber, bodyBytes, authInfoBytes, classicalKey, pqKey,
    signBytesVersion: version, mode: classicalOnly ? "classicalOnly" : "hybrid",
  });
  const r = await broadcastTx({ rest, txBytes });
  if (r.code !== 0) throw new Error(`the chain refused the transaction (code ${r.code} ${r.codespace}): ${r.log}`);
  const done = await waitTx(rest, r.txhash);
  if (done.code !== 0) throw new Error(`the transaction failed on chain (code ${done.code}): ${done.raw_log}`);
  return r.txhash;
}

const status = await fetchPqKeyStatus({ rest, address });
if (!status.registered) {
  const msg = buildRegisterPqKeyMessage({ chainId: CHAIN_ID, account: address, pqKey, classicalPublicKey: classicalKey.publicKey });
  await submit({ messages: [msg], classicalOnly: true });
  console.error("faucet: post-quantum key registered");
}
const hash = await submit({ messages: [sendMsg(address, to, amount)] });
console.log(`sent ${amount} uqor to ${to}: ${hash}`);
