// SPDX-License-Identifier: MIT
#![allow(deprecated)] // solana-sdk 2.3 deprecates its system_program / system_instruction re-exports
//! Integration tests: the escrow program runs natively (processor! mode) inside solana-program-test's
//! bank, and CPIs into the real SPL Token 3.5, Token-2022 8.0 and Associated Token Account 1.1.1
//! SBF binaries bundled with solana-program-test.

use solana_program_test::{processor, BanksClientError, ProgramTest, ProgramTestContext};
use solana_sdk::{
    account::Account,
    clock::Clock,
    instruction::{AccountMeta, Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction, system_program,
    transaction::{Transaction, TransactionError},
};
use truce_escrow::{
    error::EscrowError,
    instruction::{
        self as ix, associated_token_address, EscrowInstruction, TokenAccounts, ASSOCIATED_TOKEN_PROGRAM_ID,
        TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
    },
    state::{Deal, Status, MIN_DISPUTE_TIMEOUT},
};

const SOL: u64 = 1_000_000_000;
const HOUR: i64 = 3600;
const DAY: i64 = 86_400;

fn pid() -> Pubkey {
    // Placeholder: the program is not deployed anywhere.
    Pubkey::new_from_array(*b"TruceEscrow-placeholder-id-00000")
}

struct Env {
    ctx: ProgramTestContext,
    buyer: Keypair,
    seller: Keypair,
    stranger: Keypair,
}

async fn env() -> Env {
    let mut pt = ProgramTest::new("truce_escrow", pid(), processor!(truce_escrow::processor::process_instruction));
    pt.prefer_bpf(false);
    let buyer = Keypair::new();
    let seller = Keypair::new();
    let stranger = Keypair::new();
    for k in [&buyer, &seller, &stranger] {
        pt.add_account(k.pubkey(), Account { lamports: 100 * SOL, owner: system_program::ID, ..Account::default() });
    }
    let ctx = pt.start_with_context().await;
    Env { ctx, buyer, seller, stranger }
}

impl Env {
    async fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> Result<(), BanksClientError> {
        let bh = self.ctx.get_new_latest_blockhash().await.unwrap();
        let payer = self.ctx.payer.insecure_clone();
        // Sign only with the keys the message actually requires (extra keypairs are ignored).
        let msg = Message::new(ixs, Some(&payer.pubkey()));
        let required = &msg.account_keys[..msg.header.num_required_signatures as usize];
        let mut all: Vec<&Keypair> = vec![&payer];
        all.extend(signers.iter().copied().filter(|k| required.contains(&k.pubkey()) && k.pubkey() != payer.pubkey()));
        let mut tx = Transaction::new_unsigned(msg);
        tx.try_sign(&all, bh).expect("a required signer is missing from the test");
        self.ctx.banks_client.process_transaction(tx).await
    }

    async fn now(&mut self) -> i64 {
        self.ctx.banks_client.get_sysvar::<Clock>().await.unwrap().unix_timestamp
    }

    async fn warp(&mut self, secs: i64) {
        let mut c: Clock = self.ctx.banks_client.get_sysvar().await.unwrap();
        c.unix_timestamp += secs;
        self.ctx.set_sysvar(&c);
    }

    async fn lamports(&mut self, k: &Pubkey) -> u64 {
        self.ctx.banks_client.get_balance(*k).await.unwrap()
    }

    async fn account(&mut self, k: &Pubkey) -> Option<Account> {
        self.ctx.banks_client.get_account(*k).await.unwrap()
    }

    async fn token_balance(&mut self, k: &Pubkey) -> u64 {
        let a = self.account(k).await.expect("token account");
        u64::from_le_bytes(a.data[64..72].try_into().unwrap())
    }

    async fn deal(&mut self, k: &Pubkey) -> Deal {
        Deal::unpack(&self.account(k).await.expect("deal account").data).unwrap()
    }

    fn deal_addr(&self, id: u64) -> Pubkey {
        Deal::find_address(&pid(), &self.buyer.pubkey(), id).0
    }

    async fn open_sol(&mut self, id: u64, amount: u64, release_in: i64) -> Result<Pubkey, BanksClientError> {
        let t = self.now().await;
        let i = ix::open(&pid(), &self.buyer.pubkey(), &self.seller.pubkey(), id, amount, t + release_in, DAY, None);
        let b = self.buyer.insecure_clone();
        self.send(&[i], &[&b]).await?;
        Ok(self.deal_addr(id))
    }

    async fn payout(
        &mut self,
        e: EscrowInstruction,
        deal: &Pubkey,
        token: Option<TokenAccounts>,
        signers: &[&Keypair],
    ) -> Result<(), BanksClientError> {
        let i = ix::payout(&pid(), e, deal, &self.buyer.pubkey(), &self.seller.pubkey(), token);
        self.send(&[i], signers).await
    }

    // ---- token helpers (raw instructions, no spl crates needed) ----

    /// Creates a mint with the payer as mint authority. `fee_bps` adds a Token-2022 transfer fee.
    async fn create_mint(&mut self, token_program: Pubkey, decimals: u8, fee_bps: Option<u16>) -> Pubkey {
        let mint = Keypair::new();
        let payer = self.ctx.payer.pubkey();
        let space = if fee_bps.is_some() { 165 + 1 + 4 + 108 } else { 82 };
        let rent = self.ctx.banks_client.get_rent().await.unwrap().minimum_balance(space);
        let mut ixs = vec![system_instruction::create_account(&payer, &mint.pubkey(), rent, space as u64, &token_program)];
        if let Some(bps) = fee_bps {
            let mut d = vec![26u8, 0, 0, 0]; // TransferFeeExtension::InitializeTransferFeeConfig, no authorities
            d.extend_from_slice(&bps.to_le_bytes());
            d.extend_from_slice(&u64::MAX.to_le_bytes());
            ixs.push(Instruction { program_id: token_program, accounts: vec![AccountMeta::new(mint.pubkey(), false)], data: d });
        }
        let mut d = vec![20u8, decimals]; // InitializeMint2
        d.extend_from_slice(payer.as_ref());
        d.push(0); // no freeze authority
        ixs.push(Instruction { program_id: token_program, accounts: vec![AccountMeta::new(mint.pubkey(), false)], data: d });
        self.send(&ixs, &[&mint]).await.unwrap();
        mint.pubkey()
    }

    async fn create_ata(&mut self, owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
        let ata = associated_token_address(owner, mint, token_program);
        let i = Instruction {
            program_id: ASSOCIATED_TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(self.ctx.payer.pubkey(), true),
                AccountMeta::new(ata, false),
                AccountMeta::new_readonly(*owner, false),
                AccountMeta::new_readonly(*mint, false),
                AccountMeta::new_readonly(system_program::ID, false),
                AccountMeta::new_readonly(*token_program, false),
            ],
            data: vec![1],
        };
        self.send(&[i], &[]).await.unwrap();
        ata
    }

    async fn mint_to(&mut self, mint: &Pubkey, to: &Pubkey, amount: u64, token_program: &Pubkey) {
        let mut d = vec![7u8];
        d.extend_from_slice(&amount.to_le_bytes());
        let i = Instruction {
            program_id: *token_program,
            accounts: vec![
                AccountMeta::new(*mint, false),
                AccountMeta::new(*to, false),
                AccountMeta::new_readonly(self.ctx.payer.pubkey(), true),
            ],
            data: d,
        };
        self.send(&[i], &[]).await.unwrap();
    }

    /// Mint with buyer and seller token accounts; the buyer holds `fund` tokens.
    async fn token_setup(&mut self, token_program: Pubkey, fee_bps: Option<u16>, fund: u64) -> TokenAccounts {
        let mint = self.create_mint(token_program, 6, fee_bps).await;
        let (b, s) = (self.buyer.pubkey(), self.seller.pubkey());
        let buyer_token = self.create_ata(&b, &mint, &token_program).await;
        let seller_token = self.create_ata(&s, &mint, &token_program).await;
        self.mint_to(&mint, &buyer_token, fund, &token_program).await;
        TokenAccounts { mint, token_program, buyer_token, seller_token }
    }

    async fn open_token(&mut self, id: u64, amount: u64, t: TokenAccounts) -> Result<Pubkey, BanksClientError> {
        let now = self.now().await;
        let i = ix::open(&pid(), &self.buyer.pubkey(), &self.seller.pubkey(), id, amount, now + HOUR, DAY, Some(t));
        let b = self.buyer.insecure_clone();
        self.send(&[i], &[&b]).await?;
        Ok(self.deal_addr(id))
    }
}

