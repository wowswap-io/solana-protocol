use anchor_lang::prelude::*;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, PartialOrd, AnchorDeserialize, AnchorSerialize,
)]
pub struct UnixTimestamp(u64);

impl UnixTimestamp {
    pub const ZERO: Self = UnixTimestamp::new(0);

    pub const fn new(inner: u64) -> Self {
        Self(inner)
    }

    pub fn now() -> Result<Self, ProgramError> {
        Ok(Self(Clock::get()?.unix_timestamp as u64))
    }

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub const fn into_inner(self) -> u64 {
        self.0
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, AnchorDeserialize, AnchorSerialize,
)]
pub struct TokenAmount(u64);

impl TokenAmount {
    pub const ZERO: Self = TokenAmount::new(0);

    pub const fn new(inner: u64) -> Self {
        Self(inner)
    }

    pub fn from_u128(value: u128) -> Self {
        assert!(value <= u64::MAX as u128, "TokenAmount::from_u128 overflow");
        Self::new(value as u64)
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    pub fn checked_div(self, other: Self) -> Option<Self> {
        self.0.checked_div(other.0).map(Self)
    }

    pub fn safe_sub(self, other: Self) -> Self {
        match self.0.overflowing_sub(other.0) {
            (v, false) => Self(v),
            (_, true) => panic!("safe_sub"),
        }
    }

    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub const fn into_wad(self) -> Wad {
        Wad::from_u64(self.0)
    }

    pub const fn into_ray(self) -> Ray {
        Ray::from_u64(self.0)
    }

    pub const fn into_inner(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, AnchorDeserialize, AnchorSerialize)]
pub struct Rate(u128);

impl Rate {
    pub const ZERO: Self = Rate::new(0);
    pub const RAY_RATIO: u128 = 1_000_000_000;

    pub const fn new(inner: u128) -> Self {
        Self(inner)
    }

    pub const fn into_ray(self) -> Ray {
        Ray::new(self.0.overflowing_div(Self::RAY_RATIO).0)
    }

    pub const fn into_inner(self) -> u128 {
        self.0
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, PartialOrd, AnchorDeserialize, AnchorSerialize,
)]
pub struct Factor(u64);

impl Factor {
    pub const ONE: Self = Factor::new(10_000);
    const HALF: Self = Factor::new(5_000);

