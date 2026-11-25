// SPDX-License-Identifier: MIT
//! Instruction handlers.

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    log::sol_log_data,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction, system_program,
    sysvar::Sysvar,
};

use crate::{
    error::EscrowError,
    instruction::{
        associated_token_address, EscrowInstruction, ASSOCIATED_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
    },
    state::{Deal, Status, DEAL_SEED, MIN_DISPUTE_TIMEOUT},
};

pub fn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match EscrowInstruction::unpack(data)? {
        EscrowInstruction::Open { deal_id, amount, release_at, dispute_timeout } => {
            open(program_id, accounts, deal_id, amount, release_at, dispute_timeout)
        }
        EscrowInstruction::Dispute => dispute(program_id, accounts),
        EscrowInstruction::Release => {
            let (mut p, d) = Payout::load(program_id, accounts)?;
            require_signer(p.buyer)?;
            p.close(d.amount, 0, b"Released", None)
        }
        EscrowInstruction::Refund => {
            let (mut p, d) = Payout::load(program_id, accounts)?;
            require_signer(p.seller)?;
            p.close(0, d.amount, b"Refunded", None)
        }
        EscrowInstruction::Claim => {
            let (mut p, d) = Payout::load(program_id, accounts)?;
            if d.status != Status::Funded {
                return Err(EscrowError::BadState.into());
            }
            if now()? < d.release_at {
                return Err(EscrowError::TooEarly.into());
            }
            p.close(d.amount, 0, b"Released", None)
        }
        EscrowInstruction::Expire => {
            let (mut p, d) = Payout::load(program_id, accounts)?;
            if d.status != Status::Disputed {
                return Err(EscrowError::BadState.into());
            }
            // Cannot overflow: open() checks release_at + dispute_timeout fits and disputed_at < release_at.
            let ends = d.disputed_at.checked_add(d.dispute_timeout).ok_or(EscrowError::Overflow)?;
            if now()? < ends {
                return Err(EscrowError::TooEarly.into());
            }
            let to_seller = d.amount / 2;
            let to_buyer = d.amount - to_seller;
            p.close(to_seller, to_buyer, b"Expired", None)
        }
        EscrowInstruction::Settle { to_seller, to_buyer, agreement_digest } => {
            let (mut p, d) = Payout::load(program_id, accounts)?;
            // Both parties sign this very transaction, which carries the split and the digest.
            // The runtime has verified both ed25519 signatures over the whole message before we run.
            require_signer(p.buyer)?;
            require_signer(p.seller)?;
            if to_seller.checked_add(to_buyer) != Some(d.amount) {
                return Err(EscrowError::BadSplit.into());
            }
            p.close(to_seller, to_buyer, b"Settled", Some(&agreement_digest))
        }
    }
}

fn now() -> Result<i64, ProgramError> {
    Ok(Clock::get()?.unix_timestamp)
}

fn require_signer(a: &AccountInfo) -> ProgramResult {
    if a.is_signer {
        Ok(())
    } else {
        Err(EscrowError::NotParty.into())
    }
}

fn is_token_program(k: &Pubkey) -> bool {
    *k == TOKEN_PROGRAM_ID || *k == TOKEN_2022_PROGRAM_ID
}

// SPL token account layout (same base layout in Token and Token-2022): mint 0..32, owner 32..64, amount 64..72.
fn token_account_fields(acc: &AccountInfo, token_program: &Pubkey) -> Result<(Pubkey, Pubkey, u64), ProgramError> {
    if acc.owner != token_program {
        return Err(EscrowError::BadAccount.into());
    }
    let d = acc.try_borrow_data()?;
    if d.len() < 165 {
        return Err(EscrowError::BadAccount.into());
    }
    Ok((
        Pubkey::new_from_array(d[0..32].try_into().unwrap()),
        Pubkey::new_from_array(d[32..64].try_into().unwrap()),
        u64::from_le_bytes(d[64..72].try_into().unwrap()),
    ))
}

