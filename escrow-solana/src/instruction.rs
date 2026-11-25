// SPDX-License-Identifier: MIT
//! Instruction encoding and client-side builders.
//!
//! Encoding: one tag byte, then little-endian fields. No Borsh, so any client can build it by hand.

use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
    system_program,
};

use crate::{error::EscrowError, state::Deal};

pub const TOKEN_PROGRAM_ID: Pubkey = solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const TOKEN_2022_PROGRAM_ID: Pubkey = solana_program::pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
pub const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey =
    solana_program::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscrowInstruction {
    /// Buyer opens a deal and locks SOL or an SPL token.
    ///
    /// Accounts (SOL):
    ///   0. `[signer, writable]` buyer
    ///   1. `[]` seller
    ///   2. `[writable]` deal PDA ["deal", buyer, deal_id]
    ///   3. `[]` system program
    /// Additional accounts (token deal):
    ///   4. `[]` mint
    ///   5. `[writable]` vault: associated token account of the deal PDA for this mint
    ///   6. `[writable]` buyer's token account (source)
    ///   7. `[]` token program (SPL Token or Token-2022)
    ///   8. `[]` associated token account program
    Open { deal_id: u64, amount: u64, release_at: i64, dispute_timeout: i64 },
    /// Buyer or seller opens a dispute before `release_at`.
    ///
    ///   0. `[signer]` buyer or seller
    ///   1. `[writable]` deal PDA
    Dispute,
    /// Buyer pays everything to the seller. Payout accounts, buyer must sign.
    Release,
    /// Seller refunds everything to the buyer. Payout accounts, seller must sign.
    Refund,
    /// Anyone pays the seller after `release_at` if not disputed. Payout accounts.
    Claim,
    /// Anyone splits the funds evenly once a dispute is older than `dispute_timeout`. Payout accounts.
    Expire,
    /// Pay out a split both parties agreed. Payout accounts, buyer AND seller must sign.
    /// `agreement_digest` is e.g. the TruceHQ agreement digest; it is logged with the payout.
    Settle { to_seller: u64, to_buyer: u64, agreement_digest: [u8; 32] },
}

// Payout accounts, shared by Release, Refund, Claim, Expire and Settle:
//   0. `[writable]` deal PDA
//   1. `[writable]` buyer (receives SOL and the rent of closed accounts)
//   2. `[writable]` seller
// Additional accounts (token deal):
//   3. `[writable]` vault
//   4. `[writable]` buyer's token account (owner = buyer, same mint)
//   5. `[writable]` seller's token account (owner = seller, same mint)
//   6. `[]` mint
//   7. `[]` token program

impl EscrowInstruction {
    pub fn pack(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(73);
        match self {
            Self::Open { deal_id, amount, release_at, dispute_timeout } => {
                v.push(0);
                v.extend_from_slice(&deal_id.to_le_bytes());
                v.extend_from_slice(&amount.to_le_bytes());
                v.extend_from_slice(&release_at.to_le_bytes());
                v.extend_from_slice(&dispute_timeout.to_le_bytes());
            }
            Self::Dispute => v.push(1),
            Self::Release => v.push(2),
            Self::Refund => v.push(3),
            Self::Claim => v.push(4),
            Self::Expire => v.push(5),
            Self::Settle { to_seller, to_buyer, agreement_digest } => {
                v.push(6);
                v.extend_from_slice(&to_seller.to_le_bytes());
                v.extend_from_slice(&to_buyer.to_le_bytes());
                v.extend_from_slice(agreement_digest);
            }
        }
        v
    }

    pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        let (&tag, rest) = data.split_first().ok_or(EscrowError::InvalidInstruction)?;
        let u = |o: usize| -> Result<u64, ProgramError> {
            rest.get(o..o + 8)
                .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
                .ok_or_else(|| EscrowError::InvalidInstruction.into())
        };
        let exact = |n: usize| -> Result<(), ProgramError> {
            if rest.len() == n { Ok(()) } else { Err(EscrowError::InvalidInstruction.into()) }
        };
        Ok(match tag {
            0 => {
                exact(32)?;
                Self::Open {
                    deal_id: u(0)?,
                    amount: u(8)?,
                    release_at: u(16)? as i64,
                    dispute_timeout: u(24)? as i64,
                }
            }
            1 => { exact(0)?; Self::Dispute }
            2 => { exact(0)?; Self::Release }
            3 => { exact(0)?; Self::Refund }
            4 => { exact(0)?; Self::Claim }
            5 => { exact(0)?; Self::Expire }
            6 => {
                exact(48)?;
                Self::Settle {
                    to_seller: u(0)?,
                    to_buyer: u(8)?,
                    agreement_digest: rest[16..48].try_into().unwrap(),
                }
            }
            _ => return Err(EscrowError::InvalidInstruction.into()),
        })
    }
}