fn code(r: Result<(), BanksClientError>) -> InstructionError {
    match r.expect_err("expected the transaction to fail").unwrap() {
        TransactionError::InstructionError(_, e) => e,
        other => panic!("unexpected error {other:?}"),
    }
}

fn custom(e: EscrowError) -> InstructionError {
    InstructionError::Custom(e as u32)
}

// ---------------------------------------------------------------- SOL

#[tokio::test]
async fn sol_open_and_release() {
    let mut e = env().await;
    let (b, s) = (e.buyer.insecure_clone(), e.seller.insecure_clone());
    let b0 = e.lamports(&b.pubkey()).await;
    let s0 = e.lamports(&s.pubkey()).await;
    let deal = e.open_sol(1, 2 * SOL, HOUR).await.unwrap();

    let d = e.deal(&deal).await;
    assert_eq!(d.status, Status::Funded);
    assert_eq!(d.amount, 2 * SOL);
    assert_eq!(d.seller, s.pubkey());
    let rent = e.lamports(&deal).await - 2 * SOL;
    assert_eq!(e.lamports(&b.pubkey()).await, b0 - 2 * SOL - rent);

    // Only the buyer can release: the seller sending it without the buyer's signature fails.
    let mut i = ix::payout(&pid(), EscrowInstruction::Release, &deal, &b.pubkey(), &s.pubkey(), None);
    i.accounts[1].is_signer = false;
    i.accounts[2].is_signer = true;
    assert_eq!(code(e.send(&[i], &[&s]).await), custom(EscrowError::NotParty));
    e.payout(EscrowInstruction::Release, &deal, None, &[&b]).await.unwrap();

    assert_eq!(e.lamports(&s.pubkey()).await, s0 + 2 * SOL);
    assert_eq!(e.lamports(&b.pubkey()).await, b0 - 2 * SOL, "buyer gets the deal account rent back");
    assert!(e.account(&deal).await.is_none(), "deal account closed");
}

