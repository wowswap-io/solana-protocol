use anchor_lang::prelude::*;
use solana_program::{entrypoint::ProgramResult, program_option::COption};

use super::{
    authority,
    error::{WowswapResult, WowswapResultEmpty},
    governance::{self, Governance},
    math::{self, Factor, Rate, TokenAmount, UnixTimestamp},
    swap::SwapPositionState,
    token::{self, SplToken, TokenAccount, TokenMint},
};

#[derive(Debug, Default, Clone, Copy, PartialEq, AnchorSerialize, AnchorDeserialize)]
pub struct ReserveState {
    pub borrow_rate: Rate,
    pub treasure_accrued: TokenAmount,
    pub treasurer_update: UnixTimestamp,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, AnchorSerialize, AnchorDeserialize)]
pub struct ReserveDebt {
    pub average_rate: Rate,
    pub total: TokenAmount,
    pub last_update: UnixTimestamp,
}

impl ReserveDebt {
    pub fn get_total_debt(&self, timestamp: UnixTimestamp) -> TokenAmount {
        self.total
            .into_ray()
            .ray_mul(math::interest::calculate_compounded(
                self.average_rate,
                self.last_update,
                timestamp,
            ))
            .as_token_amount()
    }
}

#[account]
#[derive(Debug, Default, Copy, PartialEq)]
pub struct Reserve {
    pub signer: Pubkey,
    pub nonce: u8,

    pub lendable_mint: Pubkey,
    pub lendable_vault: Pubkey,
    pub redeemable_mint: Pubkey,

    pub state: ReserveState,
    pub debt: ReserveDebt,
}

impl Reserve {
    pub fn update_state(
        &mut self,
        governance: &Governance,
        total_debt: TokenAmount,
        timestamp: UnixTimestamp,
    ) {
        self.state.treasure_accrued = self.get_liquidity_fee_accrued(governance, total_debt);
        self.state.treasurer_update = timestamp;
    }

    fn get_liquidity_fee_accrued(
        &self,
        governance: &Governance,
        current_debt: TokenAmount,
    ) -> TokenAmount {
        let fee = {
            if current_debt.is_zero() {
                TokenAmount::ZERO
            } else {
                let previous_debt = self
                    .debt
                    .total
                    .into_ray()
                    .ray_mul(math::interest::calculate_compounded(
                        self.debt.average_rate,
                        self.debt.last_update,
                        self.state.treasurer_update,
                    ))
                    .as_token_amount();

                let debt_accrued = current_debt
                    .checked_sub(previous_debt)
                    .expect("invalid debt");

                TokenAmount::from_u128(
                    governance
                        .treasure_factor()
                        .percentage_mul(debt_accrued.into_inner() as u128),
                )
            }
        };

        self.state
            .treasure_accrued
            .checked_add(fee)
            .expect("accured treasure overflow")
    }

