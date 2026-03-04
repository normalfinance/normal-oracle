use crate::errors::NormalOracleError;
use oracle::{ errors::OracleError, state::{ HistoricalOracleData, OracleValidity } };
use sep_40_oracle::{ Asset, PriceData, PriceFeedClient };
use soroban_sdk::{ panic_with_error, Address, Env, Symbol };
// use types::oracle::OraclePriceData;
use utils::{
    constant::{ FIVE_MINUTE, PERCENTAGE_PRECISION, PERCENTAGE_PRECISION_U64, PRICE_PRECISION },
    math::safe_math::{ PrecisionMath, SafeConversion, SafeMath },
    temporal::Delay,
};

pub fn prices<F: Fn(u64) -> Option<PriceData>>(
    e: &Env,
    get_price_fn: F,
    mut records: u32,
) -> Option<Vec<PriceData>> {
    // Check if the asset is valid
    let mut timestamp = obtain_record_timestamp(e);
    if timestamp == 0 {
        return None;
    }

    let mut prices = Vec::new(e);
    let resolution = e.get_resolution() as u64;

    // Limit the number of records to 20
    records = records.min(20);

    while records > 0 {
        if let Some(price) = get_price_fn(timestamp) {
            prices.push_back(price);
        }

        // Decrement records counter in every iteration
        records -= 1;

        if timestamp < resolution {
            break;
        }
        timestamp -= resolution;
    }

    if prices.is_empty() {
        None
    } else {
        Some(prices)
    }
}

pub fn now(e: &Env) -> u64 {
    e.ledger().timestamp() * 1000 //convert to milliseconds
}

pub fn obtain_record_timestamp(e: &Env) -> u64 {
    let last_timestamp = e.get_last_timestamp();
    let ledger_timestamp = now(&e);
    let resolution = e.get_resolution() as u64;
    if
        last_timestamp == 0 || //no prices yet
        last_timestamp > ledger_timestamp || //last timestamp is in the future
        ledger_timestamp - last_timestamp >= resolution * 2
        //last timestamp is too far in the past, so we cannot return the last price
    {
        return 0;
    }
    last_timestamp
}

pub fn get_twap<F: Fn(u64) -> Option<PriceData>>(
    e: &Env,
    get_price_fn: F,
    records: u32
) -> Option<i128> {
    let prices = prices(&e, get_price_fn, records)?;

    if prices.len() != records {
        return None;
    }

    let last_price_timestamp = prices.first()?.timestamp * 1000; //convert to milliseconds to match the timestamp format
    let timeframe = e.get_resolution() as u64;
    let current_time = now(&e);

    //check if the last price is too old
    if last_price_timestamp + timeframe + 60 * 1000 < current_time {
        return None;
    }

    let sum: i128 = prices
        .iter()
        .map(|price_data| price_data.price)
        .sum();
    Some(sum / (prices.len() as i128))
}

pub fn get_x_price(
    e: &Env,
    base_asset: Asset,
    quote_asset: Asset,
    timestamp: u64,
    decimals: u32
) -> Option<PriceData> {
    let asset_pair_indexes = get_asset_pair_indexes(e, base_asset, quote_asset);
    if asset_pair_indexes.is_none() {
        return None;
    }
    get_x_price_by_indexes(e, asset_pair_indexes.unwrap(), timestamp, decimals)
}

pub fn get_x_price_by_indexes(
    e: &Env,
    asset_pair_indexes: (u8, u8),
    timestamp: u64,
    decimals: u32
) -> Option<PriceData> {
    let (base_asset, quote_asset) = asset_pair_indexes;
    //check if the asset are the same
    if base_asset == quote_asset {
        return Some(get_normalized_price_data((10i128).pow(decimals), timestamp));
    }

    //get the price for base_asset
    let base_asset_price = e.get_price(base_asset, timestamp);
    if base_asset_price.is_none() {
        return None;
    }

    //get the price for quote_asset
    let quote_asset_price = e.get_price(quote_asset, timestamp);
    if quote_asset_price.is_none() {
        return None;
    }

    //calculate the cross price
    Some(
        get_normalized_price_data(
            base_asset_price.unwrap().fixed_div_floor(quote_asset_price.unwrap(), decimals),
            timestamp
        )
    )
}

pub fn get_asset_pair_indexes(e: &Env, base_asset: Asset, quote_asset: Asset) -> Option<(u8, u8)> {
    let base_asset = e.get_asset_index(&base_asset);
    if base_asset.is_none() {
        return None;
    }

    let quote_asset = e.get_asset_index(&quote_asset);
    if quote_asset.is_none() {
        return None;
    }

    Some((base_asset.unwrap(), quote_asset.unwrap()))
}

pub fn get_price_data(e: &Env, asset: Asset, timestamp: u64) -> Option<PriceData> {
    let asset: Option<u8> = e.get_asset_index(&asset);
    if asset.is_none() {
        return None;
    }
    get_price_data_by_index(e, asset.unwrap(), timestamp)
}

pub fn get_price_data_by_index(e: &Env, asset: u8, timestamp: u64) -> Option<PriceData> {
    let price = e.get_price(asset, timestamp);
    if price.is_none() {
        return None;
    }
    Some(get_normalized_price_data(price.unwrap(), timestamp))
}

