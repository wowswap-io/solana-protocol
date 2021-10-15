use anchor_lang::prelude::*;
use serum_dex::state::{MarketState, ToAlignedBytes};
use solana_program::{
    entrypoint::ProgramResult, program_error::ProgramError, program_option::COption,
};
use std::convert::identity;

use super::{
    authority,
    dex::{
        self, Dex, DexAccounts, DexLimitPrice, DexNonZeroTokenAmount, DexNonZeroTokenQty,
        DexTokenQty, __client_accounts_dex_accounts, __cpi_client_accounts_dex_accounts,
    },
    error::{WowswapError, WowswapResultEmpty},
    governance::{self, Governance},
    math::{self, Factor, Rate, TokenAmount, UnixTimestamp},
    reserve::Reserve,
    token::{self, SplToken, TokenAccount, TokenAccountState, TokenMint},
};

#[derive(Debug, Default, Clone, Copy, PartialEq, AnchorSerialize, AnchorDeserialize)]
pub struct SwapState {
    pub total_loan: TokenAmount,
}

#[account]
#[derive(Debug, Default, Copy, PartialEq)]
pub struct Swap {
    pub signer: Pubkey,
    pub nonce: u8,

    pub reserve: Pubkey,

    pub coin_mint: Pubkey,
    pub coin_vault: Pubkey,

    pub pc_mint: Pubkey,
    pub pc_vault: Pubkey,

    pub proxy_token_mint: Pubkey,
    pub state: SwapState,

    pub dex_program: Pubkey,
    pub dex_market: Pubkey,
    pub dex_open_orders: Pubkey,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, AnchorSerialize, AnchorDeserialize)]
pub struct SwapPositionState {
    pub loan: TokenAmount,
    pub rate: Rate,
    pub amount: TokenAmount,
    pub timestamp: UnixTimestamp,
}

impl SwapPositionState {
    pub fn calculate_debt_increase(&self, timestamp: UnixTimestamp) -> (TokenAmount, TokenAmount) {
        if self.amount.is_zero() {
            (TokenAmount::ZERO, TokenAmount::ZERO)
        } else {
            let current_debt = self.get_debt(timestamp);
            let increase = current_debt
                .checked_sub(self.amount)
                .expect("invalid increase");
            (current_debt, increase)
        }
    }

    pub fn get_debt(&self, timestamp: UnixTimestamp) -> TokenAmount {
        self.amount
            .into_ray()
            .ray_mul(math::interest::calculate_compounded(
                self.rate,
                self.timestamp,
                timestamp,
            ))
            .as_token_amount()
    }
}

#[account]
#[derive(Debug, Copy, Default, PartialEq)]
pub struct SwapPosition {
    pub nonce: u8,

    pub swap: Pubkey,
    pub trader: Pubkey,

    pub proxy_token_account: Pubkey,

    pub state: SwapPositionState,
}

