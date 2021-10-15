use anchor_lang::prelude::*;
use serum_dex::{instruction, matching, state::MarketState};
use solana_program::{entrypoint::ProgramResult, program::invoke_signed};
use std::num::NonZeroU64;

use super::{math::TokenAmount, token};

declare_id!("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin");

#[derive(Debug, Clone, Copy)]
pub struct Dex;

impl anchor_lang::AccountDeserialize for Dex {
    fn try_deserialize(buf: &mut &[u8]) -> Result<Self, ProgramError> {
        Self::try_deserialize_unchecked(buf)
    }

    fn try_deserialize_unchecked(_buf: &mut &[u8]) -> Result<Self, ProgramError> {
        Ok(Self)
    }
}

impl anchor_lang::Id for Dex {
    fn id() -> Pubkey {
        ID
    }
}

#[derive(Clone, Accounts)]
pub struct DexAccounts<'info> {
    pub dex_program: Program<'info, Dex>,

    #[account(mut)]
    pub market: AccountInfo<'info>,
    #[account(mut)]
    pub open_orders: AccountInfo<'info>,

    #[account(mut)]
    pub request_queue: AccountInfo<'info>,
    #[account(mut)]
    pub event_queue: AccountInfo<'info>,
    #[account(mut)]
    pub bids: AccountInfo<'info>,
    #[account(mut)]
    pub asks: AccountInfo<'info>,

    #[account(mut)]
    pub coin_vault: AccountInfo<'info>,
    #[account(mut)]
    pub pc_vault: AccountInfo<'info>,

    pub vault_signer: AccountInfo<'info>,
}

