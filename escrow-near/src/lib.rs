// SPDX-License-Identifier: MIT
//! TruceEscrow for NEAR: minimal, non-custodial escrow for agent purchases with a neutral settlement step.
//!
//! The buyer locks NEAR or a NEP-141 token for a seller. Funds move only when:
//!   1. the buyer releases them to the seller, or the seller refunds the buyer; or
//!   2. the release deadline passes with no dispute, and anyone triggers the seller's claim; or
//!   3. buyer and seller both approve the same split and agreement digest (typically the outcome of a
//!      TruceHQ agreement); or
//!   4. a dispute is open and the dispute timeout passes with no agreed split: the funds are split
//!      evenly, so neither side gains by stalling.
//!
//! TruceHQ (or any mediator) never holds keys, never approves and never receives funds here.
//!
//! BETA: unaudited. The contract only accepts the coins listed at initialisation, each with a maximum
//! deal size. The list is immutable: there is no owner, no admin and no upgrade method.

use near_sdk::json_types::U128;
use near_sdk::serde_json::{self, json};
use near_sdk::store::LookupMap;
use near_sdk::{env, near, require, AccountId, Gas, NearToken, PanicOnDefault, Promise, PromiseError};

/// Minimum time a dispute stays open before `expire` may split the funds: 1 day, in seconds.
pub const MIN_DISPUTE_TIMEOUT: u64 = 86_400;
/// Storage deposit per deal: 0.01 NEAR pays for 1,000 bytes of state, well above a deal record's worst case.
/// It stays with the deal record, which is kept as a permanent on-chain receipt.
pub const DEAL_STORAGE_DEPOSIT: NearToken = NearToken::from_millinear(10);
/// Bytes covered by `DEAL_STORAGE_DEPOSIT` at 10^19 yoctoNEAR per byte.
pub const DEAL_STORAGE_BYTES: u64 = 1_000;

const GAS_FT_TRANSFER: Gas = Gas::from_tgas(15);
const GAS_ON_PAYOUT: Gas = Gas::from_tgas(10);
const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);
const NANOS_PER_SEC: u64 = 1_000_000_000;

/// A coin the escrow can hold.
#[near(serializers = [borsh, json])]
#[serde(rename_all = "snake_case")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Coin {
    /// Native NEAR, amounts in yoctoNEAR.
    Native,
    /// A NEP-141 fungible token, by its contract account id.
    Ft(AccountId),
}

/// One accepted coin and its maximum deal size, fixed at initialisation.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoinCap {
    pub coin: Coin,
    pub max_amount: U128,
}

#[near(serializers = [borsh, json])]
#[serde(rename_all = "snake_case")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Token deal created by `open`; waits for the buyer's `ft_transfer_call`.
    AwaitingFunds,
    Funded,
    Disputed,
    /// Paid out (or payout in progress / to retry, see `owed_*`).
    Closed,
}

/// A split one party approved.
#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settlement {
    pub to_seller: u128,
    pub to_buyer: u128,
    pub agreement_digest: [u8; 32],
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct Deal {
    pub buyer: AccountId,
    pub seller: AccountId,
    pub coin: Coin,
    pub amount: u128,
    /// Unix seconds. The seller may claim after this if nobody disputed.
    pub release_at: u64,
    /// Seconds a dispute may stay open before the funds are split evenly.
    pub dispute_timeout: u64,
    /// Unix seconds, 0 if never disputed.
    pub disputed_at: u64,
    pub status: Status,
    /// Payouts decided but not (yet) delivered: a failed transfer is added back here and can be retried.
    pub owed_seller: u128,
    pub owed_buyer: u128,
    pub buyer_approval: Option<Settlement>,
    pub seller_approval: Option<Settlement>,
}

#[near(serializers = [json])]
pub struct SettlementView {
    pub to_seller: U128,
    pub to_buyer: U128,
    pub agreement_digest: String,
}