#[derive(Accounts)]
#[instruction(nonce: u8)]
pub struct SwapInitialize<'info> {
    #[account(init, payer = payer, space = 657)] // Current size is 337
    swap: Box<Account<'info, Swap>>,
    #[account(seeds = [(*swap).as_ref().key.as_ref()], bump = nonce)]
    signer: AccountInfo<'info>,

    #[account(
        constraint = reserve.lendable_mint == *(*pc_mint).as_ref().key
    )]
    reserve: Box<Account<'info, Reserve>>,

    coin_mint: Box<Account<'info, TokenMint>>,
    #[account(
        constraint = coin_vault.mint == *(*coin_mint).as_ref().key,
        constraint = coin_vault.owner == *signer.key,
        constraint = coin_vault.amount == 0,
        constraint = coin_vault.delegate.is_none(),
        constraint = coin_vault.state == TokenAccountState::Initialized,
        constraint = coin_vault.close_authority.is_none(),
        constraint = token::check_associated_address(&coin_vault.mint, &signer, &coin_vault),
    )]
    coin_vault: Box<Account<'info, TokenAccount>>,
    pc_mint: Box<Account<'info, TokenMint>>,
    #[account(
        constraint = pc_vault.mint == *(*pc_mint).as_ref().key,
        constraint = pc_vault.owner == *signer.key,
        constraint = pc_vault.amount == 0,
        constraint = pc_vault.delegate.is_none(),
        constraint = pc_vault.state == TokenAccountState::Initialized,
        constraint = pc_vault.close_authority.is_none(),
        constraint = token::check_associated_address(&pc_vault.mint, &signer, &pc_vault),
    )]
    pc_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        constraint = proxy_token_mint.mint_authority == COption::Some(*signer.key),
        constraint = proxy_token_mint.supply == 0,
        constraint = proxy_token_mint.decimals == pc_mint.decimals,
    )]
    proxy_token_mint: Box<Account<'info, TokenMint>>,

    dex_program: Program<'info, Dex>,
    dex_market: AccountInfo<'info>,
    #[account(mut)]
    dex_open_orders: AccountInfo<'info>,

    #[account(constraint = *authority.as_ref().key == authority::ID)]
    authority: Signer<'info>,

    payer: Signer<'info>,
    system_program: Program<'info, System>,
}

impl<'info> SwapInitialize<'info> {
    pub fn handle(&mut self, nonce: u8) -> WowswapResultEmpty {
        self.validate_market()?;
        self.initialize(nonce);
        self.init_open_orders()?;
        Ok(())
    }

    fn validate_market(&self) -> ProgramResult {
        let market = MarketState::load(&self.dex_market, self.dex_program.key)?;

        require!(
            identity(market.coin_mint) == (*self.coin_mint).as_ref().key.to_aligned_bytes(),
            WowswapError::InvalidMint
        );

        require!(
            identity(market.pc_mint) == (*self.pc_mint).as_ref().key.to_aligned_bytes(),
            WowswapError::InvalidMint
        );

        Ok(())
    }

    fn initialize(&mut self, nonce: u8) {
        let swap = &mut self.swap;

        swap.nonce = nonce;
        swap.signer = *self.signer.key;

        swap.reserve = *(*self.reserve).as_ref().key;

        swap.coin_mint = *(*self.coin_mint).as_ref().key;
        swap.coin_vault = *(*self.coin_vault).as_ref().key;

        swap.pc_mint = *(*self.pc_mint).as_ref().key;
        swap.pc_vault = *(*self.pc_vault).as_ref().key;

        swap.proxy_token_mint = *(*self.proxy_token_mint).as_ref().key;

        swap.dex_program = *self.dex_program.as_ref().key;
        swap.dex_market = *self.dex_market.key;
        swap.dex_open_orders = *self.dex_open_orders.key;
    }

    fn init_open_orders(&self) -> ProgramResult {
        dex::init_open_orders(
            self.dex_program.to_account_info(),
            self.dex_open_orders.clone(),
            self.signer.clone(),
            self.dex_market.clone(),
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }
}

#[derive(Accounts)]
#[instruction(nonce: u8)]
pub struct SwapPositionInitialize<'info> {
    #[account(
        init,
        seeds = [
            (*swap).as_ref().key.as_ref(),
            trader.key.as_ref()
        ],
        bump = nonce,
        payer = trader,
        space = 465, // Current size is 145
    )]
    position: Box<Account<'info, SwapPosition>>,

    #[account(has_one = proxy_token_mint)]
    swap: Box<Account<'info, Swap>>,

    #[account(mut)]
    trader: Signer<'info>,

    proxy_token_mint: Box<Account<'info, TokenMint>>,
    #[account(
        constraint = proxy_token_account.mint == *(*proxy_token_mint).as_ref().key,
        constraint = proxy_token_account.owner == swap.signer,
        constraint = proxy_token_account.delegate.is_none(),
        constraint = proxy_token_account.state == TokenAccountState::Initialized,
        constraint = proxy_token_account.close_authority.is_none(),
        constraint = token::check_associated_address(&proxy_token_account.mint, &trader, &proxy_token_account),
    )]
    proxy_token_account: Box<Account<'info, TokenAccount>>,

    system_program: Program<'info, System>, // Required because `position` is `init` with `seeds`
}

