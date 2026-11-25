// SPDX-License-Identifier: MIT
//! Program errors. They mirror the EVM TruceEscrow errors where one exists.

use solana_program::program_error::ProgramError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EscrowError {
    /// Wrong deal status for this instruction, or invalid open parameters.
    BadState = 0,
    /// Caller is not the buyer or seller this instruction requires.
    NotParty = 1,
    /// Deadline or dispute timeout not reached yet.
    TooEarly = 2,
    /// Release deadline already passed.
    TooLate = 3,
    /// Settlement split does not add up to the escrowed amount.
    BadSplit = 4,
    /// An account does not match the deal (wrong PDA, wrong vault, wrong mint, wrong owner).
    BadAccount = 5,
    /// Token transfer into the vault delivered nothing.
    TransferFailed = 6,
    /// Instruction data could not be decoded.
    InvalidInstruction = 7,
    /// Arithmetic overflow.
    Overflow = 8,
}

impl From<EscrowError> for ProgramError {
    fn from(e: EscrowError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
