use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    #[msg("Not enough lamports in the vault above the rent-exempt minimum")]
    InsufficientFunds,
}
