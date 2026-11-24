use crate::{
    constants::{STATE, VAULT_SEED},
    state::VaultState,
};
use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

#[derive(Accounts)]
pub struct Close<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    // `close = user` zeroes the state account and sends its rent back to the user
    // after the instruction runs.
    #[account(
        mut,
        close = user,
        seeds = [STATE, user.key().as_ref()],
        bump = vault_state.state_bump
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        seeds = [VAULT_SEED, user.key().as_ref()],
        bump = vault_state.vault_bump
    )]
    pub vault: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}

impl<'info> Close<'info> {
    pub fn close(&mut self) -> Result<()> {
        // Drain every lamport (deposits + rent). A system account with 0 lamports
        // is removed by the runtime at the end of the transaction.
        let user_key = self.user.key();
        let bump = [self.vault_state.vault_bump];
        let signer_seeds: &[&[&[u8]]] = &[&[VAULT_SEED, user_key.as_ref(), &bump]];

        let cpi_accounts = Transfer {
            from: self.vault.to_account_info(),
            to: self.user.to_account_info(),
        };
        let cpi_ctx =
            CpiContext::new_with_signer(self.system_program.key(), cpi_accounts, signer_seeds);

        transfer(cpi_ctx, self.vault.lamports())
    }
}
