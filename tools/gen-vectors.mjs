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
import { MsgCreateGroupWithPolicy, MsgSubmitProposal, MsgVote, MsgExec } from "cosmjs-types/cosmos/group/v1/tx";
import { ThresholdDecisionPolicy, PercentageDecisionPolicy } from "cosmjs-types/cosmos/group/v1/types";

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

// multisig (the chain's group module): messages as cosmjs-types encodes them, with what the device must read from each
const member1 = out.accounts[1].address;
const policyAddr = to; // any valid address stands in for a group policy account
const any = (typeUrl, value) => ({ typeUrl, value });
const thr = (t, secs) => any("/cosmos.group.v1.ThresholdDecisionPolicy", ThresholdDecisionPolicy.encode(ThresholdDecisionPolicy.fromPartial({ threshold: t, windows: { votingPeriod: { seconds: BigInt(secs), nanos: 0 }, minExecutionPeriod: { seconds: 0n, nanos: 0 } } })).finish());
const wrap = (typeUrl, value) => { const body = TxBody.encode(TxBody.fromPartial({ messages: [{ typeUrl, value }] })).finish(); return h(body); };
const groupMsgs = [];
const add = (name, typeUrl, value, expect) => groupMsgs.push({ name, body: wrap(typeUrl, value), expect });
add("create-2of2", "/cosmos.group.v1.MsgCreateGroupWithPolicy",
  MsgCreateGroupWithPolicy.encode(MsgCreateGroupWithPolicy.fromPartial({ admin: acct.address, members: [{ address: acct.address, weight: "1", metadata: "" }, { address: member1, weight: "1", metadata: "" }], groupMetadata: "team", groupPolicyMetadata: "", groupPolicyAsAdmin: false, decisionPolicy: thr("2", 86400) })).finish(),
  { kind: "create", admin: acct.address, members: [acct.address, member1], threshold: "2", votingSecs: 86400 });
add("create-weighted", "/cosmos.group.v1.MsgCreateGroupWithPolicy",
  MsgCreateGroupWithPolicy.encode(MsgCreateGroupWithPolicy.fromPartial({ admin: acct.address, members: [{ address: acct.address, weight: "3", metadata: "" }, { address: member1, weight: "1", metadata: "" }], decisionPolicy: thr("2", 3600) })).finish(),
  { kind: "create", admin: acct.address, members: [acct.address, member1], threshold: "2", votingSecs: 3600, weights: ["3", "1"] });
add("create-percentage", "/cosmos.group.v1.MsgCreateGroupWithPolicy",
  MsgCreateGroupWithPolicy.encode(MsgCreateGroupWithPolicy.fromPartial({ admin: acct.address, members: [{ address: acct.address, weight: "1", metadata: "" }], decisionPolicy: any("/cosmos.group.v1.PercentageDecisionPolicy", PercentageDecisionPolicy.encode(PercentageDecisionPolicy.fromPartial({ percentage: "0.5", windows: { votingPeriod: { seconds: 600n, nanos: 0 }, minExecutionPeriod: { seconds: 0n, nanos: 0 } } })).finish()) })).finish(),
  { kind: "create", admin: acct.address, members: [acct.address], percentage: "0.5", votingSecs: 600 });
const sendFromPolicy = (amount) => any("/cosmos.bank.v1beta1.MsgSend", MsgSend.encode(MsgSend.fromPartial({ fromAddress: policyAddr, toAddress: member1, amount: [{ denom: "uqor", amount }] })).finish());
add("propose-send", "/cosmos.group.v1.MsgSubmitProposal",
  MsgSubmitProposal.encode(MsgSubmitProposal.fromPartial({ groupPolicyAddress: policyAddr, proposers: [acct.address], metadata: "", messages: [sendFromPolicy("1500000")], exec: 0, title: "Pay vendor", summary: "Invoice 42" })).finish(),
  { kind: "propose", policy: policyAddr, proposers: [acct.address], title: "Pay vendor", innerTo: member1, innerAmount: "1500000", innerFrom: policyAddr });
add("propose-two-actions", "/cosmos.group.v1.MsgSubmitProposal",
  MsgSubmitProposal.encode(MsgSubmitProposal.fromPartial({ groupPolicyAddress: policyAddr, proposers: [acct.address], messages: [sendFromPolicy("1"), sendFromPolicy("2")], title: "Two", summary: "" })).finish(),
  { kind: "propose", policy: policyAddr, proposers: [acct.address], title: "Two", innerCount: 2 });
add("vote-yes", "/cosmos.group.v1.MsgVote", MsgVote.encode(MsgVote.fromPartial({ proposalId: 7n, voter: acct.address, option: 1, metadata: "", exec: 0 })).finish(), { kind: "vote", proposalId: 7, voter: acct.address, option: 1 });
add("vote-for-someone-else", "/cosmos.group.v1.MsgVote", MsgVote.encode(MsgVote.fromPartial({ proposalId: 7n, voter: member1, option: 1 })).finish(), { kind: "vote", proposalId: 7, voter: member1, option: 1 });
add("exec", "/cosmos.group.v1.MsgExec", MsgExec.encode(MsgExec.fromPartial({ proposalId: 7n, executor: acct.address })).finish(), { kind: "exec", proposalId: 7, executor: acct.address });
out.group = { me: acct.address, member1, policy: policyAddr, messages: groupMsgs };
console.log(JSON.stringify(out, null, 1));