#[near(serializers = [json])]
pub struct DealView {
    pub id: u64,
    pub buyer: AccountId,
    pub seller: AccountId,
    pub coin: Coin,
    pub amount: U128,
    pub release_at: u64,
    pub dispute_timeout: u64,
    pub disputed_at: u64,
    pub status: Status,
    pub owed_seller: U128,
    pub owed_buyer: U128,
    pub buyer_approval: Option<SettlementView>,
    pub seller_approval: Option<SettlementView>,
}

#[derive(near_sdk::BorshStorageKey)]
#[near(serializers = [borsh])]
enum StorageKey {
    Deals,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct TruceEscrow {
    caps: Vec<CoinCap>,
    next_id: u64,
    deals: LookupMap<u64, Deal>,
}

#[near]
impl TruceEscrow {
    /// Sets the accepted coins and their caps once. Only the contract account itself can call this,
    /// so deploy and init in one transaction, then delete the account's access keys (see README).
    #[init]
    pub fn new(caps: Vec<CoinCap>) -> Self {
        require_self();
        require!(!caps.is_empty(), "BadState: no coins");
        for (i, c) in caps.iter().enumerate() {
            require!(c.max_amount.0 > 0, "BadState: zero cap");
            require!(!caps[..i].iter().any(|p| p.coin == c.coin), "BadState: duplicate coin");
            if let Coin::Ft(t) = &c.coin {
                require!(*t != env::current_account_id(), "BadState: self as token");
            }
        }
        Self { caps, next_id: 1, deals: LookupMap::new(StorageKey::Deals) }
    }

    /// Buyer opens a deal. Native NEAR: attach `amount + DEAL_STORAGE_DEPOSIT`; the deal is funded at once.
    /// Token: attach exactly `DEAL_STORAGE_DEPOSIT`, then fund with `ft_transfer_call` (msg = the deal id).
    #[payable]
    pub fn open(
        &mut self,
        seller: AccountId,
        coin: Coin,
        amount: U128,
        release_at: u64,
        dispute_timeout: u64,
    ) -> u64 {
        let buyer = env::predecessor_account_id();
        let amount = amount.0;
        require!(seller != buyer && seller != env::current_account_id(), "BadState: seller");
        require!(amount > 0, "BadState: zero amount");
        require!(release_at > now(), "BadState: release_at");
        require!(dispute_timeout >= MIN_DISPUTE_TIMEOUT, "BadState: dispute_timeout");
        require!(release_at.checked_add(dispute_timeout).is_some(), "BadState: overflow");
        let cap = self.cap(&coin);
        require!(cap > 0 && amount <= cap, "NotAccepted");

        let attached = env::attached_deposit().as_yoctonear();
        let storage = DEAL_STORAGE_DEPOSIT.as_yoctonear();
        let status = match coin {
            Coin::Native => {
                require!(Some(attached) == amount.checked_add(storage), "BadState: deposit");
                Status::Funded
            }
            Coin::Ft(_) => {
                require!(attached == storage, "BadState: deposit");
                Status::AwaitingFunds
            }
        };

        let before = env::storage_usage();
        let id = self.next_id;
        self.next_id += 1;
        let deal = Deal {
            buyer,
            seller,
            coin,
            amount,
            release_at,
            dispute_timeout,
            disputed_at: 0,
            status,
            owed_seller: 0,
            owed_buyer: 0,
            buyer_approval: None,
            seller_approval: None,
        };
        if status == Status::Funded {
            emit_opened(id, &deal);
        }
        self.deals.insert(id, deal);
        self.deals.flush();
        // The worst case with both approvals is about 520 bytes; the deposit covers 1,000.
        require!(env::storage_usage() - before <= DEAL_STORAGE_BYTES / 2, "BadState: storage");
        id
    }