// Mint layout: decimals at byte 44, is_initialized at byte 45.
fn mint_decimals(mint: &AccountInfo, token_program: &Pubkey) -> Result<u8, ProgramError> {
    if mint.owner != token_program {
        return Err(EscrowError::BadAccount.into());
    }
    let d = mint.try_borrow_data()?;
    if d.len() < 82 || d[45] != 1 {
        return Err(EscrowError::BadAccount.into());
    }
    Ok(d[44])
}

fn transfer_checked_ix(
    token_program: &Pubkey,
    from: &Pubkey,
    mint: &Pubkey,
    to: &Pubkey,
    authority: &Pubkey,
    amount: u64,
    decimals: u8,
) -> Instruction {
    let mut data = vec![12u8]; // TransferChecked
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(decimals);
    Instruction {
        program_id: *token_program,
        accounts: vec![
            AccountMeta::new(*from, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(*to, false),
            AccountMeta::new_readonly(*authority, true),
        ],
        data,
    }
}

/// Beta caps, like the EVM contract: this program is unaudited, so each deal is small.
/// SOL: at most 5 SOL. Tokens: only USDC and USDT (classic SPL Token), at most 1,000 each.
pub const MAX_SOL_LAMPORTS: u64 = 5_000_000_000;
pub const MAX_STABLE_UNITS: u64 = 1_000_000_000; // 1,000 with 6 decimals
pub const USDC_MINT: Pubkey = solana_program::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
pub const USDT_MINT: Pubkey = solana_program::pubkey!("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB");

fn check_cap(mint: Option<&Pubkey>, amount: u64) -> ProgramResult {
    let ok = match mint {
        None => amount <= MAX_SOL_LAMPORTS,
        Some(m) if cfg!(feature = "any-mint") => {
            let _ = m;
            amount <= MAX_STABLE_UNITS
        }
        Some(m) => (*m == USDC_MINT || *m == USDT_MINT) && amount <= MAX_STABLE_UNITS,
    };
    if ok {
        Ok(())
    } else {
        Err(EscrowError::Capped.into())
    }
}

fn open(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    deal_id: u64,
    amount: u64,
    release_at: i64,
    dispute_timeout: i64,
) -> ProgramResult {
    let it = &mut accounts.iter();
    let buyer = next_account_info(it)?;
    let seller = next_account_info(it)?;
    let deal_ai = next_account_info(it)?;
    let system = next_account_info(it)?;

    if !buyer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if *system.key != system_program::ID {
        return Err(EscrowError::BadAccount.into());
    }
    if *seller.key == Pubkey::default() || seller.key == buyer.key || seller.key == deal_ai.key || amount == 0 {
        return Err(EscrowError::BadState.into());
    }
    if release_at <= now()? || dispute_timeout < MIN_DISPUTE_TIMEOUT || release_at.checked_add(dispute_timeout).is_none()
    {
        return Err(EscrowError::BadState.into());
    }

    let (expected, bump) = Deal::find_address(program_id, buyer.key, deal_id);
    if expected != *deal_ai.key {
        return Err(EscrowError::BadAccount.into());
    }
    let id_le = deal_id.to_le_bytes();
    let signer_seeds: &[&[u8]] = &[DEAL_SEED, buyer.key.as_ref(), &id_le, &[bump]];
    create_deal_account(program_id, buyer, deal_ai, system, signer_seeds)?;

    let mut deal = Deal {
        status: Status::Funded,
        bump,
        buyer: *buyer.key,
        seller: *seller.key,
        mint: Pubkey::default(),
        token_program: Pubkey::default(),
        vault: Pubkey::default(),
        deal_id,
        amount,
        release_at,
        dispute_timeout,
        disputed_at: 0,
    };

    if it.len() == 0 {
        check_cap(None, amount)?;
        // SOL: the lamports sit in the deal account itself, on top of its rent.
        invoke(
            &system_instruction::transfer(buyer.key, deal_ai.key, amount),
            &[buyer.clone(), deal_ai.clone(), system.clone()],
        )?;
    } else {
        let mint = next_account_info(it)?;
        let vault = next_account_info(it)?;
        let buyer_token = next_account_info(it)?;
        let token_program = next_account_info(it)?;
        let ata_program = next_account_info(it)?;
        if !is_token_program(token_program.key) || *ata_program.key != ASSOCIATED_TOKEN_PROGRAM_ID {
            return Err(EscrowError::BadAccount.into());
        }
        check_cap(Some(mint.key), amount)?;
        let decimals = mint_decimals(mint, token_program.key)?;
        if *vault.key != associated_token_address(deal_ai.key, mint.key, token_program.key) {
            return Err(EscrowError::BadAccount.into());
        }
        // Create the vault (idempotent: a vault pre-created or pre-funded by someone else is fine).
        invoke(
            &Instruction {
                program_id: ASSOCIATED_TOKEN_PROGRAM_ID,
                accounts: vec![
                    AccountMeta::new(*buyer.key, true),
                    AccountMeta::new(*vault.key, false),
                    AccountMeta::new_readonly(*deal_ai.key, false),
                    AccountMeta::new_readonly(*mint.key, false),
                    AccountMeta::new_readonly(system_program::ID, false),
                    AccountMeta::new_readonly(*token_program.key, false),
                ],
                data: vec![1], // CreateIdempotent
            },
            &[
                buyer.clone(),
                vault.clone(),
                deal_ai.clone(),
                mint.clone(),
                system.clone(),
                token_program.clone(),
            ],
        )?;
        let (vmint, vowner, before) = token_account_fields(vault, token_program.key)?;
        if vmint != *mint.key || vowner != *deal_ai.key {
            return Err(EscrowError::BadAccount.into());
        }
        invoke(
            &transfer_checked_ix(token_program.key, buyer_token.key, mint.key, vault.key, buyer.key, amount, decimals),
            &[buyer_token.clone(), mint.clone(), vault.clone(), buyer.clone(), token_program.clone()],
        )?;
        // Count what actually arrived, so Token-2022 transfer-fee mints work too.
        let (_, _, after) = token_account_fields(vault, token_program.key)?;
        let received = after.checked_sub(before).ok_or(EscrowError::Overflow)?;
        if received == 0 {
            return Err(EscrowError::TransferFailed.into());
        }
        deal.amount = received;
        deal.mint = *mint.key;
        deal.token_program = *token_program.key;
        deal.vault = *vault.key;
    }

    deal.pack(&mut deal_ai.try_borrow_mut_data()?)?;
    sol_log_data(&[
        b"Opened",
        deal_ai.key.as_ref(),
        buyer.key.as_ref(),
        seller.key.as_ref(),
        deal.mint.as_ref(),
        &deal.amount.to_le_bytes(),
        &release_at.to_le_bytes(),
    ]);
    msg!("Opened deal {} amount {}", deal_id, deal.amount);
    Ok(())
}

/// Creates the deal PDA. Works even if someone sent lamports to the address first
/// (a plain create_account would fail there, letting anyone block a deal id).
fn create_deal_account<'a>(
    program_id: &Pubkey,
    payer: &AccountInfo<'a>,
    deal: &AccountInfo<'a>,
    system: &AccountInfo<'a>,
    seeds: &[&[u8]],
) -> ProgramResult {
    if deal.owner != &system_program::ID || !deal.data_is_empty() {
        return Err(EscrowError::BadState.into()); // already in use
    }
    let need = Rent::get()?.minimum_balance(Deal::LEN);
    let have = deal.lamports();
    if have == 0 {
        invoke_signed(
            &system_instruction::create_account(payer.key, deal.key, need, Deal::LEN as u64, program_id),
            &[payer.clone(), deal.clone(), system.clone()],
            &[seeds],
        )
    } else {
        if have < need {
            invoke(
                &system_instruction::transfer(payer.key, deal.key, need - have),
                &[payer.clone(), deal.clone(), system.clone()],
            )?;
        }
        invoke_signed(
            &system_instruction::allocate(deal.key, Deal::LEN as u64),
            &[deal.clone(), system.clone()],
            &[seeds],
        )?;
        invoke_signed(&system_instruction::assign(deal.key, program_id), &[deal.clone(), system.clone()], &[seeds])
    }
}