impl<'info> SwapPositionInitialize<'info> {
    pub fn handle(&mut self, nonce: u8) -> WowswapResultEmpty {
        let position = &mut self.position;

        position.nonce = nonce;

        position.swap = *(*self.swap).as_ref().key;
        position.trader = *self.trader.key;

        position.proxy_token_account = *(*self.proxy_token_account).as_ref().key;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct SwapPositionOpen<'info> {
    #[account(
        mut,
        has_one = swap,
        has_one = trader,
        has_one = proxy_token_account,
        seeds = [
            (*swap).as_ref().key.as_ref(),
            trader.key.as_ref()
        ],
        bump = position.nonce,
    )]
    position: Box<Account<'info, SwapPosition>>,

    #[account(
        mut,
        constraint = swap.signer == *swap_signer.key,
        has_one = reserve,
        constraint = swap.coin_vault == *(*swap_coin_vault).as_ref().key,
        constraint = swap.pc_vault == *(*swap_pc_vault).as_ref().key,
        has_one = proxy_token_mint,
    )]
    swap: Box<Account<'info, Swap>>,
    swap_signer: AccountInfo<'info>,

    #[account(mut)]
    swap_coin_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    swap_pc_vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    proxy_token_mint: Box<Account<'info, TokenMint>>,
    #[account(mut)]
    proxy_token_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = reserve.signer == *reserve_signer.key,
        constraint = reserve.lendable_vault == *(*reserve_lendable_vault).as_ref().key,
    )]
    reserve: Box<Account<'info, Reserve>>,
    reserve_signer: AccountInfo<'info>,
    #[account(mut)]
    reserve_lendable_vault: Box<Account<'info, TokenAccount>>,

    #[account(constraint = *(*governance).as_ref().key == governance::ID)]
    governance: Box<Account<'info, Governance>>,

    trader: Signer<'info>,

    #[account(mut, constraint = trader_pc_vault.owner == *trader.key)]
    trader_pc_vault: Box<Account<'info, TokenAccount>>,

    spl_token_program: Program<'info, SplToken>,

    dex_accounts: DexAccounts<'info>,
}

