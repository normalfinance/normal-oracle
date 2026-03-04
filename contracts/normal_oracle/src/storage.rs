use oracle::state::HistoricalOracleData;
use paste::paste;
use soroban_sdk::{contracttype, panic_with_error, Address, Env, Symbol};
use types::oracle::{OraclePriceData, OracleSource};
use utils::bump::{bump_instance, bump_persistent};
use utils::constant::{FIVE_MINUTE, PERCENTAGE_PRECISION_U64};
use utils::errors::storage_errors::StorageError;
use utils::{
    generate_instance_storage_getter, generate_instance_storage_getter_and_setter,
    generate_instance_storage_getter_and_setter_with_default,
    generate_instance_storage_getter_with_default, generate_instance_storage_setter,
};
use utils::{
    generate_persistent_storage_getter, generate_persistent_storage_getter_and_setter,
    generate_persistent_storage_getter_and_setter_with_default,
    generate_persistent_storage_getter_with_default, generate_persistent_storage_setter,
};

/********** Storage Key Types **********/

// Instance-scoped keys (global to this oracle proxy instance).
const KEY_SANITIZE_CLAMP_DENOMINATOR: &str = "SanitizeClampDenominator";
const KEY_STALE: &str = "SecondsBeforeStale";
const KEY_VOLATILE: &str = "TooVolatileRatio";

/// Persistent data keys for historical oracle state.
///
/// Historical data is kept in persistent storage so it survives across
/// ledger boundaries and can be used for TWAPs, volatility checks, and clamping.
#[derive(Clone)]
#[contracttype]
pub enum DataKey { 
    /// Axelar: Used to store the address of the Axelar Gateway contract
    Gateway,
    /// Axelar: Used to store the address of the Axelar Gas Service contract
    GasService,
    /// Axelar: Used to store message data received from other chains
    ReceivedMessage,
    
    ///
    Decimals,
    ///
    Resolution,
    ///
    BaseAsset,
    /// 
    Period
}

/********** Storage **********/

// new
generate_persistent_storage_getter_and_setter_with_default!(
    decimals,
    DataKey::Decimals,
    u32,
    7
);
generate_persistent_storage_getter_and_setter_with_default!(
    resolution,
    DataKey::Resolution,
    u32,
    500
);

// old

generate_instance_storage_getter_and_setter!(asset, KEY_ASSET, Symbol);
generate_instance_storage_getter_and_setter!(oracle, KEY_ORACLE, Address);
generate_instance_storage_getter_and_setter!(oracle_source, KEY_ORACLE_SOURCE, OracleSource);
generate_instance_storage_getter_and_setter_with_default!(
    sanitize_clamp_denominator,
    KEY_SANITIZE_CLAMP_DENOMINATOR,
    u128,
    10 // ±10% allowed price move per update
);
generate_instance_storage_getter_and_setter_with_default!(
    seconds_before_stale,
    KEY_STALE,
    u64,
    FIVE_MINUTE as u64
);

// Maximum allowed relative price change between oracle updates.
//
// Expressed in `PERCENTAGE_PRECISION_U64` units.
// Default: ±20%.
generate_instance_storage_getter_and_setter_with_default!(
    too_volatile_ratio,
    KEY_VOLATILE,
    u64,
    PERCENTAGE_PRECISION_U64 / 5 // ±20%
);

// Historical Data

/// Loads the stored [`HistoricalOracleData`] for this oracle proxy.
///
/// If no historical data exists yet (first update), this function initializes
/// the history using the provided `oracle_price_data` and current timestamp.
///
/// ### Arguments
/// - `oracle_price_data`: The latest raw price data from the upstream oracle.
/// - `now`: Current ledger timestamp (seconds).
///
/// ### Returns
/// A fully-initialized [`HistoricalOracleData`] struct suitable for:
/// - TWAP calculations
/// - volatility checks
/// - price clamping
///
/// ### Notes
/// - Historical data is stored in persistent storage and TTL-bumped on access.
/// - This function never reverts for missing history; it deterministically
///   bootstraps from the first observed price.
pub(crate) fn get_historical_data(
    e: &Env,
    oracle_price_data: &OraclePriceData,
    now: u64,
) -> HistoricalOracleData {
    let key = DataKey::HistoricalData;
    match e.storage().persistent().get(&key) {
        Some(data) => {
            bump_persistent(e, &key);
            data
        }
        None => HistoricalOracleData::default(*oracle_price_data, now),
    }
}

pub(crate) fn get_historical_data_raw(e: &Env) -> HistoricalOracleData {
    let key = DataKey::HistoricalData;
    match e.storage().persistent().get(&key) {
        Some(data) => {
            bump_persistent(e, &key);
            data
        }
        None => panic_with_error!(e, StorageError::ValueNotInitialized),
    }
}

/// Persists updated [`HistoricalOracleData`] after a successful oracle update.
///
/// Callers are responsible for ensuring:
/// - the data has passed all staleness, volatility, and clamp checks
/// - timestamps are monotonically increasing
///
/// This function only performs storage + TTL bumping.
pub(crate) fn put_historical_data(e: &Env, oracle_data: &HistoricalOracleData) {
    let key = DataKey::HistoricalData;
    e.storage().persistent().set(&key, oracle_data);
    bump_persistent(e, &key);
}
