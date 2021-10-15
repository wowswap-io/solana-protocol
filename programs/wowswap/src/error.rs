use anchor_lang::prelude::*;

pub type WowswapResult<T> = Result<T>;
pub type WowswapResultEmpty = Result<()>;

#[error]
pub enum WowswapError {
    InvalidArgument = 0,
    InvalidMint,
    InvalidLeverageFactor,
    BorrowLimitExceeded,
    LiquidateHealthyPosition,
}