/// Associated token account address (the vault for a deal PDA).
pub fn associated_token_address(wallet: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[wallet.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0
}

/// Token deal details for the builders.
#[derive(Clone, Copy, Debug)]
pub struct TokenAccounts {
    pub mint: Pubkey,
    pub token_program: Pubkey,
    /// Buyer's token account (source on open, refund destination on payout).
    pub buyer_token: Pubkey,
    /// Seller's token account (payout destination). Ignored by `open`.
    pub seller_token: Pubkey,
}

#[allow(clippy::too_many_arguments)]
pub fn open(
    program_id: &Pubkey,
    buyer: &Pubkey,
    seller: &Pubkey,
    deal_id: u64,
    amount: u64,
    release_at: i64,
    dispute_timeout: i64,
    token: Option<TokenAccounts>,
) -> Instruction {
    let (deal, _) = Deal::find_address(program_id, buyer, deal_id);
    let mut accounts = vec![
        AccountMeta::new(*buyer, true),
        AccountMeta::new_readonly(*seller, false),
        AccountMeta::new(deal, false),
        AccountMeta::new_readonly(system_program::ID, false),
    ];
    if let Some(t) = token {
        accounts.extend([
            AccountMeta::new_readonly(t.mint, false),
            AccountMeta::new(associated_token_address(&deal, &t.mint, &t.token_program), false),
            AccountMeta::new(t.buyer_token, false),
            AccountMeta::new_readonly(t.token_program, false),
            AccountMeta::new_readonly(ASSOCIATED_TOKEN_PROGRAM_ID, false),
        ]);
    }
    Instruction {
        program_id: *program_id,
        accounts,
        data: EscrowInstruction::Open { deal_id, amount, release_at, dispute_timeout }.pack(),
    }
}

pub fn dispute(program_id: &Pubkey, signer: &Pubkey, deal: &Pubkey) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![AccountMeta::new_readonly(*signer, true), AccountMeta::new(*deal, false)],
        data: EscrowInstruction::Dispute.pack(),
    }
}

/// Builds any payout instruction (Release, Refund, Claim, Expire, Settle).
pub fn payout(
    program_id: &Pubkey,
    ix: EscrowInstruction,
    deal: &Pubkey,
    buyer: &Pubkey,
    seller: &Pubkey,
    token: Option<TokenAccounts>,
) -> Instruction {
    let (buyer_signs, seller_signs) = match ix {
        EscrowInstruction::Release => (true, false),
        EscrowInstruction::Refund => (false, true),
        EscrowInstruction::Settle { .. } => (true, true),
        _ => (false, false),
    };
    let mut accounts = vec![
        AccountMeta::new(*deal, false),
        AccountMeta::new(*buyer, buyer_signs),
        AccountMeta::new(*seller, seller_signs),
    ];
    if let Some(t) = token {
        accounts.extend([
            AccountMeta::new(associated_token_address(deal, &t.mint, &t.token_program), false),
            AccountMeta::new(t.buyer_token, false),
            AccountMeta::new(t.seller_token, false),
            AccountMeta::new_readonly(t.mint, false),
            AccountMeta::new_readonly(t.token_program, false),
        ]);
    }
    Instruction { program_id: *program_id, accounts, data: ix.pack() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all() {
        for ix in [
            EscrowInstruction::Open { deal_id: 9, amount: 5, release_at: 100, dispute_timeout: 86_400 },
            EscrowInstruction::Dispute,
            EscrowInstruction::Release,
            EscrowInstruction::Refund,
            EscrowInstruction::Claim,
            EscrowInstruction::Expire,
            EscrowInstruction::Settle { to_seller: 3, to_buyer: 4, agreement_digest: [7u8; 32] },
        ] {
            assert_eq!(EscrowInstruction::unpack(&ix.pack()).unwrap(), ix);
        }
    }

    #[test]
    fn rejects_bad_data() {
        assert!(EscrowInstruction::unpack(&[]).is_err());
        assert!(EscrowInstruction::unpack(&[99]).is_err());
        assert!(EscrowInstruction::unpack(&[0, 1, 2]).is_err());
        assert!(EscrowInstruction::unpack(&[2, 0]).is_err()); // trailing bytes
        assert!(EscrowInstruction::unpack(&[6; 40]).is_err());
    }
}