    pub const fn new(inner: u64) -> Self {
        Self(inner)
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    pub fn checked_mul(self, other: Self) -> Option<Self> {
        self.0.checked_mul(other.0).map(Self)
    }

    pub fn checked_div(self, other: Self) -> Option<Self> {
        self.0.checked_div(other.0).map(Self)
    }

    pub fn percentage_mul(self, value: u128) -> u128 {
        value
            .checked_mul(self.0 as u128)
            .and_then(|v| v.checked_add(Self::HALF.0 as u128))
            .and_then(|v| v.checked_div(Self::ONE.0 as u128))
            .expect("Factor::percentage_mul overflow")
    }

    pub fn invert(self) -> Self {
        Self::ONE
            .checked_sub(self)
            .expect("Factor::invert overflow")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Wad(u128);

impl Wad {
    // 1e+9
    pub const ONE: Self = Self::new(1_000_000_000);
    // 0.5e+9
    const HALF: Self = Self::new(500_000_000);

    pub const fn new(inner: u128) -> Self {
        Self(inner)
    }

    const fn from_u64(value: u64) -> Self {
        Self::new(value as u128)
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    pub fn checked_mul(self, other: Self) -> Option<Self> {
        self.0.checked_mul(other.0).map(Self)
    }

    pub fn checked_div(self, other: Self) -> Option<Self> {
        self.0.checked_div(other.0).map(Self)
    }

    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    // (a * b + HALF_WAD) / WAD
    pub fn wad_mul(self, other: Self) -> Self {
        self.checked_mul(other)
            .and_then(|v| v.checked_add(Self::HALF))
            .and_then(|v| v.checked_div(Self::ONE))
            .expect("Wad::wad_mul overflow")
    }

    // (a * WAD + b / 2) / b
    pub fn wad_div(self, other: Self) -> Self {
        self.checked_mul(Self::ONE)
            .and_then(|v| {
                let two = Wad::new(2);
                v.checked_add(other.checked_div(two).expect("division by zero"))
            })
            .and_then(|v| v.checked_div(other))
            .expect("Wad::wad_div overflow")
    }

    // a * 1e+9
    pub fn into_ray(self) -> Ray {
        Ray::new(
            (self.0 as u128)
                .checked_mul(1_000_000_000)
                .expect("Wad::into_ray overflow"),
        )
    }

    pub fn as_token_amount(self) -> TokenAmount {
        TokenAmount::from_u128(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Ray(u128);

impl Ray {
    // 1e+18
    pub const ONE: Self = Self::new(1_000_000_000_000_000_000);
    // 0.5e+18
    const HALF: Self = Self::new(500_000_000_000_000_000);

    pub const fn new(inner: u128) -> Self {
        Self(inner)
    }

    pub const fn from_u64(value: u64) -> Self {
        Self::new(value as u128)
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    pub fn checked_mul(self, other: Self) -> Option<Self> {
        self.0.checked_mul(other.0).map(Self)
    }

    pub fn checked_div(self, other: Self) -> Option<Self> {
        self.0.checked_div(other.0).map(Self)
    }

    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    // (a * b + HALF_RAY) / RAY
    pub fn ray_mul(self, other: Self) -> Self {
        self.checked_mul(other)
            .and_then(|v| v.checked_add(Self::HALF))
            .and_then(|v| v.checked_div(Self::ONE))
            .expect("Ray::ray_mul overflow")
    }

    // (a * RAY + b / 2) / b
    pub fn ray_div(self, other: Self) -> Self {
        self.checked_mul(Self::ONE)
            .and_then(|v| {
                let two = Ray::new(2);
                v.checked_add(other.checked_div(two).expect("division by zero"))
            })
            .and_then(|v| v.checked_div(other))
            .expect("Ray::ray_div overflow")
    }

    pub fn invert(self) -> Self {
        Self::ONE.checked_sub(self).expect("Ray::invert overflow")
    }

    pub fn as_token_amount(self) -> TokenAmount {
        TokenAmount::from_u128(self.0)
    }

    pub fn as_rate(self) -> Rate {
        Rate::new(
            self.0
                .checked_mul(Rate::RAY_RATIO)
                .expect("Ray::as_rate overflow"),
        )
    }

    pub const fn into_inner(self) -> u128 {
        self.0
    }
}

pub mod liquidity {
    use super::{TokenAmount, Wad};

    pub fn mint_amount(
        amount: TokenAmount,
        total_supply: TokenAmount,
        total_liquidity: TokenAmount,
    ) -> TokenAmount {
        let index = if total_supply.is_zero() || total_liquidity.is_zero() {
            Wad::ONE
        } else {
            total_supply.into_wad().wad_div(total_liquidity.into_wad())
        };
        amount.into_wad().wad_mul(index).as_token_amount()
    }

    pub fn calculate_share(
        partion: TokenAmount,
        total: TokenAmount,
        total_liquidity: TokenAmount,
    ) -> TokenAmount {
        let share = if total.is_zero() {
            Wad::new(0)
        } else {
            partion.into_wad().wad_div(total.into_wad())
        };
        share.wad_mul(total_liquidity.into_wad()).as_token_amount()
    }
}

pub mod interest {
    use super::{Rate, Ray, TokenAmount, UnixTimestamp};

    // Calculate the interest using a compounded interest rate formula in RAY.
    // To avoid expensive exponentiation, the calculation is performed using a binomial approximation:
    // (1+x)^n = 1+n*x+[n/2*(n-1)]*x^2+[n/6*(n-1)*(n-2)*x^3...
    pub fn calculate_compounded(
        rate: Rate,
        last_timestamp: UnixTimestamp,
        timestamp: UnixTimestamp,
    ) -> Ray {
        let rate_ray = rate.into_ray();
        let mut result = Ray::ONE;

        let exp = timestamp
            .checked_sub(last_timestamp)
            .expect("Invalid timestamps");
        if exp.is_zero() {
            return result;
        }

        let mut el = rate_ray
            .checked_mul(Ray::from_u64(exp.into_inner()))
            .expect("compounded overflow");
        result = result.checked_add(el).expect("compounded overflow");
        for i in 1..5 {
            let multiplier = match exp.checked_sub(UnixTimestamp::new(i)) {
                None => break,
                Some(exp) if exp == UnixTimestamp::ZERO => break,
                Some(exp) => exp,
            };

            // el = raymul_u128(rate, el * (exp - i)) / (i + 1)
            el = el
                .checked_mul(Ray::from_u64(multiplier.into_inner()))
                .expect("compounded overflow");
            el = rate_ray
                .ray_mul(el)
                .checked_div(Ray::from_u64(i + 1))
                .expect("compounded overflow");
            result = result.checked_add(el).expect("compounded overflow");
        }
        result
    }

    // Calculate utilization rate based on current debt and available liquidity.
    fn calculate_utilization(debt: TokenAmount, liquidity: TokenAmount) -> Ray {
        debt.into_ray().ray_div(
            liquidity
                .into_ray()
                .checked_add(debt.into_ray())
                .expect("utilization rate overflow"),
        )
    }

    pub fn borrow_rate(
        debt: TokenAmount,
        liquidity: TokenAmount,
        base_borrow_rate: Rate,
        excess_slope: Ray,
        optimal_slope: Ray,
        optimal_utilization: Ray,
    ) -> Rate {
        let utilization = calculate_utilization(debt, liquidity);
        match utilization.checked_sub(optimal_utilization) {
            // utilization >= optimal_utilization
            Some(diff) if !diff.is_zero() => {
                // Utilization is too high, so calculate rate based on excess slope.
                base_borrow_rate
                    .into_ray()
                    .checked_add(optimal_slope)
                    .and_then(|v| {
                        let excess_rate_ratio = diff.ray_div(optimal_utilization.invert());
                        let extra = excess_slope.ray_mul(excess_rate_ratio);
                        v.checked_add(extra)
                    })
            }
            // utilization < optimal_utilization
            Some(_) | None => {
                // Utilization is okay, so calculate rate based on optimal slope.
                base_borrow_rate
                    .into_ray()
                    .checked_add(optimal_slope.ray_mul(utilization.ray_div(optimal_utilization)))
            }
        }
        .expect("borrow_rate overflow")
        .as_rate()
    }
}
