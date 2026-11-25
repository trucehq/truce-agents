// SPDX-License-Identifier: MIT
use super::*;
use near_sdk::mock::MockAction;
use near_sdk::test_utils::{get_created_receipts, get_logs, VMContextBuilder};
use near_sdk::{testing_env, PromiseResult, RuntimeFeesConfig};

const T0: u64 = 1_800_000_000; // Unix seconds
const DAY: u64 = 86_400;
const NEAR: u128 = 1_000_000_000_000_000_000_000_000;
const USDC_UNIT: u128 = 1_000_000;

fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}
fn escrow() -> AccountId {
    acc("escrow.near")
}
fn buyer() -> AccountId {
    acc("buyer.near")
}
fn seller() -> AccountId {
    acc("seller.near")
}
fn eve() -> AccountId {
    acc("eve.near")
}
fn usdc() -> AccountId {
    acc("usdc.near")
}
fn usdt() -> AccountId {
    acc("usdt.near")
}
fn storage() -> u128 {
    DEAL_STORAGE_DEPOSIT.as_yoctonear()
}
const DIGEST: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
const DIGEST2: &str = "0000000000000000000000000000000000000000000000000000000000000001";

/// Sets the next call's caller, attached deposit (yocto) and time (Unix seconds).
fn ctx(pred: &AccountId, deposit: u128, secs: u64) {
    testing_env!(context(pred, deposit, secs));
}

fn context(pred: &AccountId, deposit: u128, secs: u64) -> near_sdk::VMContext {
    VMContextBuilder::new()
        .current_account_id(escrow())
        .predecessor_account_id(pred.clone())
        .signer_account_id(pred.clone())
        .attached_deposit(NearToken::from_yoctonear(deposit))
        .block_timestamp(secs * 1_000_000_000)
        .account_balance(NearToken::from_near(1_000))
        .build()
}

/// Runs the next call as the escrow's own callback, with the given result of the previous transfer.
fn ctx_callback(result: PromiseResult) {
    testing_env!(
        context(&escrow(), 0, T0),
        near_sdk::test_vm_config(),
        RuntimeFeesConfig::test(),
        Default::default(),
        vec![result],
    );
}

fn caps() -> Vec<CoinCap> {
    vec![
        CoinCap { coin: Coin::Native, max_amount: U128(100 * NEAR) },
        CoinCap { coin: Coin::Ft(usdc()), max_amount: U128(1_000 * USDC_UNIT) },
        CoinCap { coin: Coin::Ft(usdt()), max_amount: U128(500 * USDC_UNIT) },
    ]
}

fn setup() -> TruceEscrow {
    ctx(&escrow(), 0, T0);
    TruceEscrow::new(caps())
}

fn open_native(c: &mut TruceEscrow, amount: u128) -> u64 {
    ctx(&buyer(), amount + storage(), T0);
    c.open(seller(), Coin::Native, U128(amount), T0 + 7 * DAY, 2 * DAY)
}

fn open_ft(c: &mut TruceEscrow, token: &AccountId, amount: u128) -> u64 {
    ctx(&buyer(), storage(), T0);
    let id = c.open(seller(), Coin::Ft(token.clone()), U128(amount), T0 + 7 * DAY, 2 * DAY);
    assert_eq!(c.get_deal(id).unwrap().status, Status::AwaitingFunds);
    ctx(token, 0, T0 + 1);
    let unused = c.ft_on_transfer(buyer(), U128(amount), id.to_string());
    assert_eq!(unused.0, 0);
    assert_eq!(c.get_deal(id).unwrap().status, Status::Funded);
    id
}

/// Transfers created by the last call: (receiver, Native amount) or (token, ft receiver, amount).
#[derive(Debug, PartialEq)]
enum Pay {
    Near(AccountId, u128),
    Ft(AccountId, AccountId, u128),
}