impl<'info> SwapPositionOpen<'info> {
    pub fn handle(
        &mut self,
        limit_price: DexLimitPrice,
        coin_qty: DexNonZeroTokenQty,
        leverage_factor: Factor,
    ) -> WowswapResultEmpty {
        let timestamp = UnixTimestamp::now()?;

        let max_leverage_factor = self.governance.max_leverage_factor();
        require!(
            leverage_factor >= Factor::ONE && leverage_factor <= max_leverage_factor,
            WowswapError::InvalidLeverageFactor
        );
        let coin_qty_loan = DexTokenQty::from_u128(
            leverage_factor
                .checked_sub(Factor::ONE)
                .ok_or(WowswapError::InvalidLeverageFactor)?
                .percentage_mul(coin_qty.into_inner().get() as u128),
        );
        let coin_qty = coin_qty
            .checked_add(coin_qty_loan)
            .expect("coin_qty overflow");

        let lot_sizes = dex::market_lot_sizes(&self.dex_accounts)?;
        let native_coin_qty = coin_qty
            .checked_mul_lot_size(lot_sizes.coin)
            .ok_or(WowswapError::InvalidArgument)?
            .as_token_amount();
        let pc_lot_limit_price = limit_price.checked_mul_lot_size(lot_sizes.pc);
        let native_pc_qty_loan = pc_lot_limit_price
            .and_then(|v| v.checked_mul_token_qty(coin_qty_loan))
            .ok_or(WowswapError::InvalidArgument)?;
        let native_pc_qty_including_fees = pc_lot_limit_price
            .and_then(|v| v.checked_mul_nonzero_token_qty(coin_qty))
            .ok_or(WowswapError::InvalidArgument)?;

        if native_pc_qty_loan > TokenAmount::ZERO {
            self.take_reserve_funds(native_pc_qty_loan)?;
        }

        self.take_trader_funds(
            native_pc_qty_including_fees
                .as_token_amount()
                .safe_sub(native_pc_qty_loan),
        )?;

        self.make_swap(limit_price, coin_qty, native_pc_qty_including_fees)?;
        self.swap_pc_vault.reload()?;

        if native_pc_qty_loan > TokenAmount::ZERO {
            let return_amount = std::cmp::min(
                native_pc_qty_loan,
                TokenAmount::new(self.swap_pc_vault.amount),
            );
            let native_pc_qty_loan = native_pc_qty_loan
                .checked_sub(return_amount)
                .expect("native_pc_qty_loan overflow");

            self.return_reserve_funds(return_amount)?;
            self.swap_pc_vault.reload()?;

            if native_pc_qty_loan > TokenAmount::ZERO {
                self.swap.state.total_loan = self
                    .swap
                    .state
                    .total_loan
                    .checked_add(native_pc_qty_loan)
                    .expect("total_loan overflow");
                self.position.state.loan = self
                    .position
                    .state
                    .loan
                    .checked_add(native_pc_qty_loan)
                    .expect("loan overflow");

                let pool_utilization = self.governance.pool_utilization_allowance();
                let total_debt = self.reserve.debt.get_total_debt(timestamp);
                let total_liquidity = self.reserve.get_total_liquidity(
                    total_debt,
                    TokenAmount::new(self.reserve_lendable_vault.amount),
                );
                let borrow_limit = TokenAmount::from_u128(
                    pool_utilization.percentage_mul(total_liquidity.into_inner() as u128),
                );
                require!(
                    self.swap.state.total_loan < borrow_limit,
                    WowswapError::BorrowLimitExceeded
                );

                let rate_multiplier = leverage_factor
                    .checked_sub(Factor::ONE)
                    .and_then(|v| {
                        v.checked_mul(
                            self.governance
                                .max_rate_multiplier()
                                .checked_sub(Factor::ONE)
                                .expect("invalid max_rate_multiplier"),
                        )
                    })
                    .and_then(|v| {
                        v.checked_div(
                            max_leverage_factor
                                .checked_sub(Factor::ONE)
                                .expect("invalid max_leverage_factor"),
                        )
                    })
                    .and_then(|v| v.checked_add(Factor::ONE))
                    .expect("rate_multiplier overflow");

                self.reserve_update_state(
                    timestamp,
                    total_debt,
                    native_pc_qty_loan,
                    rate_multiplier,
                );
            }
        }

        self.return_trader_funds()?;

        self.mint_proxy_token(native_coin_qty)?;

        Ok(())
    }

    fn take_reserve_funds(&self, amount: TokenAmount) -> ProgramResult {
        token::transfer(
            self.reserve_lendable_vault.to_account_info(),
            self.swap_pc_vault.to_account_info(),
            self.reserve_signer.clone(),
            amount,
            &[&[(*self.reserve).as_ref().key.as_ref(), &[self.reserve.nonce]]],
        )
    }

    fn take_trader_funds(&self, amount: TokenAmount) -> ProgramResult {
        token::transfer(
            self.trader_pc_vault.to_account_info(),
            self.swap_pc_vault.to_account_info(),
            self.trader.to_account_info(),
            amount,
            &[],
        )
    }