    pub fn get_total_liquidity(
        &self,
        total_debt: TokenAmount,
        liquidity: TokenAmount,
    ) -> TokenAmount {
        total_debt
            .checked_add(liquidity)
            .and_then(|v| v.checked_sub(self.state.treasure_accrued))
            .expect("total_liquidity overflow")
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_borrow_rate(
        &mut self,
        governance: &Governance,
        liquidity: TokenAmount,
        liquidity_added: TokenAmount,
        liquidity_removed: TokenAmount,
        total_debt: TokenAmount,
        debt_added: TokenAmount,
        debt_removed: TokenAmount,
    ) {
        let debt = total_debt
            .checked_add(debt_added)
            .and_then(|v| v.checked_sub(debt_removed))
            .expect("debt overflow");

        let liquidity = liquidity
            .checked_add(liquidity_added)
            .and_then(|v| v.checked_sub(liquidity_removed))
            .expect("liquidity overflow");

        self.state.borrow_rate = math::interest::borrow_rate(
            debt,
            liquidity,
            governance.base_borrow_rate(),
            governance.excess_slope(),
            governance.optimal_slope(),
            governance.optimal_utilization(),
        );
    }

    pub fn increase_debt(
        &mut self,
        position: &mut SwapPositionState,
        timestamp: UnixTimestamp,
        previous_total: TokenAmount,
        amount: TokenAmount,
        rate_multiplier: Factor,
    ) {
        let rate = Rate::new(rate_multiplier.percentage_mul(self.state.borrow_rate.into_inner()));
        let amount_ray_rate = amount.into_wad().into_ray().ray_mul(rate.into_ray());

        let (current_debt, debt_increase) = position.calculate_debt_increase(timestamp);
        let next_total = previous_total
            .checked_add(amount)
            .expect("total debt overflow");
        self.debt.total = next_total;

        // Update user debt
        position.amount = position
            .amount
            .checked_add(amount)
            .and_then(|v| v.checked_add(debt_increase))
            .expect("amount overflow");
        position.rate = position
            .rate
            .into_ray()
            .ray_mul(current_debt.into_wad().into_ray())
            .checked_add(amount_ray_rate)
            .map(|v| {
                let debt = current_debt.checked_add(amount).expect("debt overflow");
                v.ray_div(debt.into_wad().into_ray())
            })
            .expect("rate overflow")
            .as_rate();
        position.timestamp = timestamp;

        // Recalculate an average borrow rate
        self.debt.average_rate = self
            .debt
            .average_rate
            .into_ray()
            .ray_mul(previous_total.into_wad().into_ray())
            .checked_add(amount_ray_rate)
            .map(|v| v.ray_div(next_total.into_wad().into_ray()))
            .expect("rate overflow")
            .as_rate();
        self.debt.last_update = timestamp;
    }

    pub fn decrease_debt(
        &mut self,
        position: &mut SwapPositionState,
        timestamp: UnixTimestamp,
        reserve_total_debt: TokenAmount,
        debt_change: TokenAmount,
    ) {
        let (current_debt, debt_increase) = position.calculate_debt_increase(timestamp);

        // Since the total debt and each individual user's debts are accrued separately, due to an
        // accumulation error the last borrower to repay loan may try to repay more than the total
        // debt outstanding.
        // In this case when the last borrower repays the debt, we simply set the total outstanding
        // debt and the average stable rate to 0.
        if reserve_total_debt <= debt_change {
            self.debt.average_rate = Rate::ZERO;
            self.debt.total = TokenAmount::ZERO;
        } else {
            let next_total = reserve_total_debt
                .checked_sub(debt_change)
                .expect("total debt overflow");
            self.debt.total = next_total;

            // For the reason described above, when the last user repays the debt, it might happen
            // that user's rate * user's balance > avg rate * total debt. In that case, we simply
            // set the avg rate to 0
            let first_term = self
                .debt
                .average_rate
                .into_ray()
                .ray_mul(reserve_total_debt.into_wad().into_ray());
            let second_term = position
                .rate
                .into_ray()
                .ray_mul(debt_change.into_wad().into_ray());

            if second_term >= first_term {
                self.debt.average_rate = Rate::ZERO;
                self.debt.total = TokenAmount::ZERO;
            } else {
                self.debt.average_rate = first_term
                    .checked_sub(second_term)
                    .expect("rate overflow")
                    .ray_div(next_total.into_wad().into_ray())
                    .as_rate();
            }
        }

        if debt_change == current_debt {
            position.rate = Rate::ZERO;
            position.amount = TokenAmount::ZERO;
            position.timestamp = UnixTimestamp::ZERO;
        } else {
            position.amount = position
                .amount
                .checked_add(debt_increase)
                .and_then(|v| v.checked_sub(debt_change))
                .expect("amount overflow");
            position.timestamp = timestamp;
        }

        self.debt.last_update = timestamp;
    }
}

#[derive(Accounts)]
#[instruction(nonce: u8)]
pub struct ReserveInitialize<'info> {
    #[account(init, payer = payer, space = 489)] // Current size is 169
    reserve: Box<Account<'info, Reserve>>,
    #[account(seeds = [(*reserve).as_ref().key.as_ref()], bump = nonce)]
    signer: AccountInfo<'info>,

