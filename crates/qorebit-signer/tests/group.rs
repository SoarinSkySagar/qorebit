//! Multisig (the chain's group module): the device must read each message itself, show what it does, and
//! refuse what it cannot show or what is not this account's. The messages are encoded by `cosmjs-types`
//! (tests/vectors.json, made by `node tools/gen-vectors.mjs`).

use qorebit_signer::tx::{self, GroupPolicy, Mode, Msg};
use qorebit_signer::Wallet;
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

/// The decoded view of one of the vector messages, plus the checks of account `who` (0 = the author).
fn view(name: &str) -> (tx::View, Value) {
    let v = vectors();
    let auth = hex::decode(s(&v["txs"][0]["authInfoBytes"])).unwrap();
    let m = v["group"]["messages"].as_array().unwrap().iter().find(|m| m["name"] == name).unwrap().clone();
    let body = hex::decode(s(&m["body"])).unwrap();
    (tx::decode(&body, &auth).unwrap(), m["expect"].clone())
}

fn check(view: &tx::View, who: u32) -> Result<(), tx::TxError> {
    let w = Wallet::new(&[0u8; 32], "");
    let mut acct = w.account(who).unwrap();
    tx::check(view, &mut acct, "qorechain-local", Mode::Hybrid)
}

#[test]
fn a_multisig_is_read_completely() {
    let (v, e) = view("create-2of2");
    let Msg::GroupCreate { admin, members, policy, voting_secs, policy_as_admin } = &v.msgs[0] else { panic!("{:?}", v.msgs[0]) };
    assert_eq!(admin, s(&e["admin"]));
    assert_eq!(members.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), e["members"].as_array().unwrap().iter().map(s).collect::<Vec<_>>());
    assert!(members.iter().all(|m| m.1 == "1"));
    assert_eq!(policy, &GroupPolicy::Threshold("2".into()));
    assert_eq!(*voting_secs, 86400);
    assert!(!policy_as_admin);
    assert!(check(&v, 0).is_ok());
}

#[test]
fn only_the_admin_creates_the_multisig() {
    let (v, _) = view("create-2of2");
    assert!(check(&v, 1).is_err());
}

#[test]
fn the_device_creates_only_equal_weight_threshold_multisigs() {
    let (w, _) = view("create-weighted");
    assert!(matches!(&w.msgs[0], Msg::GroupCreate { .. }));
    assert!(check(&w, 0).is_err(), "unequal weights are refused");
    let (p, e) = view("create-percentage");
    let Msg::GroupCreate { policy, .. } = &p.msgs[0] else { panic!() };
    assert_eq!(policy, &GroupPolicy::Percentage(s(&e["percentage"]).into()));
    assert!(check(&p, 0).is_err(), "a percentage policy is refused");
}

#[test]
fn too_many_members_a_bad_threshold_and_a_duplicate_are_refused() {
    let (v, _) = view("create-2of2");
    let mut big = v.clone();
    if let Msg::GroupCreate { members, .. } = &mut big.msgs[0] {
        for i in 0..5 {
            members.push((format!("qor1extra{i}"), "1".into()));
        }
    }
    assert!(check(&big, 0).is_err());
    let mut high = v.clone();
    if let Msg::GroupCreate { policy, .. } = &mut high.msgs[0] {
        *policy = GroupPolicy::Threshold("3".into());
    }
    assert!(check(&high, 0).is_err(), "3 of 2");
    let mut zero = v.clone();
    if let Msg::GroupCreate { policy, .. } = &mut zero.msgs[0] {
        *policy = GroupPolicy::Threshold("0".into());
    }
    assert!(check(&zero, 0).is_err(), "0 of 2");
    let mut dup = v.clone();
    if let Msg::GroupCreate { members, .. } = &mut dup.msgs[0] {
        members[1] = members[0].clone();
    }
    assert!(check(&dup, 0).is_err(), "the same member twice");
}

#[test]
fn a_proposal_shows_what_the_multisig_would_do() {
    let (v, e) = view("propose-send");
    let Msg::GroupPropose { policy, proposers, title, inner, .. } = &v.msgs[0] else { panic!() };
    assert_eq!(policy, s(&e["policy"]));
    assert_eq!(proposers.len(), 1);
    assert_eq!(title, "Pay vendor");
    let Msg::Send { from, to, amount } = &inner[0] else { panic!("{:?}", inner[0]) };
    assert_eq!(from, s(&e["innerFrom"]));
    assert_eq!(to, s(&e["innerTo"]));
    assert_eq!(amount[0].amount, "1500000");
    assert!(check(&v, 0).is_ok());
    assert!(check(&v, 1).is_err(), "only the proposer's own account signs a proposal");
}

#[test]
fn a_proposal_of_two_actions_or_from_another_account_is_refused() {
    let (two, _) = view("propose-two-actions");
    assert!(check(&two, 0).is_err(), "the device shows one action per proposal");
    let (v, _) = view("propose-send");
    let mut wrong_from = v.clone();
    if let Msg::GroupPropose { inner, .. } = &mut wrong_from.msgs[0] {
        if let Msg::Send { from, .. } = &mut inner[0] {
            *from = "qor1someoneelse".into();
        }
    }
    assert!(check(&wrong_from, 0).is_err(), "the proposed action must be from the multisig's own account");
    let mut other_proposer = v.clone();
    if let Msg::GroupPropose { proposers, .. } = &mut other_proposer.msgs[0] {
        proposers.push("qor1another".into());
    }
    assert!(check(&other_proposer, 0).is_err(), "no extra proposers");
}

#[test]
fn votes_and_executions_are_this_accounts_own() {
    let (vote, e) = view("vote-yes");
    assert_eq!(vote.msgs[0], Msg::GroupVote { proposal_id: 7, voter: s(&e["voter"]).into(), option: 1 });
    assert!(check(&vote, 0).is_ok());
    let (other, _) = view("vote-for-someone-else");
    assert!(check(&other, 0).is_err(), "a vote cast for another member");
    let (exec, e) = view("exec");
    assert_eq!(exec.msgs[0], Msg::GroupExec { proposal_id: 7, executor: s(&e["executor"]).into() });
    assert!(check(&exec, 0).is_ok());
    assert!(check(&exec, 1).is_err());
    let mut bad_option = vote.clone();
    if let Msg::GroupVote { option, .. } = &mut bad_option.msgs[0] {
        *option = 9;
    }
    assert!(check(&bad_option, 0).is_err());
}

#[test]
fn group_messages_are_never_signed_classical_only() {
    let (v, _) = view("vote-yes");
    let w = Wallet::new(&[0u8; 32], "");
    let mut acct = w.account(0).unwrap();
    assert!(tx::check(&v, &mut acct, "qorechain-local", Mode::ClassicalOnly).is_err());
}