    fn make_swap(
        &self,
        limit_price: DexLimitPrice,
        coin_qty: DexNonZeroTokenQty,
        max_native_pc_qty_including_fees: DexNonZeroTokenAmount,
    ) -> ProgramResult {
        dex::buy(
            &self.dex_accounts,
            self.swap_coin_vault.to_account_info(),
            self.swap_pc_vault.to_account_info(),
            self.swap_signer.clone(),
            limit_price,
            coin_qty,
            max_native_pc_qty_including_fees,
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }

    fn return_reserve_funds(&self, amount: TokenAmount) -> ProgramResult {
        token::transfer(
            self.swap_pc_vault.to_account_info(),
            self.reserve_lendable_vault.to_account_info(),
            self.swap_signer.clone(),
            amount,
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }

    fn reserve_update_state(
        &mut self,
        timestamp: UnixTimestamp,
        total_debt: TokenAmount,
        amount: TokenAmount,
        rate_multiplier: Factor,
    ) {
        let reserve = &mut self.reserve;
        let governance = &self.governance;
        reserve.update_state(governance, total_debt, timestamp);

        reserve.update_borrow_rate(
            governance,
            // We did not reload `reserve_lendable_vault` after transfers, so it's ok
            TokenAmount::new(self.reserve_lendable_vault.amount),
            TokenAmount::ZERO,
            amount,
            total_debt,
            amount,
            TokenAmount::ZERO,
        );

        reserve.increase_debt(
            &mut self.position.state,
            timestamp,
            total_debt,
            amount,
            rate_multiplier,
        );
    }

    fn return_trader_funds(&self) -> ProgramResult {
        token::transfer(
            self.swap_pc_vault.to_account_info(),
            self.trader_pc_vault.to_account_info(),
            self.swap_signer.clone(),
            TokenAmount::new(self.swap_pc_vault.amount),
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }

    fn mint_proxy_token(&self, amount: TokenAmount) -> ProgramResult {
        token::mint_to(
            self.proxy_token_mint.to_account_info(),
            self.proxy_token_account.to_account_info(),
            self.swap_signer.clone(),
            amount,
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }
}

#[derive(Accounts)]
pub struct SwapPositionClose<'info> {
    #[account(
        mut,
        has_one = swap,
        has_one = trader,
        has_one = proxy_token_account,
        seeds = [
            (*swap).as_ref().key.as_ref(),
            trader.key.as_ref()
        ],
        bump = position.nonce,
    )]
    position: Box<Account<'info, SwapPosition>>,

    #[account(
        mut,
        constraint = swap.signer == *swap_signer.key,
        has_one = reserve,
        constraint = swap.coin_vault == *(*swap_coin_vault).as_ref().key,
        constraint = swap.pc_vault == *(*swap_pc_vault).as_ref().key,
        has_one = proxy_token_mint,
    )]
    swap: Box<Account<'info, Swap>>,
    swap_signer: AccountInfo<'info>,

    #[account(mut)]
    swap_coin_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    swap_pc_vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    proxy_token_mint: Box<Account<'info, TokenMint>>,
    #[account(mut)]
    proxy_token_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = reserve.signer == *reserve_signer.key,
        constraint = reserve.lendable_vault == *(*reserve_lendable_vault).as_ref().key,
    )]
    reserve: Box<Account<'info, Reserve>>,
    reserve_signer: AccountInfo<'info>,
    #[account(mut)]
    reserve_lendable_vault: Box<Account<'info, TokenAccount>>,

    #[account(constraint = *(*governance).as_ref().key == governance::ID)]
    governance: Box<Account<'info, Governance>>,

    trader: Signer<'info>,
    #[account(mut, constraint = trader_pc_vault.owner == *trader.key)]
    trader_pc_vault: Box<Account<'info, TokenAccount>>,

    spl_token_program: Program<'info, SplToken>,

    dex_accounts: DexAccounts<'info>,
}