#[tokio::test]
async fn sol_seller_refund() {
    let mut e = env().await;
    let (b, s) = (e.buyer.insecure_clone(), e.seller.insecure_clone());
    let b0 = e.lamports(&b.pubkey()).await;
    let deal = e.open_sol(2, SOL, HOUR).await.unwrap();
    let mut i = ix::payout(&pid(), EscrowInstruction::Refund, &deal, &b.pubkey(), &s.pubkey(), None);
    i.accounts[1].is_signer = true;
    i.accounts[2].is_signer = false;
    assert_eq!(code(e.send(&[i], &[&b]).await), custom(EscrowError::NotParty));
    e.payout(EscrowInstruction::Refund, &deal, None, &[&s]).await.unwrap();
    assert_eq!(e.lamports(&b.pubkey()).await, b0);
    assert!(e.account(&deal).await.is_none());
}

#[tokio::test]
async fn sol_claim_after_deadline_by_anyone() {
    let mut e = env().await;
    let s0 = e.lamports(&e.seller.pubkey()).await;
    let deal = e.open_sol(3, SOL, HOUR).await.unwrap();
    let anyone = e.stranger.insecure_clone();
    assert_eq!(code(e.payout(EscrowInstruction::Claim, &deal, None, &[&anyone]).await), custom(EscrowError::TooEarly));
    e.warp(HOUR).await;
    e.payout(EscrowInstruction::Claim, &deal, None, &[&anyone]).await.unwrap();
    assert_eq!(e.lamports(&e.seller.pubkey()).await, s0 + SOL);
    // Replay on the closed deal fails.
    assert_eq!(code(e.payout(EscrowInstruction::Claim, &deal, None, &[]).await), custom(EscrowError::BadAccount));
}