    /// NEP-141 receiver. Funds a token deal created by `open`. Returns the amount the token contract must
    /// give back to the sender: everything if rejected, the excess if more than the deal amount arrived.
    pub fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> U128 {
        let token = env::predecessor_account_id();
        let amount = amount.0;
        let reject = |why: &str| {
            env::log_str(&format!("ft_on_transfer rejected: {why}"));
            U128(amount)
        };
        if self.cap(&Coin::Ft(token.clone())) == 0 {
            return reject("token not accepted");
        }
        let Ok(id) = msg.trim().parse::<u64>() else {
            return reject("msg must be the deal id");
        };
        let Some(deal) = self.deals.get_mut(&id) else {
            return reject("no such deal");
        };
        if deal.status != Status::AwaitingFunds {
            return reject("deal not awaiting funds");
        }
        if deal.coin != Coin::Ft(token) {
            return reject("wrong token for this deal");
        }
        if deal.buyer != sender_id {
            return reject("sender is not the buyer");
        }
        if amount < deal.amount {
            return reject("amount too small");
        }
        if now() >= deal.release_at {
            return reject("release_at passed");
        }
        deal.status = Status::Funded;
        let unused = amount - deal.amount;
        emit_opened(id, deal);
        U128(unused)
    }

    /// Either party opens a dispute before the release deadline. Funds stay locked.
    pub fn dispute(&mut self, id: u64) {
        let caller = env::predecessor_account_id();
        let t = now();
        let d = self.deal_mut(id);
        require!(d.status == Status::Funded, "BadState");
        require!(caller == d.buyer || caller == d.seller, "NotParty");
        require!(t < d.release_at, "TooLate");
        d.status = Status::Disputed;
        d.disputed_at = t;
        emit("disputed", json!({ "id": id, "by": caller }));
    }

    /// Buyer releases everything to the seller (happy path).
    pub fn release(&mut self, id: u64) {
        let d = self.deal_mut(id);
        require!(is_open(d.status), "BadState");
        require!(env::predecessor_account_id() == d.buyer, "NotParty");
        let a = d.amount;
        self.close(id, a, 0);
        emit("released", json!({ "id": id, "to_seller": U128(a) }));
    }

    /// Seller refunds everything to the buyer.
    pub fn refund(&mut self, id: u64) {
        let d = self.deal_mut(id);
        require!(is_open(d.status), "BadState");
        require!(env::predecessor_account_id() == d.seller, "NotParty");
        let a = d.amount;
        self.close(id, 0, a);
        emit("refunded", json!({ "id": id, "to_buyer": U128(a) }));
    }

    /// Anyone may pay the seller after the release deadline when nobody disputed.
    pub fn claim(&mut self, id: u64) {
        let t = now();
        let d = self.deal_mut(id);
        require!(d.status == Status::Funded, "BadState");
        require!(t >= d.release_at, "TooEarly");
        let a = d.amount;
        self.close(id, a, 0);
        emit("released", json!({ "id": id, "to_seller": U128(a) }));
    }

    /// Once a dispute has run past its timeout unsettled, anyone may split the funds evenly
    /// (the odd yocto unit goes to the buyer).
    pub fn expire(&mut self, id: u64) {
        let t = now();
        let d = self.deal_mut(id);
        require!(d.status == Status::Disputed, "BadState");
        // Cannot overflow: open() checked release_at + dispute_timeout, and disputed_at < release_at.
        require!(t >= d.disputed_at + d.dispute_timeout, "TooEarly");
        let to_seller = d.amount / 2;
        let to_buyer = d.amount - to_seller;
        self.close(id, to_seller, to_buyer);
        emit("expired", json!({ "id": id, "to_seller": U128(to_seller), "to_buyer": U128(to_buyer) }));
    }