fn payouts() -> Vec<Pay> {
    let mut out = Vec::new();
    for r in get_created_receipts() {
        for a in &r.actions {
            match a {
                MockAction::Transfer { deposit, .. } => out.push(Pay::Near(r.receiver_id.clone(), deposit.as_yoctonear())),
                MockAction::FunctionCallWeight { method_name, args, attached_deposit, .. }
                    if method_name == b"ft_transfer" =>
                {
                    assert_eq!(attached_deposit.as_yoctonear(), 1, "ft_transfer needs 1 yocto");
                    let v: serde_json::Value = serde_json::from_slice(args).unwrap();
                    out.push(Pay::Ft(
                        r.receiver_id.clone(),
                        v["receiver_id"].as_str().unwrap().parse().unwrap(),
                        v["amount"].as_str().unwrap().parse().unwrap(),
                    ));
                }
                MockAction::FunctionCallWeight { method_name, .. } => {
                    assert_eq!(method_name, b"on_payout", "unexpected call")
                }
                _ => {}
            }
        }
    }
    out
}

fn has_event(name: &str) -> bool {
    get_logs().iter().any(|l| l.starts_with("EVENT_JSON:") && l.contains(&format!("\"event\":\"{name}\"")))
}

fn status(c: &TruceEscrow, id: u64) -> Status {
    c.get_deal(id).unwrap().status
}

// ---------- init and caps ----------

#[test]
fn init_stores_caps() {
    let c = setup();
    assert_eq!(c.get_caps(), caps());
    assert_eq!(c.max_amount(Coin::Native).0, 100 * NEAR);
    assert_eq!(c.max_amount(Coin::Ft(usdt())).0, 500 * USDC_UNIT);
    assert_eq!(c.max_amount(Coin::Ft(eve())).0, 0);
    assert_eq!(c.deal_storage_deposit().0, storage());
}

#[test]
#[should_panic(expected = "Private method")]
fn init_only_by_contract_itself() {
    ctx(&eve(), 0, T0);
    TruceEscrow::new(caps());
}

#[test]
#[should_panic(expected = "no coins")]
fn init_rejects_empty() {
    ctx(&escrow(), 0, T0);
    TruceEscrow::new(vec![]);
}

#[test]
#[should_panic(expected = "zero cap")]
fn init_rejects_zero_cap() {
    ctx(&escrow(), 0, T0);
    TruceEscrow::new(vec![CoinCap { coin: Coin::Native, max_amount: U128(0) }]);
}

#[test]
#[should_panic(expected = "duplicate coin")]
fn init_rejects_duplicate() {
    ctx(&escrow(), 0, T0);
    TruceEscrow::new(vec![
        CoinCap { coin: Coin::Ft(usdc()), max_amount: U128(1) },
        CoinCap { coin: Coin::Ft(usdc()), max_amount: U128(2) },
    ]);
}

#[test]
fn native_at_cap_is_accepted() {
    let mut c = setup();
    let id = open_native(&mut c, 100 * NEAR);
    assert_eq!(c.get_deal(id).unwrap().amount.0, 100 * NEAR);
    assert!(has_event("opened"));
}

#[test]
#[should_panic(expected = "NotAccepted")]
fn native_over_cap_rejected() {
    let mut c = setup();
    open_native(&mut c, 100 * NEAR + 1);
}

#[test]
#[should_panic(expected = "NotAccepted")]
fn token_over_cap_rejected() {
    let mut c = setup();
    ctx(&buyer(), storage(), T0);
    c.open(seller(), Coin::Ft(usdt()), U128(500 * USDC_UNIT + 1), T0 + DAY, DAY);
}

#[test]
#[should_panic(expected = "NotAccepted")]
fn unlisted_token_rejected() {
    let mut c = setup();
    ctx(&buyer(), storage(), T0);
    c.open(seller(), Coin::Ft(eve()), U128(1), T0 + DAY, DAY);
}

#[test]
fn native_only_contract_rejects_tokens() {
    ctx(&escrow(), 0, T0);
    let mut c = TruceEscrow::new(vec![CoinCap { coin: Coin::Native, max_amount: U128(NEAR) }]);
    ctx(&usdc(), 0, T0);
    assert_eq!(c.ft_on_transfer(buyer(), U128(5), "1".into()).0, 5);
}