#[tokio::test]
async fn sol_dispute_blocks_claim_and_rules() {
    let mut e = env().await;
    let (b, s, x) = (e.buyer.insecure_clone(), e.seller.insecure_clone(), e.stranger.insecure_clone());
    let deal = e.open_sol(4, SOL, HOUR).await.unwrap();

    // A stranger cannot dispute.
    let r = e.send(&[ix::dispute(&pid(), &x.pubkey(), &deal)], &[&x]).await;
    assert_eq!(code(r), custom(EscrowError::NotParty));

    e.send(&[ix::dispute(&pid(), &s.pubkey(), &deal)], &[&s]).await.unwrap();
    let d = e.deal(&deal).await;
    assert_eq!(d.status, Status::Disputed);
    assert_eq!(d.disputed_at, e.now().await);

    // Cannot dispute twice; claim is blocked even after the deadline.
    let r = e.send(&[ix::dispute(&pid(), &b.pubkey(), &deal)], &[&b]).await;
    assert_eq!(code(r), custom(EscrowError::BadState));
    e.warp(2 * HOUR).await;
    assert_eq!(code(e.payout(EscrowInstruction::Claim, &deal, None, &[]).await), custom(EscrowError::BadState));

    // Release still works while disputed (buyer gives in).
    e.payout(EscrowInstruction::Release, &deal, None, &[&b]).await.unwrap();
}

#[tokio::test]
async fn sol_dispute_too_late() {
    let mut e = env().await;
    let b = e.buyer.insecure_clone();
    let deal = e.open_sol(5, SOL, HOUR).await.unwrap();
    e.warp(HOUR).await;
    let r = e.send(&[ix::dispute(&pid(), &b.pubkey(), &deal)], &[&b]).await;
    assert_eq!(code(r), custom(EscrowError::TooLate));
}

#[tokio::test]
async fn sol_expire_splits_evenly_after_timeout() {
    let mut e = env().await;
    let b = e.buyer.insecure_clone();
    let (b0, s0) = (e.lamports(&b.pubkey()).await, e.lamports(&e.seller.pubkey()).await);
    let amount = SOL + 1; // odd: the extra lamport goes to the buyer, as in the EVM version
    let deal = e.open_sol(6, amount, HOUR).await.unwrap();
    e.send(&[ix::dispute(&pid(), &b.pubkey(), &deal)], &[&b]).await.unwrap();

    assert_eq!(code(e.payout(EscrowInstruction::Expire, &deal, None, &[]).await), custom(EscrowError::TooEarly));
    e.warp(DAY - 10).await;
    assert_eq!(code(e.payout(EscrowInstruction::Expire, &deal, None, &[]).await), custom(EscrowError::TooEarly));
    e.warp(10).await;
    e.payout(EscrowInstruction::Expire, &deal, None, &[]).await.unwrap();

    assert_eq!(e.lamports(&e.seller.pubkey()).await, s0 + amount / 2);
    assert_eq!(e.lamports(&b.pubkey()).await, b0 - amount / 2);
    assert!(e.account(&deal).await.is_none());
}

#[tokio::test]
async fn sol_expire_needs_dispute() {
    let mut e = env().await;
    let deal = e.open_sol(7, SOL, HOUR).await.unwrap();
    e.warp(10 * DAY).await;
    assert_eq!(code(e.payout(EscrowInstruction::Expire, &deal, None, &[]).await), custom(EscrowError::BadState));
}

