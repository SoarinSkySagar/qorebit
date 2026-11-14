// Ground-truth vectors from QoreChain's own QoreSign (@qorechain/qoresign 0.2.0): the Rust signer
// (crates/qorebit-signer) must reproduce them byte for byte.
//   node tools/gen-vectors.mjs > crates/qorebit-signer/tests/vectors.json
import { sha256 } from "@noble/hashes/sha2.js";
import { shake256 } from "@noble/hashes/sha3.js";
import {
  addressFromPublicKey, deriveCanonicalPqKey, deriveClassicalKey, signHybrid, hybridSignBytes,
  buildRegisterPqKeyMessage, signProofOfPossession, proofOfPossessionMessage, pqSign, stripExtension,
  classicalSign, encodeSignDoc, toHex, signMessage,
} from "@qorechain/qoresign";
import { MsgSend } from "cosmjs-types/cosmos/bank/v1beta1/tx";
import { TxBody, AuthInfo, SignerInfo, Fee, ModeInfo } from "cosmjs-types/cosmos/tx/v1beta1/tx";
import { SignMode } from "cosmjs-types/cosmos/tx/signing/v1beta1/signing";
import { PubKey } from "cosmjs-types/cosmos/crypto/secp256k1/keys";
import { MsgDelegate } from "cosmjs-types/cosmos/staking/v1beta1/tx";

const h = (b) => toHex(b);
const hs = (b) => toHex(sha256(b));
const mnemonic = Array(23).fill("abandon").concat("art").join(" ");
const out = { mnemonic, accounts: [], txs: [] };

for (const [index, passphrase] of [[0, ""], [1, ""], [0, "TREZOR"]]) {
  const classical = deriveClassicalKey({ mnemonic, index, passphrase });
  const address = addressFromPublicKey(classical.publicKey, "secp256k1").bech32;
  const pq = deriveCanonicalPqKey({ mnemonic, address });
  const seed = shake256(new TextEncoder().encode(`qorechain:pqc:v1|${address}|${mnemonic}`), { dkLen: 32 });
  const msg = new TextEncoder().encode("qore hardware wallet test message");
  const sig = pqSign(pq.secretKey, msg);
  out.accounts.push({
    index, passphrase, address,
    classicalPrivate: h(classical.privateKey), classicalPublic: h(classical.publicKey),
    pqSeed: h(seed), pqPublicSha256: hs(pq.publicKey), pqPublicHead: h(pq.publicKey.slice(0, 32)),
    pqMsg: h(msg), pqSigSha256: hs(sig),
  });
}

// a hybrid MsgSend, v2 and v1 sign bytes, with and without the public key in the extension
const acct = out.accounts[0];
const classical = deriveClassicalKey({ mnemonic, index: 0 });
const pqKey = deriveCanonicalPqKey({ mnemonic, address: acct.address });
function mkTx(name, { chainId, accountNumber, sequence, messages, memo = "", fee = "50000", gas = 500000n, version, includePublicKey }) {
  const bodyBytes = TxBody.encode(TxBody.fromPartial({ messages, memo })).finish();
  const authInfoBytes = AuthInfo.encode(AuthInfo.fromPartial({
    signerInfos: [SignerInfo.fromPartial({
      publicKey: { typeUrl: "/cosmos.crypto.secp256k1.PubKey", value: PubKey.encode({ key: classical.publicKey }).finish() },
      modeInfo: ModeInfo.fromPartial({ single: { mode: SignMode.SIGN_MODE_DIRECT } }),
      sequence: BigInt(sequence),
    })],
    fee: Fee.fromPartial({ amount: [{ denom: "uqor", amount: fee }], gasLimit: gas }),
  })).finish();
  const r = signHybrid({ chainId, accountNumber, bodyBytes, authInfoBytes, classicalKey: classical, pqKey, signBytesVersion: version, includePublicKey });
  const pqBytes = hybridSignBytes({ chainId, bodyBytes, authInfoBytes, signBytesVersion: version });
  out.txs.push({
    name, chainId, accountNumber, sequence, version, includePublicKey: !!includePublicKey,
    bodyBytes: h(bodyBytes), authInfoBytes: h(authInfoBytes), pqSignBytes: h(pqBytes),
    finalBodySha256: hs(r.bodyBytes), classicalSig: h(r.classicalSignature), pqSigSha256: hs(r.pqSignature),
    txBytesSha256: hs(r.txBytes),
  });
}
const send = (to, amount) => ({ typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: MsgSend.encode(MsgSend.fromPartial({ fromAddress: acct.address, toAddress: to, amount: [{ denom: "uqor", amount }] })).finish() });
const to = addressFromPublicKey(deriveClassicalKey({ mnemonic, index: 5 }).publicKey, "secp256k1").bech32;
mkTx("send-v2", { chainId: "qorechain-local", accountNumber: 7, sequence: 3, messages: [send(to, "1234567")], memo: "hello", version: "v2" });
mkTx("send-v2-pk", { chainId: "qorechain-diana", accountNumber: 12, sequence: 0, messages: [send(to, "5")], version: "v2", includePublicKey: true });
mkTx("send-v1", { chainId: "qorechain-vladi", accountNumber: 1, sequence: 99, messages: [send(to, "1000000000000")], version: "v1" });
mkTx("delegate-v2", { chainId: "qorechain-local", accountNumber: 7, sequence: 4, messages: [{ typeUrl: "/cosmos.staking.v1beta1.MsgDelegate", value: MsgDelegate.encode(MsgDelegate.fromPartial({ delegatorAddress: acct.address, validatorAddress: "qorvaloper1erhaz6w27w49esd3ldrsyg8xvwk6jns5y3ka9v", amount: { denom: "uqor", amount: "250000000" } })).finish() }], version: "v2" });

// key registration: the proof of possession and the whole MsgRegisterPQCKeyV2
const popMsg = proofOfPossessionMessage({ chainId: "qorechain-local", account: acct.address, publicKey: pqKey.publicKey });
const pop = signProofOfPossession({ chainId: "qorechain-local", account: acct.address, pqKey });
const reg = buildRegisterPqKeyMessage({ chainId: "qorechain-local", account: acct.address, pqKey, classicalPublicKey: classical.publicKey });
out.register = { chainId: "qorechain-local", popMessage: new TextDecoder().decode(popMsg), popSigSha256: hs(pop), msgValueSha256: hs(reg.value), msgTypeUrl: reg.typeUrl };
console.log(JSON.stringify(out, null, 1));