// ---------- open validation ----------

#[test]
#[should_panic(expected = "BadState: deposit")]
fn native_open_needs_exact_deposit() {
    let mut c = setup();
    ctx(&buyer(), NEAR, T0); // storage deposit missing
    c.open(seller(), Coin::Native, U128(NEAR), T0 + DAY, DAY);
}

#[test]
#[should_panic(expected = "BadState: deposit")]
fn token_open_needs_exact_storage_deposit() {
    let mut c = setup();
    ctx(&buyer(), storage() + 1, T0);
    c.open(seller(), Coin::Ft(usdc()), U128(1), T0 + DAY, DAY);
}

#[test]
#[should_panic(expected = "BadState: seller")]
fn open_rejects_self_as_seller() {
    let mut c = setup();
    ctx(&buyer(), NEAR + storage(), T0);
    c.open(buyer(), Coin::Native, U128(NEAR), T0 + DAY, DAY);
}

#[test]
#[should_panic(expected = "BadState: seller")]
fn open_rejects_escrow_as_seller() {
    let mut c = setup();
    ctx(&buyer(), NEAR + storage(), T0);
    c.open(escrow(), Coin::Native, U128(NEAR), T0 + DAY, DAY);
}

#[test]
#[should_panic(expected = "BadState: zero amount")]
fn open_rejects_zero_amount() {
    let mut c = setup();
    ctx(&buyer(), storage(), T0);
    c.open(seller(), Coin::Native, U128(0), T0 + DAY, DAY);
}

#[test]
#[should_panic(expected = "BadState: release_at")]
fn open_rejects_past_release() {
    let mut c = setup();
    ctx(&buyer(), NEAR + storage(), T0);
    c.open(seller(), Coin::Native, U128(NEAR), T0, DAY);
}

#[test]
#[should_panic(expected = "BadState: dispute_timeout")]
fn open_rejects_short_dispute_timeout() {
    let mut c = setup();
    ctx(&buyer(), NEAR + storage(), T0);
    c.open(seller(), Coin::Native, U128(NEAR), T0 + DAY, DAY - 1);
}

#[test]
#[should_panic(expected = "BadState: overflow")]
fn open_rejects_time_overflow() {
    let mut c = setup();
    ctx(&buyer(), NEAR + storage(), T0);
    c.open(seller(), Coin::Native, U128(NEAR), u64::MAX - 10, DAY);
}

#[test]
fn deal_ids_increment() {
    let mut c = setup();
    assert_eq!(open_native(&mut c, NEAR), 1);
    assert_eq!(open_native(&mut c, NEAR), 2);
    assert_eq!(c.next_id(), 3);
}

#[test]
fn worst_case_deal_storage_fits_deposit() {
    // 64-character account ids everywhere, a token deal, and both approvals stored.
    let long = |p: &str| acc(&format!("{p}{}", "x".repeat(64 - p.len())));
    let (b, s, t) = (long("b"), long("s"), long("t"));
    ctx(&escrow(), 0, T0);
    let mut c = TruceEscrow::new(vec![CoinCap { coin: Coin::Ft(t.clone()), max_amount: U128(u128::MAX) }]);
    // The mocked storage_usage restarts with each testing_env!, so measure each call on its own.
    let mut used = 0;
    let mut step = |f: &mut dyn FnMut(&mut TruceEscrow)| {
        let before = env::storage_usage();
        f(&mut c);
        c.deals.flush();
        used += env::storage_usage() - before;
    };
    let mut id = 0;
    ctx(&b, storage(), T0);
    step(&mut |c| id = c.open(s.clone(), Coin::Ft(t.clone()), U128(u128::MAX), T0 + DAY, DAY));
    ctx(&t, 0, T0);
    step(&mut |c| assert_eq!(c.ft_on_transfer(b.clone(), U128(u128::MAX), id.to_string()).0, 0));
    ctx(&b, 0, T0);
    step(&mut |c| c.approve_settlement(id, U128(u128::MAX), U128(0), DIGEST.into()));
    ctx(&s, 0, T0);
    step(&mut |c| c.approve_settlement(id, U128(u128::MAX - 1), U128(1), DIGEST.into()));
    println!("worst case deal storage: {used} bytes");
    assert!(used > 300 && used <= DEAL_STORAGE_BYTES / 2, "worst case deal uses {used} bytes");
}