    /// Buyer or seller approves a split. `agreement_digest` is 64 hex characters (e.g. the TruceHQ
    /// agreement_digest without its `sha256:` prefix). When the other party has approved exactly the same
    /// split and digest, the funds are paid out in this call. Calling again replaces the caller's approval.
    pub fn approve_settlement(&mut self, id: u64, to_seller: U128, to_buyer: U128, agreement_digest: String) {
        let caller = env::predecessor_account_id();
        let digest = parse_digest(&agreement_digest);
        let d = self.deal_mut(id);
        require!(is_open(d.status), "BadState");
        require!(to_seller.0.checked_add(to_buyer.0) == Some(d.amount), "BadSplit");
        let s = Settlement { to_seller: to_seller.0, to_buyer: to_buyer.0, agreement_digest: digest };
        let is_buyer = caller == d.buyer;
        if is_buyer {
            d.buyer_approval = Some(s);
        } else if caller == d.seller {
            d.seller_approval = Some(s);
        } else {
            env::panic_str("NotParty");
        }
        emit(
            "settlement_approved",
            json!({ "id": id, "by": caller, "to_seller": to_seller, "to_buyer": to_buyer, "agreement_digest": agreement_digest.to_ascii_lowercase() }),
        );
        if d.buyer_approval.is_some() && d.buyer_approval == d.seller_approval {
            self.close(id, to_seller.0, to_buyer.0);
            emit(
                "settled",
                json!({ "id": id, "to_seller": to_seller, "to_buyer": to_buyer, "agreement_digest": hex(&digest) }),
            );
        }
    }

    /// Buyer or seller withdraws its own pending approval.
    pub fn withdraw_approval(&mut self, id: u64) {
        let caller = env::predecessor_account_id();
        let d = self.deal_mut(id);
        require!(is_open(d.status), "BadState");
        if caller == d.buyer {
            d.buyer_approval = None;
        } else if caller == d.seller {
            d.seller_approval = None;
        } else {
            env::panic_str("NotParty");
        }
        emit("approval_withdrawn", json!({ "id": id, "by": caller }));
    }

    /// Anyone may retry a payout that failed (for example because the recipient was not registered with
    /// the token contract). The recipients are fixed in the deal, so nobody can redirect the funds.
    pub fn retry_payout(&mut self, id: u64) {
        let d = self.deal_mut(id);
        require!(d.status == Status::Closed, "BadState");
        require!(d.owed_seller > 0 || d.owed_buyer > 0, "BadState: nothing owed");
        self.pay_out(id);
    }

    /// Callback after each payout transfer. On failure the amount is owed again and can be retried.
    pub fn on_payout(&mut self, id: u64, to_seller: bool, amount: U128) -> bool {
        require_self();
        // An empty or a too-long return value both mean the transfer succeeded; only a failure matters.
        let ok = !matches!(env::promise_result_checked(0, 0), Err(PromiseError::Failed));
        if !ok {
            let d = self.deal_mut(id);
            if to_seller {
                d.owed_seller += amount.0;
            } else {
                d.owed_buyer += amount.0;
            }
            emit("payout_failed", json!({ "id": id, "to_seller": to_seller, "amount": amount }));
        }
        ok
    }

    // ---- views ----

    pub fn get_deal(&self, id: u64) -> Option<DealView> {
        self.deals.get(&id).map(|d| DealView {
            id,
            buyer: d.buyer.clone(),
            seller: d.seller.clone(),
            coin: d.coin.clone(),
            amount: U128(d.amount),
            release_at: d.release_at,
            dispute_timeout: d.dispute_timeout,
            disputed_at: d.disputed_at,
            status: d.status,
            owed_seller: U128(d.owed_seller),
            owed_buyer: U128(d.owed_buyer),
            buyer_approval: d.buyer_approval.as_ref().map(view_settlement),
            seller_approval: d.seller_approval.as_ref().map(view_settlement),
        })
    }

    /// Accepted coins and the maximum deal size for each.
    pub fn get_caps(&self) -> Vec<CoinCap> {
        self.caps.clone()
    }

    /// Maximum deal size for a coin; 0 means the coin is not accepted.
    pub fn max_amount(&self, coin: Coin) -> U128 {
        U128(self.cap(&coin))
    }

    pub fn deal_storage_deposit(&self) -> U128 {
        U128(DEAL_STORAGE_DEPOSIT.as_yoctonear())
    }