pub fn init_open_orders<'info>(
    dex_program: AccountInfo<'info>,
    open_orders: AccountInfo<'info>,
    owner: AccountInfo<'info>,
    market: AccountInfo<'info>,
    seeds: &[&[&[u8]]],
) -> ProgramResult {
    invoke_signed(
        &instruction_patched::init_open_orders(
            dex_program.key,
            open_orders.key,
            owner.key,
            market.key,
            None,
        )?,
        &[open_orders, owner, market],
        seeds,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn buy<'info>(
    dex: &DexAccounts<'info>,
    swap_coin_vault: AccountInfo<'info>,
    swap_pc_vault: AccountInfo<'info>,
    swap_signer: AccountInfo<'info>,
    limit_price: DexLimitPrice,
    max_coin_qty: DexNonZeroTokenQty,
    max_native_pc_qty_including_fees: DexNonZeroTokenAmount,
    seeds: &[&[&[u8]]],
) -> ProgramResult {
    make_swap(
        matching::Side::Bid,
        dex,
        swap_coin_vault,
        swap_pc_vault,
        swap_signer,
        limit_price,
        max_coin_qty,
        max_native_pc_qty_including_fees,
        seeds,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn sell<'info>(
    dex: &DexAccounts<'info>,
    swap_coin_vault: AccountInfo<'info>,
    swap_pc_vault: AccountInfo<'info>,
    swap_signer: AccountInfo<'info>,
    limit_price: DexLimitPrice,
    max_coin_qty: DexNonZeroTokenQty,
    max_native_pc_qty_including_fees: DexNonZeroTokenAmount,
    seeds: &[&[&[u8]]],
) -> ProgramResult {
    make_swap(
        matching::Side::Ask,
        dex,
        swap_coin_vault,
        swap_pc_vault,
        swap_signer,
        limit_price,
        max_coin_qty,
        max_native_pc_qty_including_fees,
        seeds,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn make_swap<'info>(
    side: matching::Side,
    dex: &DexAccounts<'info>,
    swap_coin_vault: AccountInfo<'info>,
    swap_pc_vault: AccountInfo<'info>,
    swap_signer: AccountInfo<'info>,
    limit_price: DexLimitPrice,
    max_coin_qty: DexNonZeroTokenQty,
    max_native_pc_qty_including_fees: DexNonZeroTokenAmount,
    seeds: &[&[&[u8]]],
) -> ProgramResult {
    let order_payer = match side {
        matching::Side::Bid => swap_pc_vault.clone(),
        matching::Side::Ask => swap_coin_vault.clone(),
    };

    invoke_signed(
        &instruction::new_order(
            dex.market.key,
            dex.open_orders.key,
            dex.request_queue.key,
            dex.event_queue.key,
            dex.bids.key,
            dex.asks.key,
            order_payer.key,
            swap_signer.key,
            dex.coin_vault.key,
            dex.pc_vault.key,
            &token::ID,
            &token::ID, // Should be `rent_sysvar_id` but this is not used in v0.4.0
            None,       // srm_account_referral
            dex.dex_program.key,
            side,
            limit_price.into_inner(),
            max_coin_qty.into_inner(),
            matching::OrderType::ImmediateOrCancel,
            0, // client_order_id
            instruction::SelfTradeBehavior::AbortTransaction,
            u16::MAX, // limit
            max_native_pc_qty_including_fees.into_inner(),
        )?,
        &[
            dex.market.clone(),
            dex.open_orders.clone(),
            dex.request_queue.clone(),
            dex.event_queue.clone(),
            dex.bids.clone(),
            dex.asks.clone(),
            order_payer,
            swap_signer.clone(),
            dex.coin_vault.clone(),
            dex.pc_vault.clone(),
            // spl_token_program,
            // srm_account_referral
        ],
        seeds,
    )?;

    invoke_signed(
        &instruction::settle_funds(
            dex.dex_program.key,
            dex.market.key,
            &token::ID, // spl_token_program.key,
            dex.open_orders.key,
            swap_signer.key,
            dex.coin_vault.key,
            swap_coin_vault.key,
            dex.pc_vault.key,
            swap_pc_vault.key,
            None, // referrer_pc_wallet
            dex.vault_signer.key,
        )?,
        &[
            dex.market.clone(),
            dex.open_orders.clone(),
            swap_signer,
            dex.coin_vault.clone(),
            dex.pc_vault.clone(),
            swap_coin_vault.clone(),
            swap_pc_vault.clone(),
            dex.vault_signer.clone(),
            // spl_token_program,
            // referrer_pc_wallet
        ],
        seeds,
    )
}

// v0.4.0 start use dynamic sysvars but keys still need to be passed
// Need to be reviewed before `serum-dex` update!
// https://github.com/project-serum/serum-dex/blob/v0.4.0/dex/src/instruction.rs#L909-L931
mod instruction_patched {
    use serum_dex::{error::DexError, instruction::MarketInstruction};
    use solana_program::{
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
    };

    pub fn init_open_orders(
        program_id: &Pubkey,
        open_orders: &Pubkey,
        owner: &Pubkey,
        market: &Pubkey,
        market_authority: Option<&Pubkey>,
    ) -> Result<Instruction, DexError> {
        let data = MarketInstruction::InitOpenOrders.pack();
        let mut accounts: Vec<AccountMeta> = vec![
            AccountMeta::new(*open_orders, false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(*market, false),
            // AccountMeta::new_readonly(rent::ID, false),
            AccountMeta::new_readonly(*market, false),
        ];
        if let Some(market_authority) = market_authority {
            accounts.push(AccountMeta::new_readonly(*market_authority, true));
        }
        Ok(Instruction {
            program_id: *program_id,
            data,
            accounts,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MarketLotSizes {
    pub coin: u64,
    pub pc: u64,
}

pub fn market_lot_sizes(dex_accounts: &DexAccounts) -> Result<MarketLotSizes, ProgramError> {
    let market = MarketState::load(&dex_accounts.market, dex_accounts.dex_program.key)?;
    Ok(MarketLotSizes {
        coin: market.coin_lot_size,
        pc: market.pc_lot_size,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct DexLimitPrice(NonZeroU64);

impl borsh::BorshDeserialize for DexLimitPrice {
    fn deserialize(buf: &mut &[u8]) -> borsh::maybestd::io::Result<Self> {
        let u: u64 = borsh::BorshDeserialize::deserialize(buf)?;
        NonZeroU64::new(u).map(Self).ok_or_else(|| {
            borsh::maybestd::io::Error::new(
                borsh::maybestd::io::ErrorKind::InvalidInput,
                "DexLimitPrice can not be zero",
            )
        })
    }
}

impl borsh::BorshSerialize for DexLimitPrice {
    fn serialize<W: borsh::maybestd::io::Write>(
        &self,
        writer: &mut W,
    ) -> borsh::maybestd::io::Result<()> {
        borsh::BorshSerialize::serialize(&self.0.get(), writer)
    }
}

impl DexLimitPrice {
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub fn checked_mul_lot_size(self, lot_size: u64) -> Option<DexNonZeroTokenAmount> {
        self.0
            .get()
            .checked_mul(lot_size)
            .and_then(DexNonZeroTokenAmount::new)
    }

    const fn into_inner(self) -> NonZeroU64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DexTokenQty(u64);

impl DexTokenQty {
    pub const fn new(amount: u64) -> Self {
        Self(amount)
    }

    pub fn from_u128(value: u128) -> Self {
        if value > u64::MAX as u128 {
            panic!("DexTokenQty::from_u128 overflow")
        }
        Self(value as u64)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DexNonZeroTokenQty(NonZeroU64);

impl borsh::BorshDeserialize for DexNonZeroTokenQty {
    fn deserialize(buf: &mut &[u8]) -> borsh::maybestd::io::Result<Self> {
        let u: u64 = borsh::BorshDeserialize::deserialize(buf)?;
        NonZeroU64::new(u).map(Self).ok_or_else(|| {
            borsh::maybestd::io::Error::new(
                borsh::maybestd::io::ErrorKind::InvalidInput,
                "DexNonZeroTokenQty can not be zero",
            )
        })
    }
}

impl borsh::BorshSerialize for DexNonZeroTokenQty {
    fn serialize<W: borsh::maybestd::io::Write>(
        &self,
        writer: &mut W,
    ) -> borsh::maybestd::io::Result<()> {
        borsh::BorshSerialize::serialize(&self.0.get(), writer)
    }
}

impl DexNonZeroTokenQty {
    pub const fn from_token_amount(value: TokenAmount) -> Option<Self> {
        match NonZeroU64::new(value.into_inner()) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub fn checked_add(self, other: DexTokenQty) -> Option<Self> {
        self.0
            .get()
            .checked_add(other.0)
            .and_then(NonZeroU64::new)
            .map(Self)
    }

    pub fn checked_mul_lot_size(self, other: u64) -> Option<DexNonZeroTokenAmount> {
        self.0
            .get()
            .checked_mul(other)
            .and_then(DexNonZeroTokenAmount::new)
    }

    pub const fn into_inner(self) -> NonZeroU64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DexNonZeroTokenAmount(NonZeroU64);

impl DexNonZeroTokenAmount {
    const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub fn checked_mul_token_qty(self, other: DexTokenQty) -> Option<TokenAmount> {
        self.0.get().checked_mul(other.0).map(TokenAmount::new)
    }

    pub fn checked_mul_nonzero_token_qty(
        self,
        other: DexNonZeroTokenQty,
    ) -> Option<DexNonZeroTokenAmount> {
        self.0
            .get()
            .checked_mul(other.0.get())
            .and_then(DexNonZeroTokenAmount::new)
    }

    const fn into_inner(self) -> NonZeroU64 {
        self.0
    }

    pub const fn as_token_amount(self) -> TokenAmount {
        TokenAmount::new(self.0.get())
    }
}