// ---------- NEP-141 funding ----------

#[test]
fn token_deal_funded_by_ft_on_transfer() {
    let mut c = setup();
    let id = open_ft(&mut c, &usdc(), 250 * USDC_UNIT);
    let d = c.get_deal(id).unwrap();
    assert_eq!(d.coin, Coin::Ft(usdc()));
    assert_eq!(d.amount.0, 250 * USDC_UNIT);
}

#[test]
fn ft_excess_is_returned() {
    let mut c = setup();
    ctx(&buyer(), storage(), T0);
    let id = c.open(seller(), Coin::Ft(usdc()), U128(100), T0 + DAY, DAY);
    ctx(&usdc(), 0, T0);
    assert_eq!(c.ft_on_transfer(buyer(), U128(130), id.to_string()).0, 30);
    assert_eq!(status(&c, id), Status::Funded);
    assert_eq!(c.get_deal(id).unwrap().amount.0, 100);
}

#[test]
fn ft_short_amount_is_returned_in_full() {
    let mut c = setup();
    ctx(&buyer(), storage(), T0);
    let id = c.open(seller(), Coin::Ft(usdc()), U128(100), T0 + DAY, DAY);
    ctx(&usdc(), 0, T0);
    assert_eq!(c.ft_on_transfer(buyer(), U128(99), id.to_string()).0, 99);
    assert_eq!(status(&c, id), Status::AwaitingFunds);
}

#[test]
fn ft_from_unlisted_token_is_returned() {
    let mut c = setup();
    ctx(&buyer(), storage(), T0);
    let id = c.open(seller(), Coin::Ft(usdc()), U128(100), T0 + DAY, DAY);
    // A fake contract calls ft_on_transfer pretending to deliver USDC.
    ctx(&eve(), 0, T0);
    assert_eq!(c.ft_on_transfer(buyer(), U128(100), id.to_string()).0, 100);
    assert_eq!(status(&c, id), Status::AwaitingFunds);
}

#[test]
fn ft_wrong_listed_token_is_returned() {
    let mut c = setup();
    ctx(&buyer(), storage(), T0);
    let id = c.open(seller(), Coin::Ft(usdc()), U128(100), T0 + DAY, DAY);
    ctx(&usdt(), 0, T0); // accepted token, but not this deal's token
    assert_eq!(c.ft_on_transfer(buyer(), U128(100), id.to_string()).0, 100);
    assert_eq!(status(&c, id), Status::AwaitingFunds);
}

#[test]
fn ft_from_non_buyer_is_returned() {
    let mut c = setup();
    ctx(&buyer(), storage(), T0);
    let id = c.open(seller(), Coin::Ft(usdc()), U128(100), T0 + DAY, DAY);
    ctx(&usdc(), 0, T0);
    assert_eq!(c.ft_on_transfer(eve(), U128(100), id.to_string()).0, 100);
    assert_eq!(status(&c, id), Status::AwaitingFunds);
}

#[test]
fn ft_bad_msg_or_unknown_deal_is_returned() {
    let mut c = setup();
    ctx(&usdc(), 0, T0);
    assert_eq!(c.ft_on_transfer(buyer(), U128(7), "hello".into()).0, 7);
    assert_eq!(c.ft_on_transfer(buyer(), U128(7), "42".into()).0, 7);
}

#[test]
fn ft_for_native_deal_is_returned() {
    let mut c = setup();
    let id = open_native(&mut c, NEAR);
    ctx(&usdc(), 0, T0);
    assert_eq!(c.ft_on_transfer(buyer(), U128(7), id.to_string()).0, 7);
}