#[tokio::test]
async fn sol_settle_with_both_signatures() {
    let mut e = env().await;
    let (b, s) = (e.buyer.insecure_clone(), e.seller.insecure_clone());
    let (b0, s0) = (e.lamports(&b.pubkey()).await, e.lamports(&s.pubkey()).await);
    let deal = e.open_sol(8, 10 * SOL, HOUR).await.unwrap();
    e.send(&[ix::dispute(&pid(), &b.pubkey(), &deal)], &[&b]).await.unwrap();

    let digest = [0xabu8; 32];
    let split = |to_seller, to_buyer| EscrowInstruction::Settle { to_seller, to_buyer, agreement_digest: digest };

    // Bad split.
    assert_eq!(code(e.payout(split(7 * SOL, 2 * SOL), &deal, None, &[&b, &s]).await), custom(EscrowError::BadSplit));
    assert_eq!(code(e.payout(split(u64::MAX, 2), &deal, None, &[&b, &s]).await), custom(EscrowError::BadSplit));

    // Only one party signing: the seller's signature is missing.
    let mut one = ix::payout(&pid(), split(7 * SOL, 3 * SOL), &deal, &b.pubkey(), &s.pubkey(), None);
    one.accounts[2].is_signer = false;
    assert_eq!(code(e.send(&[one], &[&b]).await), custom(EscrowError::NotParty));

    // A stranger signing in the seller's place is not the seller.
    let wrong = Instruction {
        program_id: pid(),
        accounts: vec![
            AccountMeta::new(deal, false),
            AccountMeta::new(b.pubkey(), true),
            AccountMeta::new(e.stranger.pubkey(), true),
        ],
        data: split(7 * SOL, 3 * SOL).pack(),
    };
    let x = e.stranger.insecure_clone();
    assert_eq!(code(e.send(&[wrong], &[&b, &x]).await), custom(EscrowError::NotParty));

    // Both sign: paid out.
    e.payout(split(7 * SOL, 3 * SOL), &deal, None, &[&b, &s]).await.unwrap();
    assert_eq!(e.lamports(&s.pubkey()).await, s0 + 7 * SOL);
    assert_eq!(e.lamports(&b.pubkey()).await, b0 - 7 * SOL);

    // Replay: the deal is closed.
    assert_eq!(code(e.payout(split(7 * SOL, 3 * SOL), &deal, None, &[&b, &s]).await), custom(EscrowError::BadAccount));
}

#[tokio::test]
async fn sol_settle_without_dispute_and_full_to_one_side() {
    let mut e = env().await;
    let (b, s) = (e.buyer.insecure_clone(), e.seller.insecure_clone());
    let b0 = e.lamports(&b.pubkey()).await;
    let deal = e.open_sol(9, SOL, HOUR).await.unwrap();
    let i = EscrowInstruction::Settle { to_seller: 0, to_buyer: SOL, agreement_digest: [1u8; 32] };
    e.payout(i, &deal, None, &[&b, &s]).await.unwrap();
    assert_eq!(e.lamports(&b.pubkey()).await, b0);
}

#[tokio::test]
async fn sol_open_validation() {
    let mut e = env().await;
    let (b, s) = (e.buyer.insecure_clone(), e.seller.insecure_clone());
    let now = e.now().await;
    let mk = |seller: Pubkey, amount, release_at, timeout| {
        ix::open(&pid(), &b.pubkey(), &seller, 10, amount, release_at, timeout, None)
    };
    for (i, want) in [
        (mk(s.pubkey(), SOL, now + HOUR, MIN_DISPUTE_TIMEOUT - 1), EscrowError::BadState), // timeout < 1 day
        (mk(s.pubkey(), SOL, now, DAY), EscrowError::BadState),                            // release_at not in future
        (mk(b.pubkey(), SOL, now + HOUR, DAY), EscrowError::BadState),                     // seller == buyer
        (mk(Pubkey::default(), SOL, now + HOUR, DAY), EscrowError::BadState),              // no seller
        (mk(s.pubkey(), 0, now + HOUR, DAY), EscrowError::BadState),                       // zero amount
        (mk(s.pubkey(), SOL, now + HOUR, i64::MAX), EscrowError::BadState),                // overflow
    ] {
        assert_eq!(code(e.send(&[i], &[&b]).await), custom(want));
    }
    // Wrong deal address (not the PDA for this buyer and id).
    let mut i = mk(s.pubkey(), SOL, now + HOUR, DAY);
    i.accounts[2].pubkey = Deal::find_address(&pid(), &b.pubkey(), 11).0;
    assert_eq!(code(e.send(&[i], &[&b]).await), custom(EscrowError::BadAccount));

    // The same deal id cannot be opened twice while live.
    e.open_sol(12, SOL, HOUR).await.unwrap();
    assert_eq!(code(e.open_sol(12, SOL, HOUR).await.map(|_| ())), custom(EscrowError::BadState));
}