impl<'info> SwapPositionClose<'info> {
    pub fn handle(
        &mut self,
        limit_price: DexLimitPrice,
        coin_qty: DexNonZeroTokenQty,
    ) -> WowswapResultEmpty {
        let timestamp = UnixTimestamp::now()?;

        let lot_sizes = dex::market_lot_sizes(&self.dex_accounts)?;
        let native_coin_qty = coin_qty
            .checked_mul_lot_size(lot_sizes.coin)
            .ok_or(WowswapError::InvalidArgument)?;
        let native_pc_qty_including_fees = limit_price
            .checked_mul_lot_size(lot_sizes.pc)
            .and_then(|v| v.checked_mul_nonzero_token_qty(coin_qty))
            .ok_or(WowswapError::InvalidArgument)?;

        self.burn_proxy_token(native_coin_qty.as_token_amount())?;

        self.make_swap(limit_price, coin_qty, native_pc_qty_including_fees)?;
        self.swap_pc_vault.reload()?;

        let current_debt = self.position.state.get_debt(timestamp);
        if current_debt > TokenAmount::ZERO {
            let swap_pc_vault_balance = TokenAmount::new(self.swap_pc_vault.amount);
            let (debt_change, loan_change) = if current_debt > swap_pc_vault_balance {
                let loan_change = math::liquidity::calculate_share(
                    swap_pc_vault_balance,
                    current_debt,
                    self.position.state.loan,
                );
                (swap_pc_vault_balance, loan_change)
            } else {
                (current_debt, self.position.state.loan)
            };

            self.swap.state.total_loan = self
                .swap
                .state
                .total_loan
                .checked_sub(loan_change)
                .expect("total_loan overflow");
            self.position.state.loan = self
                .position
                .state
                .loan
                .checked_sub(loan_change)
                .expect("loan overflow");

            self.return_reserve_funds(debt_change)?;
            self.swap_pc_vault.reload()?;

            self.reserve_update_state(timestamp, debt_change);
        }

        self.return_trader_funds()?;

        Ok(())
    }