fn load_deal(program_id: &Pubkey, deal_ai: &AccountInfo) -> Result<Deal, ProgramError> {
    if deal_ai.owner != program_id || !deal_ai.is_writable {
        return Err(EscrowError::BadAccount.into());
    }
    let d = Deal::unpack(&deal_ai.try_borrow_data()?)?;
    let expected = Pubkey::create_program_address(
        &[DEAL_SEED, d.buyer.as_ref(), &d.deal_id.to_le_bytes(), &[d.bump]],
        program_id,
    )?;
    if expected != *deal_ai.key {
        return Err(EscrowError::BadAccount.into());
    }
    Ok(d)
}

fn dispute(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let it = &mut accounts.iter();
    let signer = next_account_info(it)?;
    let deal_ai = next_account_info(it)?;
    let mut d = load_deal(program_id, deal_ai)?;
    if d.status != Status::Funded {
        return Err(EscrowError::BadState.into());
    }
    if !signer.is_signer || (*signer.key != d.buyer && *signer.key != d.seller) {
        return Err(EscrowError::NotParty.into());
    }
    let t = now()?;
    if t >= d.release_at {
        return Err(EscrowError::TooLate.into());
    }
    d.status = Status::Disputed;
    d.disputed_at = t;
    d.pack(&mut deal_ai.try_borrow_mut_data()?)?;
    sol_log_data(&[b"Disputed", deal_ai.key.as_ref(), signer.key.as_ref()]);
    msg!("Disputed by {}", signer.key);
    Ok(())
}