#[tokio::test]
async fn sol_open_works_on_prefunded_deal_address() {
    // Anyone can send lamports to a predictable PDA; that must not block opening the deal.
    let mut e = env().await;
    let deal = e.deal_addr(13);
    let x = e.stranger.insecure_clone();
    e.send(&[system_instruction::transfer(&x.pubkey(), &deal, 1_000_000)], &[&x]).await.unwrap();
    e.open_sol(13, SOL, HOUR).await.unwrap();
    assert_eq!(e.deal(&deal).await.amount, SOL);
}

#[tokio::test]
async fn sol_payout_rejects_wrong_recipient() {
    let mut e = env().await;
    let deal = e.open_sol(14, SOL, HOUR).await.unwrap();
    e.warp(HOUR).await;
    let mut i = ix::payout(&pid(), EscrowInstruction::Claim, &deal, &e.buyer.pubkey(), &e.seller.pubkey(), None);
    i.accounts[2].pubkey = e.stranger.pubkey();
    assert_eq!(code(e.send(&[i], &[]).await), custom(EscrowError::NotParty));
}

// ---------------------------------------------------------------- SPL Token / Token-2022

#[tokio::test]
async fn spl_token_open_and_settle_closes_vault() {
    let mut e = env().await;
    let (b, s) = (e.buyer.insecure_clone(), e.seller.insecure_clone());
    let t = e.token_setup(TOKEN_PROGRAM_ID, None, 1_000_000).await; // e.g. USDC, 6 decimals
    let b0 = e.lamports(&b.pubkey()).await;
    let deal = e.open_token(20, 600_000, t).await.unwrap();

    let d = e.deal(&deal).await;
    assert_eq!((d.amount, d.mint, d.token_program), (600_000, t.mint, TOKEN_PROGRAM_ID));
    let vault = associated_token_address(&deal, &t.mint, &TOKEN_PROGRAM_ID);
    assert_eq!(d.vault, vault);
    assert_eq!(e.token_balance(&vault).await, 600_000);
    assert_eq!(e.token_balance(&t.buyer_token).await, 400_000);

    e.send(&[ix::dispute(&pid(), &s.pubkey(), &deal)], &[&s]).await.unwrap();
    let i = EscrowInstruction::Settle { to_seller: 450_000, to_buyer: 150_000, agreement_digest: [9u8; 32] };
    e.payout(i, &deal, Some(t), &[&b, &s]).await.unwrap();

    assert_eq!(e.token_balance(&t.seller_token).await, 450_000);
    assert_eq!(e.token_balance(&t.buyer_token).await, 550_000);
    assert!(e.account(&vault).await.is_none(), "empty classic vault is closed");
    assert!(e.account(&deal).await.is_none());
    assert_eq!(e.lamports(&b.pubkey()).await, b0, "buyer gets both rents back");
}

#[tokio::test]
async fn spl_token_claim_rejects_redirected_token_account() {
    let mut e = env().await;
    let t = e.token_setup(TOKEN_PROGRAM_ID, None, 1_000).await;
    let deal = e.open_token(21, 1_000, t).await.unwrap();
    e.warp(HOUR).await;

    // Stranger calls claim but points the seller payout at their own token account.
    let x = e.stranger.insecure_clone();
    let theirs = e.create_ata(&x.pubkey(), &t.mint, &TOKEN_PROGRAM_ID).await;
    let bad = TokenAccounts { seller_token: theirs, ..t };
    assert_eq!(code(e.payout(EscrowInstruction::Claim, &deal, Some(bad), &[&x]).await), custom(EscrowError::BadAccount));

    // Wrong vault (the buyer's own account) is rejected too.
    let mut i = ix::payout(&pid(), EscrowInstruction::Claim, &deal, &e.buyer.pubkey(), &e.seller.pubkey(), Some(t));
    i.accounts[3].pubkey = t.buyer_token;
    assert_eq!(code(e.send(&[i], &[&x]).await), custom(EscrowError::BadAccount));

    e.payout(EscrowInstruction::Claim, &deal, Some(t), &[&x]).await.unwrap();
    assert_eq!(e.token_balance(&t.seller_token).await, 1_000);
}

