use anchor_lang::prelude::*;

pub mod dex;
pub mod error;
pub mod governance;
pub mod math;
pub mod reserve;
pub mod swap;
pub mod token;

use dex::{DexLimitPrice, DexNonZeroTokenQty};
use error::WowswapResultEmpty;
use governance::*;
use math::{Factor, TokenAmount};
use reserve::*;
use swap::*;

pub mod authority {
    use super::declare_id;

    declare_id!("WowY47CddJnybWZkWmcCX5t8mQZnGVpyHXKjL6Tb279");
}

declare_id!("Wow1snUDtX9HME1tb3NhAaNwFSvJxsKNQKiYGQqkG6q");

#[program]
pub mod wowswap {
    use super::*;

    pub fn governance_initialize(
        ctx: Context<GovernanceInitialize>,
        governance: Governance,
    ) -> WowswapResultEmpty {
        ctx.accounts.handle(governance)
    }

    pub fn reserve_initialize(ctx: Context<ReserveInitialize>, nonce: u8) -> WowswapResultEmpty {
        ctx.accounts.handle(nonce)
    }

    pub fn reserve_deposit(
        ctx: Context<ReserveDeposit>,
        amount: TokenAmount,
    ) -> WowswapResultEmpty {
        ctx.accounts.handle(amount)
    }

    pub fn reserve_withdraw(
        ctx: Context<ReserveWithdraw>,
        amount: TokenAmount,
    ) -> WowswapResultEmpty {
        ctx.accounts.handle(amount)
    }

    pub fn swap_initialize(ctx: Context<SwapInitialize>, nonce: u8) -> WowswapResultEmpty {
        ctx.accounts.handle(nonce)
    }

    pub fn swap_position_initialize(
        ctx: Context<SwapPositionInitialize>,
        nonce: u8,
    ) -> WowswapResultEmpty {
        ctx.accounts.handle(nonce)
    }

    pub fn swap_position_open(
        ctx: Context<SwapPositionOpen>,
        limit_price: DexLimitPrice,
        coin_qty: DexNonZeroTokenQty,
        leverage_factor: Factor,
    ) -> WowswapResultEmpty {
        ctx.accounts.handle(limit_price, coin_qty, leverage_factor)
    }

    pub fn swap_position_close(
        ctx: Context<SwapPositionClose>,
        limit_price: DexLimitPrice,
        coin_qty: DexNonZeroTokenQty,
    ) -> WowswapResultEmpty {
        ctx.accounts.handle(limit_price, coin_qty)
    }

    pub fn swap_position_liquidate(ctx: Context<SwapPositionLiquidate>) -> WowswapResultEmpty {
        ctx.accounts.handle()
    }
}