#[test]
fn ft_second_funding_is_returned() {
    let mut c = setup();
    let id = open_ft(&mut c, &usdc(), 100);
    ctx(&usdc(), 0, T0 + 2);
    assert_eq!(c.ft_on_transfer(buyer(), U128(100), id.to_string()).0, 100);
    assert_eq!(c.get_deal(id).unwrap().amount.0, 100);
}

#[test]
fn ft_after_release_at_is_returned() {
    let mut c = setup();
    ctx(&buyer(), storage(), T0);
    let id = c.open(seller(), Coin::Ft(usdc()), U128(100), T0 + DAY, DAY);
    ctx(&usdc(), 0, T0 + DAY);
    assert_eq!(c.ft_on_transfer(buyer(), U128(100), id.to_string()).0, 100);
    assert_eq!(status(&c, id), Status::AwaitingFunds);
}

#[test]
#[should_panic(expected = "BadState")]
fn unfunded_token_deal_cannot_be_released() {
    let mut c = setup();
    ctx(&buyer(), storage(), T0);
    let id = c.open(seller(), Coin::Ft(usdc()), U128(100), T0 + DAY, DAY);
    ctx(&buyer(), 0, T0);
    c.release(id);
}

// ---------- release / refund / claim ----------

#[test]
fn release_pays_seller_native() {
    let mut c = setup();
    let id = open_native(&mut c, 5 * NEAR);
    ctx(&buyer(), 0, T0 + 10);
    c.release(id);
    assert_eq!(payouts(), vec![Pay::Near(seller(), 5 * NEAR)]);
    assert_eq!(status(&c, id), Status::Closed);
    assert!(has_event("released"));
}

#[test]
fn release_pays_seller_token() {
    let mut c = setup();
    let id = open_ft(&mut c, &usdc(), 100 * USDC_UNIT);
    ctx(&buyer(), 0, T0 + 10);
    c.release(id);
    assert_eq!(payouts(), vec![Pay::Ft(usdc(), seller(), 100 * USDC_UNIT)]);
}

#[test]
#[should_panic(expected = "NotParty")]
fn release_only_by_buyer() {
    let mut c = setup();
    let id = open_native(&mut c, NEAR);
    ctx(&seller(), 0, T0);
    c.release(id);
}

#[test]
fn refund_pays_buyer_even_when_disputed() {
    let mut c = setup();
    let id = open_ft(&mut c, &usdt(), 40);
    ctx(&buyer(), 0, T0 + 5);
    c.dispute(id);
    ctx(&seller(), 0, T0 + 6);
    c.refund(id);
    assert_eq!(payouts(), vec![Pay::Ft(usdt(), buyer(), 40)]);
    assert!(has_event("refunded"));
}

#[test]
#[should_panic(expected = "NotParty")]
fn refund_only_by_seller() {
    let mut c = setup();
    let id = open_native(&mut c, NEAR);
    ctx(&buyer(), 0, T0);
    c.refund(id);
}

#[test]
fn claim_by_anyone_after_deadline() {
    let mut c = setup();
    let id = open_native(&mut c, 3 * NEAR);
    ctx(&eve(), 0, T0 + 7 * DAY);
    c.claim(id);
    assert_eq!(payouts(), vec![Pay::Near(seller(), 3 * NEAR)]);
}

#[test]
#[should_panic(expected = "TooEarly")]
fn claim_before_deadline_fails() {
    let mut c = setup();
    let id = open_native(&mut c, NEAR);
    ctx(&seller(), 0, T0 + 7 * DAY - 1);
    c.claim(id);
}

#[test]
#[should_panic(expected = "BadState")]
fn claim_blocked_by_dispute() {
    let mut c = setup();
    let id = open_native(&mut c, NEAR);
    ctx(&buyer(), 0, T0 + 1);
    c.dispute(id);
    ctx(&seller(), 0, T0 + 30 * DAY);
    c.claim(id);
}

#[test]
#[should_panic(expected = "BadState")]
fn no_double_payout_after_release() {
    let mut c = setup();
    let id = open_native(&mut c, NEAR);
    ctx(&buyer(), 0, T0);
    c.release(id);
    ctx(&eve(), 0, T0 + 7 * DAY);
    c.claim(id);
}