/// Accounts for every instruction that closes a deal.
struct Payout<'a, 'b> {
    deal_ai: &'a AccountInfo<'b>,
    buyer: &'a AccountInfo<'b>,
    seller: &'a AccountInfo<'b>,
    token: Option<TokenPayout<'a, 'b>>,
    deal: Deal,
}

struct TokenPayout<'a, 'b> {
    vault: &'a AccountInfo<'b>,
    buyer_token: &'a AccountInfo<'b>,
    seller_token: &'a AccountInfo<'b>,
    mint: &'a AccountInfo<'b>,
    token_program: &'a AccountInfo<'b>,
}

impl<'a, 'b> Payout<'a, 'b> {
    /// Loads and checks every account against the stored deal. Status must be Funded or Disputed.
    fn load(program_id: &'a Pubkey, accounts: &'a [AccountInfo<'b>]) -> Result<(Self, Deal), ProgramError> {
        let it = &mut accounts.iter();
        let deal_ai = next_account_info(it)?;
        let buyer = next_account_info(it)?;
        let seller = next_account_info(it)?;
        let deal = load_deal(program_id, deal_ai)?;
        if *buyer.key != deal.buyer || *seller.key != deal.seller {
            return Err(EscrowError::NotParty.into());
        }
        let token = if deal.is_sol() {
            None
        } else {
            let t = TokenPayout {
                vault: next_account_info(it)?,
                buyer_token: next_account_info(it)?,
                seller_token: next_account_info(it)?,
                mint: next_account_info(it)?,
                token_program: next_account_info(it)?,
            };
            if *t.vault.key != deal.vault || *t.mint.key != deal.mint || *t.token_program.key != deal.token_program {
                return Err(EscrowError::BadAccount.into());
            }
            // Recipient token accounts must belong to the right party, so a third party calling
            // claim or expire cannot redirect the funds.
            let (bm, bo, _) = token_account_fields(t.buyer_token, &deal.token_program)?;
            let (sm, so, _) = token_account_fields(t.seller_token, &deal.token_program)?;
            if bm != deal.mint || bo != deal.buyer || sm != deal.mint || so != deal.seller {
                return Err(EscrowError::BadAccount.into());
            }
            Some(t)
        };
        let d = deal.clone();
        Ok((Payout { deal_ai, buyer, seller, token, deal }, d))
    }

    /// Pays out and closes the deal. The deal account (and, for classic SPL Token, an empty vault)
    /// is closed and its rent goes back to the buyer, who paid it.
    fn close(&mut self, to_seller: u64, to_buyer: u64, event: &[u8], digest: Option<&[u8; 32]>) -> ProgramResult {
        let d = &self.deal;
        debug_assert_eq!(to_seller as u128 + to_buyer as u128, d.amount as u128);

        // Effects first: wipe the deal so nothing can act on it again.
        self.deal_ai.try_borrow_mut_data()?.fill(0);

        match &self.token {
            None => {
                // The deal account is owned by this program, so it can debit it directly.
                let all = self.deal_ai.lamports();
                let rent_and_rest = all.checked_sub(d.amount).ok_or(EscrowError::Overflow)?;
                **self.deal_ai.try_borrow_mut_lamports()? = 0;
                **self.seller.try_borrow_mut_lamports()? =
                    self.seller.lamports().checked_add(to_seller).ok_or(EscrowError::Overflow)?;
                **self.buyer.try_borrow_mut_lamports()? = self
                    .buyer
                    .lamports()
                    .checked_add(to_buyer)
                    .and_then(|v| v.checked_add(rent_and_rest))
                    .ok_or(EscrowError::Overflow)?;
            }
            Some(t) => {
                let decimals = mint_decimals(t.mint, t.token_program.key)?;
                let id_le = d.deal_id.to_le_bytes();
                let seeds: &[&[u8]] = &[DEAL_SEED, d.buyer.as_ref(), &id_le, &[d.bump]];
                for (to, amt) in [(t.seller_token, to_seller), (t.buyer_token, to_buyer)] {
                    if amt > 0 {
                        invoke_signed(
                            &transfer_checked_ix(
                                t.token_program.key,
                                t.vault.key,
                                t.mint.key,
                                to.key,
                                self.deal_ai.key,
                                amt,
                                decimals,
                            ),
                            &[t.vault.clone(), t.mint.clone(), to.clone(), self.deal_ai.clone(), t.token_program.clone()],
                            &[seeds],
                        )?;
                    }
                }
                // Close the vault only when it is empty and classic SPL Token. Tokens donated to the
                // vault, or Token-2022 withheld fees, would make CloseAccount fail and block the payout,
                // so in those cases the vault is simply left open.
                let (_, _, left) = token_account_fields(t.vault, t.token_program.key)?;
                if left == 0 && *t.token_program.key == TOKEN_PROGRAM_ID {
                    invoke_signed(
                        &Instruction {
                            program_id: TOKEN_PROGRAM_ID,
                            accounts: vec![
                                AccountMeta::new(*t.vault.key, false),
                                AccountMeta::new(*self.buyer.key, false),
                                AccountMeta::new_readonly(*self.deal_ai.key, true),
                            ],
                            data: vec![9], // CloseAccount
                        },
                        &[t.vault.clone(), self.buyer.clone(), self.deal_ai.clone(), t.token_program.clone()],
                        &[seeds],
                    )?;
                }
                // Return the deal account's rent to the buyer.
                let all = self.deal_ai.lamports();
                **self.deal_ai.try_borrow_mut_lamports()? = 0;
                **self.buyer.try_borrow_mut_lamports()? =
                    self.buyer.lamports().checked_add(all).ok_or(EscrowError::Overflow)?;
            }
        }
        self.deal_ai.realloc(0, false)?; // realloc: also builds with older solana-program (Solana Playground)
        self.deal_ai.assign(&system_program::ID);

        let mut fields: Vec<&[u8]> = Vec::with_capacity(5);
        let s = to_seller.to_le_bytes();
        let b = to_buyer.to_le_bytes();
        fields.extend([event, self.deal_ai.key.as_ref(), &s[..], &b[..]]);
        if let Some(g) = digest {
            fields.push(g);
        }
        sol_log_data(&fields);
        msg!("{} to_seller {} to_buyer {}", String::from_utf8_lossy(event), to_seller, to_buyer);
        Ok(())
    }
}
