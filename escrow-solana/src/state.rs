// SPDX-License-Identifier: MIT
//! Deal account layout. One program-owned PDA per deal:
//! seeds = ["deal", buyer, deal_id (u64 little endian)].
//!
//! For SOL deals the escrowed lamports sit in this same account, on top of its rent.
//! For token deals the tokens sit in the deal PDA's associated token account (the vault),
//! whose only authority is the deal PDA, so only this program can move them.

use solana_program::{program_error::ProgramError, pubkey::Pubkey};

use crate::error::EscrowError;

pub const DEAL_SEED: &[u8] = b"deal";
/// Minimum dispute timeout, as in the EVM version: 1 day.
pub const MIN_DISPUTE_TIMEOUT: i64 = 86_400;
const VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Status {
    /// Unused or closed account. A closed deal account is wiped and returned to the system program.
    None = 0,
    Funded = 1,
    Disputed = 2,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deal {
    pub status: Status,
    pub bump: u8,
    pub buyer: Pubkey,
    pub seller: Pubkey,
    /// `Pubkey::default()` means native SOL.
    pub mint: Pubkey,
    /// SPL Token or Token-2022 program id; `Pubkey::default()` for SOL.
    pub token_program: Pubkey,
    /// The deal PDA's associated token account; `Pubkey::default()` for SOL.
    pub vault: Pubkey,
    pub deal_id: u64,
    /// What actually arrived in escrow (fee-on-transfer aware).
    pub amount: u64,
    /// Unix time after which the seller may claim if nobody disputed.
    pub release_at: i64,
    /// Seconds a dispute may stay open before anyone can split the funds evenly.
    pub dispute_timeout: i64,
    pub disputed_at: i64,
}

impl Deal {
    pub const LEN: usize = 1 + 1 + 1 + 32 * 5 + 8 * 5;

    pub fn is_sol(&self) -> bool {
        self.mint == Pubkey::default()
    }

    pub fn seeds<'a>(buyer: &'a Pubkey, deal_id_le: &'a [u8; 8]) -> [&'a [u8]; 3] {
        [DEAL_SEED, buyer.as_ref(), deal_id_le]
    }

    pub fn find_address(program_id: &Pubkey, buyer: &Pubkey, deal_id: u64) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[DEAL_SEED, buyer.as_ref(), &deal_id.to_le_bytes()], program_id)
    }

    pub fn pack(&self, dst: &mut [u8]) -> Result<(), ProgramError> {
        if dst.len() < Self::LEN {
            return Err(ProgramError::AccountDataTooSmall);
        }
        let d = &mut dst[..Self::LEN];
        d[0] = VERSION;
        d[1] = self.status as u8;
        d[2] = self.bump;
        let mut o = 3;
        for k in [&self.buyer, &self.seller, &self.mint, &self.token_program, &self.vault] {
            d[o..o + 32].copy_from_slice(k.as_ref());
            o += 32;
        }
        d[o..o + 8].copy_from_slice(&self.deal_id.to_le_bytes());
        d[o + 8..o + 16].copy_from_slice(&self.amount.to_le_bytes());
        d[o + 16..o + 24].copy_from_slice(&self.release_at.to_le_bytes());
        d[o + 24..o + 32].copy_from_slice(&self.dispute_timeout.to_le_bytes());
        d[o + 32..o + 40].copy_from_slice(&self.disputed_at.to_le_bytes());
        Ok(())
    }

    pub fn unpack(src: &[u8]) -> Result<Self, ProgramError> {
        if src.len() < Self::LEN || src[0] != VERSION {
            return Err(EscrowError::BadState.into());
        }
        let status = match src[1] {
            1 => Status::Funded,
            2 => Status::Disputed,
            _ => return Err(EscrowError::BadState.into()),
        };
        let key = |o: usize| Pubkey::new_from_array(src[o..o + 32].try_into().unwrap());
        let u = |o: usize| u64::from_le_bytes(src[o..o + 8].try_into().unwrap());
        let i = |o: usize| i64::from_le_bytes(src[o..o + 8].try_into().unwrap());
        Ok(Deal {
            status,
            bump: src[2],
            buyer: key(3),
            seller: key(35),
            mint: key(67),
            token_program: key(99),
            vault: key(131),
            deal_id: u(163),
            amount: u(171),
            release_at: i(179),
            dispute_timeout: i(187),
            disputed_at: i(195),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_roundtrip() {
        let d = Deal {
            status: Status::Disputed,
            bump: 254,
            buyer: Pubkey::new_unique(),
            seller: Pubkey::new_unique(),
            mint: Pubkey::new_unique(),
            token_program: Pubkey::new_unique(),
            vault: Pubkey::new_unique(),
            deal_id: 7,
            amount: 1_000_001,
            release_at: 1_700_000_000,
            dispute_timeout: MIN_DISPUTE_TIMEOUT,
            disputed_at: -5,
        };
        let mut buf = vec![0u8; Deal::LEN];
        d.pack(&mut buf).unwrap();
        assert_eq!(Deal::unpack(&buf).unwrap(), d);
    }

    #[test]
    fn unpack_rejects_wiped_or_closed() {
        assert!(Deal::unpack(&[0u8; Deal::LEN]).is_err());
        let mut buf = vec![0u8; Deal::LEN];
        buf[0] = VERSION; // status None
        assert!(Deal::unpack(&buf).is_err());
        assert!(Deal::unpack(&[]).is_err());
    }
}
