/** The four multisig messages, as the device reads them (the chain's group module). Pure encoders: no network. */
import { MsgCreateGroupWithPolicy, MsgSubmitProposal, MsgVote, MsgExec } from "cosmjs-types/cosmos/group/v1/tx";
import { ThresholdDecisionPolicy } from "cosmjs-types/cosmos/group/v1/types";
import { MsgSend } from "cosmjs-types/cosmos/bank/v1beta1/tx";
import type { AnyMsg } from "./tx";

/** A sanity limit for the form (the chain has its own). */
export const MAX_MEMBERS = 12;

export type VoteOption = 1 | 2 | 3 | 4; // yes, abstain, no, no with veto
export const VOTE_NAME: Record<number, string> = { 1: "Yes", 2: "Abstain", 3: "No", 4: "No with veto" };

export function createMultisigMsg(a: { admin: string; members: string[]; threshold: number; votingSeconds: number; name: string }): AnyMsg {
  const policy = ThresholdDecisionPolicy.fromPartial({
    threshold: String(a.threshold),
    windows: { votingPeriod: { seconds: BigInt(a.votingSeconds), nanos: 0 }, minExecutionPeriod: { seconds: 0n, nanos: 0 } },
  });
  return {
    typeUrl: "/cosmos.group.v1.MsgCreateGroupWithPolicy",
    value: MsgCreateGroupWithPolicy.encode(
      MsgCreateGroupWithPolicy.fromPartial({
        admin: a.admin,
        members: a.members.map((address) => ({ address, weight: "1", metadata: "" })),
        groupMetadata: a.name,
        groupPolicyMetadata: a.name,
        groupPolicyAsAdmin: false,
        decisionPolicy: { typeUrl: "/cosmos.group.v1.ThresholdDecisionPolicy", value: ThresholdDecisionPolicy.encode(policy).finish() },
      }),
    ).finish(),
  };
}

/** Propose that the multisig pays `to`. Nothing moves until enough members vote yes and someone executes it. */
export function proposePaymentMsg(a: { policy: string; proposer: string; to: string; uqor: bigint; title: string }): AnyMsg {
  const send: AnyMsg = {
    typeUrl: "/cosmos.bank.v1beta1.MsgSend",
    value: MsgSend.encode(MsgSend.fromPartial({ fromAddress: a.policy, toAddress: a.to, amount: [{ denom: "uqor", amount: a.uqor.toString() }] })).finish(),
  };
  return {
    typeUrl: "/cosmos.group.v1.MsgSubmitProposal",
    value: MsgSubmitProposal.encode(
      MsgSubmitProposal.fromPartial({ groupPolicyAddress: a.policy, proposers: [a.proposer], metadata: "", messages: [send], exec: 0, title: a.title, summary: a.title }),
    ).finish(),
  };
}

export const voteMsg = (proposalId: bigint, voter: string, option: VoteOption): AnyMsg => ({
  typeUrl: "/cosmos.group.v1.MsgVote",
  value: MsgVote.encode(MsgVote.fromPartial({ proposalId, voter, option, metadata: "", exec: 0 })).finish(),
});

export const execMsg = (proposalId: bigint, executor: string): AnyMsg => ({
  typeUrl: "/cosmos.group.v1.MsgExec",
  value: MsgExec.encode(MsgExec.fromPartial({ proposalId, executor })).finish(),
});