    lendable_mint: Box<Account<'info, TokenMint>>,
    #[account(
        constraint = lendable_vault.mint == *(*lendable_mint).as_ref().key,
        constraint = lendable_vault.owner == *signer.key,
        constraint = lendable_vault.amount == 0,
        constraint = lendable_vault.delegate.is_none(),
        constraint = lendable_vault.close_authority.is_none(),
        constraint = token::check_associated_address(&lendable_vault.mint, &signer, &lendable_vault),
    )]
    lendable_vault: Box<Account<'info, TokenAccount>>,
    #[account(
        constraint = redeemable_mint.mint_authority == COption::Some(*signer.key),
        constraint = redeemable_mint.supply == 0,
        constraint = redeemable_mint.decimals == lendable_mint.decimals,
    )]
    redeemable_mint: Box<Account<'info, TokenMint>>,

    #[account(constraint = *authority.as_ref().key == authority::ID)]
    authority: Signer<'info>,

    payer: Signer<'info>,
    system_program: Program<'info, System>,
}

impl<'info> ReserveInitialize<'info> {
    pub fn handle(&mut self, nonce: u8) -> WowswapResultEmpty {
        let reserve = &mut self.reserve;

        reserve.signer = *self.signer.key;
        reserve.nonce = nonce;

        reserve.lendable_mint = *(*self.lendable_mint).as_ref().key;
        reserve.lendable_vault = *(*self.lendable_vault).as_ref().key;
        reserve.redeemable_mint = *(*self.redeemable_mint).as_ref().key;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct ReserveDeposit<'info> {
    #[account(
        mut,
        constraint = reserve.signer == *reserve_signer.key,
        constraint = *(*reserve_lendable_vault).as_ref().key == reserve.lendable_vault,
        constraint = *(*reserve_redeemable_mint).as_ref().key == reserve.redeemable_mint,
    )]
    reserve: Box<Account<'info, Reserve>>,
    reserve_signer: AccountInfo<'info>,

    #[account(constraint = *(*governance).as_ref().key == governance::ID)]
    governance: Box<Account<'info, Governance>>,

    #[account(mut)]
    reserve_lendable_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    reserve_redeemable_mint: Box<Account<'info, TokenMint>>,

    investor: Signer<'info>,
    #[account(mut)]
    investor_lendable_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut, constraint = investor_redeemable_vault.owner == *investor.key)]
    investor_redeemable_vault: Box<Account<'info, TokenAccount>>,

    spl_token_program: Program<'info, SplToken>,
}

impl<'info> ReserveDeposit<'info> {
    pub fn handle(&mut self, amount: TokenAmount) -> WowswapResultEmpty {
        let mint_amount = self.reserve_update_state(amount)?;
        self.take_investor_funds(amount)?;
        self.mint_redeemable(mint_amount)?;
        Ok(())
    }

    fn reserve_update_state(&mut self, amount: TokenAmount) -> WowswapResult<TokenAmount> {
        let timestamp = UnixTimestamp::now()?;

        let reserve = &mut self.reserve;
        let governance = &self.governance;
        let total_debt = reserve.debt.get_total_debt(timestamp);
        reserve.update_state(governance, total_debt, timestamp);

        let liquidity = TokenAmount::new(self.reserve_lendable_vault.amount);
        reserve.update_borrow_rate(
            governance,
            liquidity,
            amount,
            TokenAmount::ZERO,
            total_debt,
            TokenAmount::ZERO,
            TokenAmount::ZERO,
        );

        let total_supply = TokenAmount::new(self.reserve_redeemable_mint.supply);
        let total_liquidity = reserve.get_total_liquidity(total_debt, liquidity);
        let mint_amount = math::liquidity::mint_amount(amount, total_supply, total_liquidity);

        Ok(mint_amount)
    }