    fn burn_proxy_token(&self, amount: TokenAmount) -> ProgramResult {
        token::burn(
            self.proxy_token_mint.to_account_info(),
            self.proxy_token_account.to_account_info(),
            self.swap_signer.clone(),
            amount,
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }

    fn make_swap(
        &self,
        limit_price: DexLimitPrice,
        coin_qty: DexNonZeroTokenQty,
        max_native_pc_qty_including_fees: DexNonZeroTokenAmount,
    ) -> ProgramResult {
        dex::sell(
            &self.dex_accounts,
            self.swap_coin_vault.to_account_info(),
            self.swap_pc_vault.to_account_info(),
            self.swap_signer.clone(),
            limit_price,
            coin_qty,
            max_native_pc_qty_including_fees,
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }

    fn return_reserve_funds(&self, amount: TokenAmount) -> ProgramResult {
        token::transfer(
            self.swap_pc_vault.to_account_info(),
            self.reserve_lendable_vault.to_account_info(),
            self.swap_signer.clone(),
            amount,
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }

    fn reserve_update_state(&mut self, timestamp: UnixTimestamp, debt_change: TokenAmount) {
        let reserve = &mut self.reserve;
        let governance = &self.governance;

        let total_debt = reserve.debt.get_total_debt(timestamp);
        reserve.update_state(governance, total_debt, timestamp);

        reserve.decrease_debt(&mut self.position.state, timestamp, total_debt, debt_change);

        let total_debt = reserve.debt.get_total_debt(timestamp);
        reserve.update_borrow_rate(
            governance,
            // We did not reload `reserve_lendable_vault` after transfers, so it's ok
            TokenAmount::new(self.reserve_lendable_vault.amount),
            debt_change,
            TokenAmount::ZERO,
            total_debt,
            TokenAmount::ZERO,
            TokenAmount::ZERO,
        );
    }

    fn return_trader_funds(&self) -> ProgramResult {
        token::transfer(
            self.swap_pc_vault.to_account_info(),
            self.trader_pc_vault.to_account_info(),
            self.swap_signer.clone(),
            TokenAmount::new(self.swap_pc_vault.amount),
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }
}

#[derive(Accounts)]
pub struct SwapPositionLiquidate<'info> {
    #[account(
        mut,
        has_one = swap,
        has_one = trader,
        has_one = proxy_token_account,
        seeds = [
            (*swap).as_ref().key.as_ref(),
            trader.key.as_ref()
        ],
        bump = position.nonce,
    )]
    position: Box<Account<'info, SwapPosition>>,

    #[account(
        mut,
        constraint = swap.signer == *swap_signer.key,
        has_one = reserve,
        constraint = swap.coin_vault == *(*swap_coin_vault).as_ref().key,
        constraint = swap.pc_vault == *(*swap_pc_vault).as_ref().key,
        has_one = proxy_token_mint,
    )]
    swap: Box<Account<'info, Swap>>,
    swap_signer: AccountInfo<'info>,

    #[account(mut)]
    swap_coin_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    swap_pc_vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    proxy_token_mint: Box<Account<'info, TokenMint>>,
    #[account(mut)]
    proxy_token_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = reserve.signer == *reserve_signer.key,
        constraint = reserve.lendable_vault == *(*reserve_lendable_vault).as_ref().key,
    )]
    reserve: Box<Account<'info, Reserve>>,
    reserve_signer: AccountInfo<'info>,
    #[account(mut)]
    reserve_lendable_vault: Box<Account<'info, TokenAccount>>,

    #[account(constraint = *(*governance).as_ref().key == governance::ID)]
    governance: Box<Account<'info, Governance>>,

    trader: AccountInfo<'info>,
    #[account(mut, constraint = trader_pc_vault.owner == *trader.key)]
    trader_pc_vault: Box<Account<'info, TokenAccount>>,

    liquidator: Signer<'info>,
    #[account(
        mut,
        constraint = liquidator_pc_vault.mint == trader_pc_vault.mint,
        constraint = liquidator_pc_vault.owner == *liquidator.key,
        constraint = token::check_associated_address(&liquidator_pc_vault.mint, &liquidator, &liquidator_pc_vault),
    )]
    liquidator_pc_vault: Box<Account<'info, TokenAccount>>,

    spl_token_program: Program<'info, SplToken>,

    dex_accounts: DexAccounts<'info>,
}

impl<'info> SwapPositionLiquidate<'info> {
    pub fn handle(&mut self) -> WowswapResultEmpty {
        let timestamp = UnixTimestamp::now()?;

        let limit_price = DexLimitPrice::new(1).expect("Invalid DexLimitPrice");
        let current_debt = self.position.state.get_debt(timestamp);
        let liqudation_cost = current_debt
            .checked_add(TokenAmount::from_u128(
                self.governance
                    .liquidation_margin()
                    .percentage_mul(current_debt.into_inner() as u128),
            ))
            .expect("token amount overflow");

        let lot_sizes = dex::market_lot_sizes(&self.dex_accounts)?;
        let native_coin_qty = TokenAmount::new(self.proxy_token_account.amount);
        let coin_qty = native_coin_qty
            .checked_div(TokenAmount::new(lot_sizes.coin))
            .and_then(DexNonZeroTokenQty::from_token_amount)
            .expect("invalid position");
        let native_pc_qty_including_fees = limit_price
            .checked_mul_lot_size(lot_sizes.pc)
            .and_then(|v| v.checked_mul_nonzero_token_qty(coin_qty))
            .ok_or(WowswapError::InvalidArgument)?;

        self.burn_proxy_token(native_coin_qty)?;

        self.make_swap(limit_price, coin_qty, native_pc_qty_including_fees)?;
        self.swap_pc_vault.reload()?;

        let amount_output = TokenAmount::new(self.swap_pc_vault.amount);
        if amount_output > liqudation_cost {
            msg!(
                "Trying to liquidate healthy position. Output amount: {:?}, liquidation cost: {:?}.",
                amount_output,
                liqudation_cost
            );
            return Err(WowswapError::LiquidateHealthyPosition.into());
        }

        let amount_left = self.pay_liquidation_reward(amount_output)?;
        match amount_left.checked_sub(current_debt) {
            Some(trader_amount) if !trader_amount.is_zero() => {
                self.return_reserve_funds(current_debt)?;
                self.return_trader_funds(trader_amount)?
            }
            Some(_) | None => self.return_reserve_funds(amount_left)?,
        };

        self.swap.state.total_loan = self
            .swap
            .state
            .total_loan
            .checked_sub(self.position.state.loan)
            .expect("total_loan overflow");
        self.position.state.loan = TokenAmount::ZERO;

        self.reserve_update_state(timestamp, current_debt);

        Ok(())
    }