pub fn get_normalized_price_data(price: i128, timestamp: u64) -> PriceData {
    PriceData {
        price,
        timestamp: timestamp / 1000, //convert to seconds
    }
}

// pub fn get_reflector_oracle_price(
//     e: &Env,
//     oracle_addr: &Address,
//     asset: &Symbol,
//     now: u64,
// ) -> OraclePriceData {
//     let oracle_client = PriceFeedClient::new(e, oracle_addr);
//     let oracle_asset = Asset::Other(asset.clone());

//     let oracle_price: u128;
//     let published_ts: u64;

//     match oracle_client.try_lastprice(&oracle_asset) {
//         Ok(Err(_)) | Err(_) => {
//             panic_with_error!(e, NormalOracleError::FailedToGetOraclePrice);
//         }
//         Ok(Ok(result)) => {
//             let oracle_price_data = result.unwrap();

//             if oracle_price_data.price < 0 {
//                 panic_with_error!(e, OracleError::OracleNonPositive);
//             }

//             oracle_price = oracle_price_data
//                 .price
//                 .safe_to_u128(e)
//                 .safe_div(&e, PRICE_PRECISION);

//             published_ts = oracle_price_data.timestamp;

//             let oracle_delay = Delay::from_timestamp_diff_expect(e, now, published_ts);

//             OraclePriceData {
//                 price: oracle_price,
//                 delay: oracle_delay,
//             }
//         }
//     }
// }

// // Updates the time-weighted average price (TWAP) for a given asset using a new oracle price.
// //
// // The new price is first sanitized to prevent manipulation, then incorporated into the TWAP
// // using a weighted rolling average. The result is stored as updated historical oracle data.
// //
// // # Arguments
// // * `e` - Soroban environment reference.
// // * `historical_data` - The previously recorded oracle data.
// // * `oracle_price_data` - The newly observed price and timestamp.
// // * `sanitize_clamp_denominator` - Clamp denominator for price sanitization.
// // * `now` - Current timestamp.
// pub fn update_twap(
//     e: &Env,
//     historical_oracle_data: &HistoricalOracleData,
//     oracle_price_data: &OraclePriceData,
//     sanitize_clamp_denominator: u128,
//     now: u64,
// ) -> HistoricalOracleData {
//     let capped_oracle_update_price = oracle::math::sanitize_new_price(
//         e,
//         oracle_price_data.price,
//         historical_oracle_data.last_price_twap,
//         sanitize_clamp_denominator,
//     );

//     let oracle_price_twap = oracle::math::calculate_new_twap(
//         e,
//         capped_oracle_update_price,
//         now,
//         historical_oracle_data.last_price_twap,
//         historical_oracle_data.last_update_ts,
//         FIVE_MINUTE as u64,
//     );

//     let new_historical_oracle_data = HistoricalOracleData {
//         last_price_twap: oracle_price_twap.safe_to_u128(e),
//         last_price: oracle_price_data.price,
//         last_update_ts: now,
//         last_delay_ts: oracle_price_data.delay.as_seconds(),
//     };
//     crate::storage::put_historical_data(e, &new_historical_oracle_data);

//     new_historical_oracle_data
// }

// // Classifies the current oracle price data as valid, stale, or invalid.
// //
// // Uses three core checks:
// // - Price is positive
// // - Price is not too volatile relative to last TWAP
// // - Price is not too old (stale) for use in pools
// //
// // # Arguments
// // * `e` - Soroban environment reference.
// // * `last_oracle_twap` - Previous TWAP value.
// // * `oracle_price_data` - Current oracle price and timestamp.
// //
// // # Returns
// // - `OracleValidity` enum indicating the health of the oracle data.
// pub fn oracle_validity(
//     e: &Env,
//     oracle_price_data: &OraclePriceData,
//     last_oracle_twap: u128,
// ) -> OracleValidity {
//     let OraclePriceData {
//         price: oracle_price,
//         delay: oracle_delay,
//     } = *oracle_price_data;

//     // Guard rails
//     let too_volatile_ratio = crate::storage::get_too_volatile_ratio(e);
//     let seconds_before_stale = crate::storage::get_seconds_before_stale(e);

//     // NonPositive
//     let is_oracle_price_nonpositive = oracle_price <= 0;

//     // Volatility
//     // if Δprice <= 0.80 or 1.20 <= Δprice → too volatile
//     let lower_bound = PERCENTAGE_PRECISION_U64.safe_sub(e, too_volatile_ratio);
//     let upper_bound = too_volatile_ratio.safe_add(e, PERCENTAGE_PRECISION_U64);

//     // Use round-to-nearest for volatility calculation (fair assessment)
//     let price_delta = oracle_price
//         .safe_fixed_div_round(e, last_oracle_twap, PERCENTAGE_PRECISION)
//         .safe_to_u64(e);

//     let is_price_too_volatile = price_delta <= lower_bound || upper_bound <= price_delta;

//     // StaleForPair
//     let is_stale = oracle_delay.as_seconds().ge(&seconds_before_stale);

//     let oracle_validity = if is_oracle_price_nonpositive {
//         OracleValidity::NonPositive
//     } else if is_price_too_volatile {
//         OracleValidity::TooVolatile
//     } else if is_stale {
//         OracleValidity::StaleForPair
//     } else {
//         OracleValidity::Valid
//     };

//     oracle_validity
// }