    fn take_investor_funds(&self, amount: TokenAmount) -> ProgramResult {
        token::transfer(
            self.investor_lendable_vault.to_account_info(),
            self.reserve_lendable_vault.to_account_info(),
            self.investor.to_account_info(),
            amount,
            &[],
        )
    }

    fn mint_redeemable(&self, amount: TokenAmount) -> ProgramResult {
        token::mint_to(
            self.reserve_redeemable_mint.to_account_info(),
            self.investor_redeemable_vault.to_account_info(),
            self.reserve_signer.to_account_info(),
            amount,
            &[&[(*self.reserve).as_ref().key.as_ref(), &[self.reserve.nonce]]],
        )
    }
}

#[derive(Accounts)]
pub struct ReserveWithdraw<'info> {
    #[account(
        mut,
        constraint = reserve.signer == *reserve_signer.key,
        constraint = *(*reserve_lendable_vault).as_ref().key == reserve.lendable_vault,
        constraint = *(*reserve_redeemable_mint).as_ref().key == reserve.redeemable_mint,
    )]
    reserve: Box<Account<'info, Reserve>>,
    reserve_signer: AccountInfo<'info>,

    #[account(constraint = *(*governance).as_ref().key == governance::ID)]
    governance: Box<Account<'info, Governance>>,

    #[account(mut)]
    reserve_lendable_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    reserve_redeemable_mint: Box<Account<'info, TokenMint>>,

    investor: Signer<'info>,
    #[account(mut, constraint = investor_lendable_vault.owner == *investor.key)]
    investor_lendable_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    investor_redeemable_vault: Box<Account<'info, TokenAccount>>,

    spl_token_program: Program<'info, SplToken>,
}

impl<'info> ReserveWithdraw<'info> {
    pub fn handle(&mut self, amount: TokenAmount) -> WowswapResultEmpty {
        let (burn_amount, withdraw_amount) = self.reserve_update_state(amount)?;
        self.burn_redeemable(burn_amount)?;
        self.payout_investor_funds(withdraw_amount)?;
        Ok(())
    }

    fn reserve_update_state(
        &mut self,
        amount: TokenAmount,
    ) -> WowswapResult<(TokenAmount, TokenAmount)> {
        let timestamp = UnixTimestamp::now()?;

        let reserve = &mut self.reserve;

        let liquidity = TokenAmount::new(self.reserve_lendable_vault.amount);
        let total_supply = TokenAmount::new(self.reserve_redeemable_mint.supply);
        let total_debt = reserve.debt.get_total_debt(timestamp);
        let total_liquidity = reserve.get_total_liquidity(total_debt, liquidity);
        let mut amount_to_withdraw =
            math::liquidity::calculate_share(amount, total_supply, total_liquidity);

        let burn_amount = if amount_to_withdraw > liquidity {
            let portion = liquidity.into_wad().wad_div(amount_to_withdraw.into_wad());
            let portion_amount = amount.into_wad().wad_mul(portion);
            amount_to_withdraw = liquidity;
            portion_amount.as_token_amount()
        } else {
            amount
        };

        let governance = &self.governance;
        reserve.update_state(governance, total_debt, timestamp);

        reserve.update_borrow_rate(
            governance,
            liquidity,
            TokenAmount::ZERO,
            amount_to_withdraw,
            total_debt,
            TokenAmount::ZERO,
            TokenAmount::ZERO,
        );

        Ok((burn_amount, amount_to_withdraw))
    }

    fn burn_redeemable(&self, amount: TokenAmount) -> ProgramResult {
        token::burn(
            self.reserve_redeemable_mint.to_account_info(),
            self.investor_redeemable_vault.to_account_info(),
            self.investor.to_account_info(),
            amount,
            &[],
        )
    }

    fn payout_investor_funds(&self, amount: TokenAmount) -> ProgramResult {
        token::transfer(
            self.reserve_lendable_vault.to_account_info(),
            self.investor_lendable_vault.to_account_info(),
            self.reserve_signer.clone(),
            amount,
            &[&[(*self.reserve).as_ref().key.as_ref(), &[self.reserve.nonce]]],
        )
    }
}