    fn burn_proxy_token(&self, amount: TokenAmount) -> ProgramResult {
        token::burn(
            self.proxy_token_mint.to_account_info(),
            self.proxy_token_account.to_account_info(),
            self.swap_signer.clone(),
            amount,
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }

    fn make_swap(
        &self,
        limit_price: DexLimitPrice,
        coin_qty: DexNonZeroTokenQty,
        max_native_pc_qty_including_fees: DexNonZeroTokenAmount,
    ) -> ProgramResult {
        dex::sell(
            &self.dex_accounts,
            self.swap_coin_vault.to_account_info(),
            self.swap_pc_vault.to_account_info(),
            self.swap_signer.clone(),
            limit_price,
            coin_qty,
            max_native_pc_qty_including_fees,
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }

    fn pay_liquidation_reward(&self, amount: TokenAmount) -> Result<TokenAmount, ProgramError> {
        let max_reward = self.governance.max_liquidation_reward();
        let mut reward = TokenAmount::from_u128(
            self.governance
                .liquidation_reward()
                .percentage_mul(amount.into_inner() as u128),
        );
        if !max_reward.is_zero() && max_reward < reward {
            reward = max_reward;
        }

        token::transfer(
            self.swap_pc_vault.to_account_info(),
            self.liquidator_pc_vault.to_account_info(),
            self.swap_signer.clone(),
            reward,
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )?;

        Ok(amount
            .checked_sub(reward)
            .expect("liquidation amount overflow"))
    }

    fn return_reserve_funds(&self, amount: TokenAmount) -> ProgramResult {
        token::transfer(
            self.swap_pc_vault.to_account_info(),
            self.reserve_lendable_vault.to_account_info(),
            self.swap_signer.clone(),
            amount,
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }

    fn return_trader_funds(&self, amount: TokenAmount) -> ProgramResult {
        token::transfer(
            self.swap_pc_vault.to_account_info(),
            self.trader_pc_vault.to_account_info(),
            self.swap_signer.clone(),
            amount,
            &[&[(*self.swap).as_ref().key.as_ref(), &[self.swap.nonce]]],
        )
    }

    fn reserve_update_state(&mut self, timestamp: UnixTimestamp, debt_change: TokenAmount) {
        let reserve = &mut self.reserve;
        let governance = &self.governance;

        let total_debt = reserve.debt.get_total_debt(timestamp);
        reserve.update_state(governance, total_debt, timestamp);

        reserve.decrease_debt(&mut self.position.state, timestamp, total_debt, debt_change);

        let total_debt = reserve.debt.get_total_debt(timestamp);
        reserve.update_borrow_rate(
            governance,
            // We did not reload `reserve_lendable_vault` after transfers, so it's ok
            TokenAmount::new(self.reserve_lendable_vault.amount),
            debt_change,
            TokenAmount::ZERO,
            total_debt,
            TokenAmount::ZERO,
            TokenAmount::ZERO,
        );
    }
}