#[test]
#[should_panic(expected = "no such deal")]
fn unknown_deal() {
    let mut c = setup();
    ctx(&buyer(), 0, T0);
    c.release(99);
}

// ---------- dispute / expire ----------

#[test]
#[should_panic(expected = "NotParty")]
fn dispute_by_stranger_fails() {
    let mut c = setup();
    let id = open_native(&mut c, NEAR);
    ctx(&eve(), 0, T0 + 1);
    c.dispute(id);
}

#[test]
#[should_panic(expected = "TooLate")]
fn dispute_at_deadline_fails() {
    let mut c = setup();
    let id = open_native(&mut c, NEAR);
    ctx(&buyer(), 0, T0 + 7 * DAY);
    c.dispute(id);
}

#[test]
#[should_panic(expected = "BadState")]
fn dispute_twice_fails() {
    let mut c = setup();
    let id = open_native(&mut c, NEAR);
    ctx(&buyer(), 0, T0 + 1);
    c.dispute(id);
    ctx(&seller(), 0, T0 + 2);
    c.dispute(id);
}

#[test]
fn expire_splits_evenly_odd_unit_to_buyer() {
    let mut c = setup();
    let amount = 5 * NEAR + 1;
    let id = open_native(&mut c, amount);
    ctx(&seller(), 0, T0 + 100);
    c.dispute(id);
    assert_eq!(c.get_deal(id).unwrap().disputed_at, T0 + 100);
    ctx(&eve(), 0, T0 + 100 + 2 * DAY);
    c.expire(id);
    let half = amount / 2;
    assert_eq!(payouts(), vec![Pay::Near(seller(), half), Pay::Near(buyer(), amount - half)]);
    assert_eq!(amount - half, half + 1);
    assert!(has_event("expired"));
}

#[test]
fn expire_token_deal() {
    let mut c = setup();
    let id = open_ft(&mut c, &usdc(), 101);
    ctx(&buyer(), 0, T0 + 5);
    c.dispute(id);
    ctx(&eve(), 0, T0 + 5 + 2 * DAY);
    c.expire(id);
    assert_eq!(payouts(), vec![Pay::Ft(usdc(), seller(), 50), Pay::Ft(usdc(), buyer(), 51)]);
}

#[test]
#[should_panic(expected = "TooEarly")]
fn expire_before_timeout_fails() {
    let mut c = setup();
    let id = open_native(&mut c, NEAR);
    ctx(&seller(), 0, T0 + 100);
    c.dispute(id);
    ctx(&eve(), 0, T0 + 100 + 2 * DAY - 1);
    c.expire(id);
}

#[test]
#[should_panic(expected = "BadState")]
fn expire_without_dispute_fails() {
    let mut c = setup();
    let id = open_native(&mut c, NEAR);
    ctx(&eve(), 0, T0 + 30 * DAY);
    c.expire(id);
}

// ---------- settle ----------

#[test]
fn settle_when_both_approve_same_split() {
    let mut c = setup();
    let id = open_ft(&mut c, &usdc(), 100);
    ctx(&buyer(), 0, T0 + 1);
    c.dispute(id);
    ctx(&seller(), 0, T0 + 2);
    c.approve_settlement(id, U128(70), U128(30), DIGEST.into());
    assert!(payouts().is_empty());
    let d = c.get_deal(id).unwrap();
    assert_eq!(d.seller_approval.as_ref().unwrap().agreement_digest, DIGEST);
    assert!(d.buyer_approval.is_none());
    ctx(&buyer(), 0, T0 + 3);
    c.approve_settlement(id, U128(70), U128(30), DIGEST.to_uppercase());
    assert_eq!(payouts(), vec![Pay::Ft(usdc(), seller(), 70), Pay::Ft(usdc(), buyer(), 30)]);
    assert!(has_event("settled"));
    assert!(get_logs().iter().any(|l| l.contains(DIGEST) && l.contains("\"settled\"")));
    let d = c.get_deal(id).unwrap();
    assert_eq!(d.status, Status::Closed);
    assert!(d.buyer_approval.is_none() && d.seller_approval.is_none());
}

