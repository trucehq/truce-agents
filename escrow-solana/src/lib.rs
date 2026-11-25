// SPDX-License-Identifier: MIT
//! TruceEscrow for Solana: minimal, non-custodial escrow with a neutral settlement step.
//!
//! The buyer locks SOL or an SPL token for a seller in a program-owned PDA. Funds move only when:
//!   1. the buyer releases them to the seller, or the seller refunds the buyer; or
//!   2. the release deadline passes with no dispute, and anyone triggers the seller's claim; or
//!   3. buyer AND seller both sign one transaction that pays out a split, typically the outcome of
//!      a TruceHQ agreement, referenced by its agreement digest; or
//!   4. a dispute is open and the dispute timeout passes with no settlement: the funds are split
//!      evenly, so neither side gains by stalling.
//!
//! TruceHQ (or any mediator) never holds keys, never signs and never receives funds here.
//! Unaudited template: audit before use with real funds.

// solana-program 2.3 marks its system_program / system_instruction re-exports deprecated in favour of
// separate crates; they are kept here to keep the dependency list to one crate.
#![allow(deprecated)]

pub mod error;
pub mod instruction;
pub mod processor;
pub mod state;

#[cfg(not(feature = "no-entrypoint"))]
mod entrypoint {
    use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};

    solana_program::entrypoint!(process_instruction);

    fn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
        crate::processor::process_instruction(program_id, accounts, data)
    }
}