#[tokio::test]
async fn spl_token_donation_to_vault_cannot_block_payout() {
    let mut e = env().await;
    let b = e.buyer.insecure_clone();
    let t = e.token_setup(TOKEN_PROGRAM_ID, None, 1_000).await;
    let deal = e.open_token(22, 500, t).await.unwrap();
    let vault = associated_token_address(&deal, &t.mint, &TOKEN_PROGRAM_ID);
    e.mint_to(&t.mint, &vault, 7, &TOKEN_PROGRAM_ID).await; // griefing donation
    e.payout(EscrowInstruction::Release, &deal, Some(t), &[&b]).await.unwrap();
    assert_eq!(e.token_balance(&t.seller_token).await, 500);
    assert_eq!(e.token_balance(&vault).await, 7, "vault left open, payout not blocked");
}

#[tokio::test]
async fn token_2022_open_expire() {
    let mut e = env().await;
    let b = e.buyer.insecure_clone();
    let t = e.token_setup(TOKEN_2022_PROGRAM_ID, None, 1_001).await;
    let deal = e.open_token(23, 1_001, t).await.unwrap();
    e.send(&[ix::dispute(&pid(), &b.pubkey(), &deal)], &[&b]).await.unwrap();
    e.warp(DAY).await;
    e.payout(EscrowInstruction::Expire, &deal, Some(t), &[]).await.unwrap();
    assert_eq!(e.token_balance(&t.seller_token).await, 500);
    assert_eq!(e.token_balance(&t.buyer_token).await, 501);
    assert!(e.account(&deal).await.is_none());
}

#[tokio::test]
async fn token_2022_transfer_fee_counts_what_arrived() {
    let mut e = env().await;
    let b = e.buyer.insecure_clone();
    let t = e.token_setup(TOKEN_2022_PROGRAM_ID, Some(100), 1_000_000).await; // 1% fee
    let deal = e.open_token(24, 1_000_000, t).await.unwrap();
    assert_eq!(e.deal(&deal).await.amount, 990_000, "deal holds what actually arrived");
    e.payout(EscrowInstruction::Release, &deal, Some(t), &[&b]).await.unwrap();
    assert_eq!(e.token_balance(&t.seller_token).await, 980_100, "seller pays the outbound fee too");
    assert!(e.account(&deal).await.is_none());
}

#[tokio::test]
async fn token_open_rejects_fake_token_program_and_wrong_vault() {
    let mut e = env().await;
    let b = e.buyer.insecure_clone();
    let t = e.token_setup(TOKEN_PROGRAM_ID, None, 1_000).await;
    let now = e.now().await;
    let base = ix::open(&pid(), &b.pubkey(), &e.seller.pubkey(), 25, 100, now + HOUR, DAY, Some(t));

    let mut i = base.clone();
    i.accounts[5].pubkey = t.buyer_token; // vault is not the deal PDA's ATA
    assert_eq!(code(e.send(&[i], &[&b]).await), custom(EscrowError::BadAccount));

    let mut i = base.clone();
    i.accounts[7].pubkey = system_program::ID; // not a token program
    assert_eq!(code(e.send(&[i], &[&b]).await), custom(EscrowError::BadAccount));

    let mut i = base;
    i.accounts[7].pubkey = TOKEN_2022_PROGRAM_ID; // mint not owned by the given token program
    assert_eq!(code(e.send(&[i], &[&b]).await), custom(EscrowError::BadAccount));
}
