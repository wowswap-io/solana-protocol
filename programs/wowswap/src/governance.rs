use anchor_lang::prelude::*;
use std::ops::DerefMut;

use super::{
    authority,
    error::WowswapResultEmpty,
    math::{Factor, Rate, Ray, TokenAmount},
};

declare_id!("WowzN6f45eVb9nHMmKCuvq79mnGMRsd1TUWBjfyXF6T");

#[account]
#[derive(Debug, Default, Copy)]
pub struct Governance {
    pub pool_utilization_allowance: u128,
    pub base_borrow_rate: u128,
    pub excess_slope: u128,
    pub optimal_slope: u128,
    pub optimal_utilization: u128,
    pub treasure_factor: u128,
    pub max_leverage_factor: u128,
    pub max_rate_multiplier: u128,
    pub liquidation_margin: u128,
    pub liquidation_reward: u128,
    pub max_liquidation_reward: u128,
}

impl Governance {
    // 1e+18
    const ACCURACY_DIVISOR: u128 = 1_000_000_000_000_000_000;

    fn apply_accuracy(value: u128, msg: &'static str) -> u64 {
        match value.overflowing_div(Self::ACCURACY_DIVISOR).0 {
            v if v > u64::MAX as u128 => panic!("{}", msg),
            v => v as u64,
        }
    }

    pub fn pool_utilization_allowance(&self) -> Factor {
        Factor::new(Self::apply_accuracy(
            self.pool_utilization_allowance,
            "Governance::pool_utilization_allowance overflow",
        ))
    }

    pub const fn base_borrow_rate(&self) -> Rate {
        Rate::new(self.base_borrow_rate)
    }

    pub fn excess_slope(&self) -> Ray {
        Ray::new(self.excess_slope)
    }

    pub fn optimal_slope(&self) -> Ray {
        Ray::new(self.optimal_slope)
    }

    pub fn optimal_utilization(&self) -> Ray {
        Ray::new(self.optimal_utilization)
    }

    pub fn treasure_factor(&self) -> Factor {
        Factor::new(Self::apply_accuracy(
            self.treasure_factor,
            "Governance::treasure_factor overflow",
        ))
    }

    pub fn max_leverage_factor(&self) -> Factor {
        Factor::new(Self::apply_accuracy(
            self.max_leverage_factor,
            "Governance::max_leverage_factor overflow",
        ))
    }

    pub fn max_rate_multiplier(&self) -> Factor {
        Factor::new(Self::apply_accuracy(
            self.max_rate_multiplier,
            "Governance::max_rate_multiplier overflow",
        ))
    }

    pub fn liquidation_margin(&self) -> Factor {
        Factor::new(Self::apply_accuracy(
            self.liquidation_margin,
            "Governance::liquidation_margin overflow",
        ))
    }

    pub fn liquidation_reward(&self) -> Factor {
        Factor::new(Self::apply_accuracy(
            self.liquidation_reward,
            "Governance::liquidation_reward overflow",
        ))
    }

    pub fn max_liquidation_reward(&self) -> TokenAmount {
        TokenAmount::new(Self::apply_accuracy(
            self.max_liquidation_reward,
            "Governance::max_liquidation_reward overflow",
        ))
    }
}

#[derive(Accounts)]
pub struct GovernanceInitialize<'info> {
    #[account(
        init,
        payer = payer,
        constraint = *(*governance).as_ref().key == ID,
        space = 2048, // Current size is 184
    )]
    governance: Box<Account<'info, Governance>>,

    #[account(constraint = *authority.as_ref().key == authority::ID)]
    authority: Signer<'info>,

    payer: Signer<'info>,
    system_program: Program<'info, System>,
}

impl<'info> GovernanceInitialize<'info> {
    pub fn handle(&mut self, governance: Governance) -> WowswapResultEmpty {
        *(*self.governance).deref_mut() = governance;
        Ok(())
    }
}
