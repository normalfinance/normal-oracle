use sep_40_oracle::Asset;
use soroban_sdk::{ Address, contracttype };

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]

// The configuration parameters for the contract.
pub struct ConfigData {
    // The admin address.
    pub admin: Address,
    pub emergency_admin: Address,
    pub operations_admin: Address,
    pub pause_admin: Address,
    pub emergency_pause_admins: Vec<Address>,
    // The retention period for the prices.
    pub period: u64,
    // The assets supported by the contract.
    pub assets: Vec<Asset>,
    // The base asset for the prices.
    pub base_asset: Asset,
    // The number of decimals for the prices.
    pub decimals: u32,
    // The resolution of the prices.
    pub resolution: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OracleSource {
    Reflector,
}

/// Guard-rail parameters applied to raw oracle updates before they are exposed
/// to downstream consumers (Treasury, Pair, etc.).
///
/// These values define *when* a price is considered stale or unsafe, and *how*
/// aggressively new oracle prices are clamped relative to historical values.
#[contracttype]
#[derive(Copy, Clone, Debug)]
pub struct GuardRails {
    /// Maximum age (in seconds) before an oracle price is considered stale.
    ///
    /// If exceeded, consumers may reject the price or treat the oracle as unhealthy.
    pub seconds_before_stale: u64,

    /// Maximum allowed relative price change between updates, expressed in
    /// `PERCENTAGE_PRECISION_U64` units.
    ///
    /// Used to detect abnormally volatile price jumps that may indicate oracle failure
    /// or manipulation.
    pub too_volatile_ratio: u64,

    /// Controls how tightly new oracle prices are clamped to historical prices.
    ///
    /// The allowed band is:
    /// ```text
    /// last_price ± (last_price / sanitize_clamp_denominator)
    /// ```
    ///
    /// Example:
    /// - `sanitize_clamp_denominator = 10` → ±10% per update
    pub sanitize_clamp_denominator: u128,
}
