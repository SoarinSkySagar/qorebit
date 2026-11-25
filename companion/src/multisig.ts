/** Multisig on QoreChain: the chain's group module. A multisig is a group of members with a decision policy
 *  ("N of M must vote yes") and an account of its own (the group policy address) that holds the funds. Moving
 *  them takes a proposal, enough yes votes, and an execution: each of those is an ordinary transaction that one
 *  member signs on their own device (hybrid secp256k1 + ML-DSA-87), so no special signing mode is needed. */
import type { NetworkId } from "./networks";
import { chainJson } from "./chain";

export { MAX_MEMBERS, VOTE_NAME, createMultisigMsg, proposePaymentMsg, voteMsg, execMsg, type VoteOption } from "./multisigMsgs";
import type { VoteOption } from "./multisigMsgs";

// ---- reading what is on chain

export interface Member {
  address: string;
}
export interface Multisig {
  groupId: string;
  policy: string; // the multisig's own account: send funds here
  name: string;
  threshold: number;
  totalWeight: number;
  votingSeconds: number;
  members: Member[];
}
export interface Action {
  kind: "send" | "other";
  text: string;
  to?: string;
  amount?: bigint;
}
export interface Proposal {
  id: string;
  title: string;
  status: string; // PROPOSAL_STATUS_*
  executor: string; // PROPOSAL_EXECUTOR_RESULT_*
  votingEnd: string;
  actions: Action[];
  tally: { yes: number; no: number; abstain: number; veto: number };
  voted: VoteOption | null; // this account's vote
}

const secs = (d: unknown) => parseInt(String(d ?? "0"), 10) || 0;

function action(m: any): Action {
  if (m?.["@type"] === "/cosmos.bank.v1beta1.MsgSend") {
    const coin = (m.amount ?? []).find((c: any) => c.denom === "uqor");
    return { kind: "send", to: m.to_address, amount: coin ? BigInt(coin.amount) : 0n, text: "Send" };
  }
  return { kind: "other", text: String(m?.["@type"] ?? "unknown").split(".").pop() ?? "unknown" };
}

/** The multisigs `address` is a member of, on this network. */
export async function multisigsOf(net: NetworkId, address: string): Promise<Multisig[]> {
  const g = await chainJson(net, `/cosmos/group/v1/groups_by_member/${address}`);
  if (g.status !== 200) return [];
  const out: Multisig[] = [];
  for (const group of g.json.groups ?? []) {
    const [pol, mem] = await Promise.all([
      chainJson(net, `/cosmos/group/v1/group_policies_by_group/${group.id}`),
      chainJson(net, `/cosmos/group/v1/group_members/${group.id}`),
    ]);
    const members: Member[] = (mem.json.members ?? []).map((m: any) => ({ address: m.member.address }));
    for (const p of pol.json.group_policies ?? []) {
      const d = p.decision_policy ?? {};
      if (d["@type"] !== "/cosmos.group.v1.ThresholdDecisionPolicy") continue; // the device and this app handle threshold multisigs
      out.push({
        groupId: String(group.id),
        policy: p.address,
        name: String(p.metadata || group.metadata || ""),
        threshold: parseInt(d.threshold, 10) || 0,
        totalWeight: parseInt(group.total_weight, 10) || members.length,
        votingSeconds: secs(d.windows?.voting_period),
        members,
      });
    }
  }
  return out;
}

export async function proposalsOf(net: NetworkId, policy: string, me: string): Promise<Proposal[]> {
  const r = await chainJson(net, `/cosmos/group/v1/proposals_by_group_policy/${policy}`);
  if (r.status !== 200) return [];
  const out: Proposal[] = [];
  for (const p of r.json.proposals ?? []) {
    const [tally, votes] = await Promise.all([
      chainJson(net, `/cosmos/group/v1/proposals/${p.id}/tally`),
      chainJson(net, `/cosmos/group/v1/votes_by_proposal/${p.id}`),
    ]);
    const t = tally.json.tally ?? p.final_tally_result ?? {};
    const mine = (votes.json.votes ?? []).find((v: any) => v.voter === me);
    const option = mine ? ({ VOTE_OPTION_YES: 1, VOTE_OPTION_ABSTAIN: 2, VOTE_OPTION_NO: 3, VOTE_OPTION_NO_WITH_VETO: 4 } as Record<string, VoteOption>)[mine.option] ?? null : null;
    out.push({
      id: String(p.id),
      title: String(p.title || p.metadata || `Proposal ${p.id}`),
      status: String(p.status),
      executor: String(p.executor_result),
      votingEnd: String(p.voting_period_end ?? ""),
      actions: (p.messages ?? []).map(action),
      tally: { yes: Number(t.yes_count ?? 0), no: Number(t.no_count ?? 0), abstain: Number(t.abstain_count ?? 0), veto: Number(t.no_with_veto_count ?? 0) },
      voted: option,
    });
  }
  return out.sort((a, b) => Number(b.id) - Number(a.id));
}

export const isOpen = (p: Proposal) => p.status === "PROPOSAL_STATUS_SUBMITTED" && (!p.votingEnd || Date.parse(p.votingEnd) > Date.now());
/** The chain marks a proposal accepted when it is run or when its voting time ends, so enough yes votes count too. */
export const hasEnoughVotes = (p: Proposal, threshold: number) => p.tally.yes >= threshold;
export const canExecute = (p: Proposal, threshold: number) =>
  p.executor.endsWith("NOT_RUN") && (p.status === "PROPOSAL_STATUS_ACCEPTED" || (p.status === "PROPOSAL_STATUS_SUBMITTED" && hasEnoughVotes(p, threshold)));
export function statusText(p: Proposal, threshold: number): string {
  if (canExecute(p, threshold)) return "Passed: ready to run";
  if (p.status === "PROPOSAL_STATUS_ACCEPTED") return p.executor.endsWith("SUCCESS") ? "Done" : p.executor.endsWith("FAILURE") ? "Ran and failed" : "Passed";
  if (isOpen(p)) return "Voting";
  if (p.status === "PROPOSAL_STATUS_SUBMITTED") return "Voting ended";
  return p.status.replace("PROPOSAL_STATUS_", "").toLowerCase().replace(/^./, (c) => c.toUpperCase());
}

export function durationText(seconds: number): string {
  if (seconds > 0 && seconds % 86400 === 0) return seconds === 86400 ? "1 day" : `${seconds / 86400} days`;
  if (seconds > 0 && seconds % 3600 === 0) return seconds === 3600 ? "1 hour" : `${seconds / 3600} hours`;
  if (seconds > 0 && seconds % 60 === 0) return `${seconds / 60} minutes`;
  return `${seconds} s`;
}