#[test]
fn settle_without_dispute_and_with_zero_side() {
    let mut c = setup();
    let id = open_native(&mut c, 4 * NEAR);
    ctx(&buyer(), 0, T0 + 1);
    c.approve_settlement(id, U128(0), U128(4 * NEAR), DIGEST.into());
    ctx(&seller(), 0, T0 + 1);
    c.approve_settlement(id, U128(0), U128(4 * NEAR), DIGEST.into());
    assert_eq!(payouts(), vec![Pay::Near(buyer(), 4 * NEAR)]);
}

#[test]
fn settle_mismatched_split_does_not_pay() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&buyer(), 0, T0 + 1);
    c.approve_settlement(id, U128(4), U128(6), DIGEST.into());
    ctx(&seller(), 0, T0 + 2);
    c.approve_settlement(id, U128(6), U128(4), DIGEST.into());
    assert!(payouts().is_empty());
    assert_eq!(status(&c, id), Status::Funded);
    // The seller changes its mind and matches the buyer: now it settles.
    ctx(&seller(), 0, T0 + 3);
    c.approve_settlement(id, U128(4), U128(6), DIGEST.into());
    assert_eq!(payouts(), vec![Pay::Near(seller(), 4), Pay::Near(buyer(), 6)]);
}

#[test]
fn settle_mismatched_digest_does_not_pay() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&buyer(), 0, T0 + 1);
    c.approve_settlement(id, U128(5), U128(5), DIGEST.into());
    ctx(&seller(), 0, T0 + 2);
    c.approve_settlement(id, U128(5), U128(5), DIGEST2.into());
    assert!(payouts().is_empty());
    assert_eq!(status(&c, id), Status::Funded);
}

#[test]
fn withdrawn_approval_blocks_settle() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&buyer(), 0, T0 + 1);
    c.approve_settlement(id, U128(5), U128(5), DIGEST.into());
    ctx(&buyer(), 0, T0 + 2);
    c.withdraw_approval(id);
    assert!(has_event("approval_withdrawn"));
    ctx(&seller(), 0, T0 + 3);
    c.approve_settlement(id, U128(5), U128(5), DIGEST.into());
    assert!(payouts().is_empty());
    assert_eq!(status(&c, id), Status::Funded);
    assert!(c.get_deal(id).unwrap().buyer_approval.is_none());
}

#[test]
#[should_panic(expected = "BadSplit")]
fn settle_split_must_total_amount() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&buyer(), 0, T0 + 1);
    c.approve_settlement(id, U128(5), U128(4), DIGEST.into());
}

#[test]
#[should_panic(expected = "BadSplit")]
fn settle_split_overflow_rejected() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&buyer(), 0, T0 + 1);
    c.approve_settlement(id, U128(u128::MAX), U128(11), DIGEST.into());
}

#[test]
#[should_panic(expected = "NotParty")]
fn settle_approval_by_stranger_fails() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&eve(), 0, T0 + 1);
    c.approve_settlement(id, U128(5), U128(5), DIGEST.into());
}

#[test]
#[should_panic(expected = "NotParty")]
fn withdraw_approval_by_stranger_fails() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&eve(), 0, T0 + 1);
    c.withdraw_approval(id);
}

#[test]
#[should_panic(expected = "BadDigest")]
fn settle_bad_digest_rejected() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&buyer(), 0, T0 + 1);
    c.approve_settlement(id, U128(5), U128(5), format!("sha256:{DIGEST}"));
}

#[test]
#[should_panic(expected = "BadState")]
fn settle_replay_after_close_fails() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&buyer(), 0, T0 + 1);
    c.approve_settlement(id, U128(5), U128(5), DIGEST.into());
    ctx(&seller(), 0, T0 + 2);
    c.approve_settlement(id, U128(5), U128(5), DIGEST.into());
    ctx(&seller(), 0, T0 + 3);
    c.approve_settlement(id, U128(5), U128(5), DIGEST.into());
}