    /// Id the next `open` will get.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }
}

impl TruceEscrow {
    fn cap(&self, coin: &Coin) -> u128 {
        self.caps.iter().find(|c| c.coin == *coin).map_or(0, |c| c.max_amount.0)
    }

    fn deal_mut(&mut self, id: u64) -> &mut Deal {
        self.deals.get_mut(&id).unwrap_or_else(|| env::panic_str("BadState: no such deal"))
    }

    fn close(&mut self, id: u64, to_seller: u128, to_buyer: u128) {
        let d = self.deal_mut(id);
        d.status = Status::Closed;
        d.buyer_approval = None;
        d.seller_approval = None;
        d.owed_seller = to_seller;
        d.owed_buyer = to_buyer;
        self.pay_out(id);
    }

    /// Sends every owed amount. The owed amount is cleared before the transfer goes out, so a concurrent
    /// retry cannot pay twice; `on_payout` adds it back if the transfer fails.
    fn pay_out(&mut self, id: u64) {
        let d = self.deal_mut(id);
        let coin = d.coin.clone();
        let mut sends = Vec::new();
        if d.owed_seller > 0 {
            sends.push((true, d.seller.clone(), d.owed_seller));
            d.owed_seller = 0;
        }
        if d.owed_buyer > 0 {
            sends.push((false, d.buyer.clone(), d.owed_buyer));
            d.owed_buyer = 0;
        }
        for (to_seller, to, amount) in sends {
            let transfer = match &coin {
                Coin::Native => Promise::new(to).transfer(NearToken::from_yoctonear(amount)),
                Coin::Ft(token) => Promise::new(token.clone()).function_call(
                    "ft_transfer".to_string(),
                    serde_json::to_vec(&json!({
                        "receiver_id": to,
                        "amount": U128(amount),
                        "memo": format!("truce-escrow deal {id}"),
                    }))
                    .unwrap(),
                    ONE_YOCTO,
                    GAS_FT_TRANSFER,
                ),
            };
            transfer
                .then(Self::ext(env::current_account_id()).with_static_gas(GAS_ON_PAYOUT).on_payout(
                    id,
                    to_seller,
                    U128(amount),
                ))
                .detach();
        }
    }
}

/// Same as near-sdk's `#[private]`, written out so unit tests exercise it.
fn require_self() {
    require!(env::predecessor_account_id() == env::current_account_id(), "Private method");
}

fn now() -> u64 {
    env::block_timestamp() / NANOS_PER_SEC
}

fn is_open(s: Status) -> bool {
    s == Status::Funded || s == Status::Disputed
}

fn parse_digest(s: &str) -> [u8; 32] {
    let b = s.as_bytes();
    require!(b.len() == 64, "BadDigest: need 64 hex characters");
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = (b[2 * i] as char).to_digit(16);
        let lo = (b[2 * i + 1] as char).to_digit(16);
        match (hi, lo) {
            (Some(h), Some(l)) => out[i] = (h * 16 + l) as u8,
            _ => env::panic_str("BadDigest: need 64 hex characters"),
        }
    }
    out
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn view_settlement(s: &Settlement) -> SettlementView {
    SettlementView {
        to_seller: U128(s.to_seller),
        to_buyer: U128(s.to_buyer),
        agreement_digest: hex(&s.agreement_digest),
    }
}

/// NEP-297 event log.
fn emit(event: &str, data: serde_json::Value) {
    env::log_str(&format!(
        "EVENT_JSON:{}",
        json!({ "standard": "truce_escrow", "version": "1.0.0", "event": event, "data": [data] })
    ));
}

fn emit_opened(id: u64, d: &Deal) {
    emit(
        "opened",
        json!({
            "id": id, "buyer": d.buyer, "seller": d.seller, "coin": d.coin,
            "amount": U128(d.amount), "release_at": d.release_at,
        }),
    );
}

#[cfg(test)]
mod tests;