#[test]
fn approvals_are_per_deal() {
    let mut c = setup();
    let a = open_native(&mut c, 10);
    let b = open_native(&mut c, 10);
    ctx(&buyer(), 0, T0 + 1);
    c.approve_settlement(a, U128(5), U128(5), DIGEST.into());
    ctx(&seller(), 0, T0 + 2);
    c.approve_settlement(b, U128(5), U128(5), DIGEST.into());
    assert!(payouts().is_empty());
    assert_eq!(status(&c, a), Status::Funded);
    assert_eq!(status(&c, b), Status::Funded);
}

#[test]
#[should_panic(expected = "BadState")]
fn settle_after_expire_fails() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&buyer(), 0, T0 + 1);
    c.approve_settlement(id, U128(10), U128(0), DIGEST.into());
    c.dispute(id);
    ctx(&eve(), 0, T0 + 1 + 2 * DAY);
    c.expire(id);
    ctx(&seller(), 0, T0 + 3 * DAY);
    c.approve_settlement(id, U128(10), U128(0), DIGEST.into());
}

// ---------- payout callbacks ----------

#[test]
fn failed_payout_is_restored_and_retried() {
    let mut c = setup();
    let id = open_ft(&mut c, &usdc(), 100);
    ctx(&buyer(), 0, T0 + 1);
    c.release(id);
    assert_eq!(c.get_deal(id).unwrap().owed_seller.0, 0);

    // Seller not registered with the token: ft_transfer fails, the callback restores the debt.
    ctx_callback(PromiseResult::Failed);
    assert!(!c.on_payout(id, true, U128(100)));
    assert!(has_event("payout_failed"));
    let d = c.get_deal(id).unwrap();
    assert_eq!(d.status, Status::Closed);
    assert_eq!(d.owed_seller.0, 100);

    // Anyone retries; it can only go to the seller.
    ctx(&eve(), 0, T0 + 2);
    c.retry_payout(id);
    assert_eq!(payouts(), vec![Pay::Ft(usdc(), seller(), 100)]);
    assert_eq!(c.get_deal(id).unwrap().owed_seller.0, 0);

    ctx_callback(PromiseResult::Successful(vec![]));
    assert!(c.on_payout(id, true, U128(100)));
    assert_eq!(c.get_deal(id).unwrap().owed_seller.0, 0);
}

#[test]
fn failed_native_payout_to_one_side_only() {
    let mut c = setup();
    let id = open_native(&mut c, 11);
    ctx(&seller(), 0, T0 + 1);
    c.dispute(id);
    ctx(&eve(), 0, T0 + 1 + 2 * DAY);
    c.expire(id);
    ctx_callback(PromiseResult::Successful(vec![]));
    c.on_payout(id, true, U128(5));
    ctx_callback(PromiseResult::Failed);
    c.on_payout(id, false, U128(6));
    let d = c.get_deal(id).unwrap();
    assert_eq!((d.owed_seller.0, d.owed_buyer.0), (0, 6));
    ctx(&buyer(), 0, T0 + 3 * DAY);
    c.retry_payout(id);
    assert_eq!(payouts(), vec![Pay::Near(buyer(), 6)]);
}

#[test]
#[should_panic(expected = "nothing owed")]
fn retry_with_nothing_owed_fails() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&buyer(), 0, T0);
    c.release(id);
    ctx(&eve(), 0, T0);
    c.retry_payout(id);
}

#[test]
#[should_panic(expected = "BadState")]
fn retry_on_open_deal_fails() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&eve(), 0, T0);
    c.retry_payout(id);
}

#[test]
#[should_panic(expected = "Private method")]
fn on_payout_is_private() {
    let mut c = setup();
    let id = open_native(&mut c, 10);
    ctx(&buyer(), 0, T0);
    c.release(id);
    testing_env!(
        context(&eve(), 0, T0),
        near_sdk::test_vm_config(),
        RuntimeFeesConfig::test(),
        Default::default(),
        vec![PromiseResult::Failed],
    );
    c.on_payout(id, true, U128(10));
}
