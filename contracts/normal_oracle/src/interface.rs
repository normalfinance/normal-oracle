use stellar_axelar_gateway::executable::AxelarExecutableInterface;
use stellar_axelar_std::types::Token;
use stellar_axelar_std::{Address, Env, String};

use oracle::state::HistoricalOracleData;
use soroban_sdk::{ Address, Env, Map, Symbol, Vec };
use types::oracle::OraclePriceData;


use crate::storage::GuardRails;

pub trait NormalOracleTrait {
    fn get_guard_rails(e: Env) -> GuardRails;

    fn get_privileged_addrs(e: Env) -> Map<Symbol, Vec<Address>>;
}

pub trait AdminInterface {
    fn set_seconds_before_stale(e: Env, admin: Address, stale_limit: u64);

    fn set_too_volatile_ratio(e: Env, admin: Address, too_volatile_ratio: u64);

    fn set_sanitize_clamp_denominator(e: Env, admin: Address, sanitize_clamp_denominator: u128);

    fn set_privileged_addrs(
        e: Env,
        admin: Address,
        operations_admin: Address,
        pause_admin: Address,
        emergency_pause_admins: Vec<Address>
    );
}

pub trait AxelarGMPInterface: AxelarExecutableInterface {
    /// Retrieves the address of the gas service.
    fn gas_service(env: &Env) -> Address;

    /// Sends a message to a specified destination chain.
    ///
    /// The function also handles the payment of gas for the cross-chain transaction.
    ///
    /// # Arguments
    /// * caller - The address of the caller initiating the message.
    /// * destination_chain - The name of the destination chain where the message will be sent.
    /// * destination_address - The address on the destination chain where the message will be sent.
    /// * message - The message to be sent.
    /// * gas_token - An optional gas token used to pay for gas during the transaction.
    ///
    /// # Authorization
    /// - The caller must authorize.
    fn send(
        env: &Env,
        caller: Address,
        destination_chain: String,
        destination_address: String,
        message: String,
        gas_token: Option<Token>,
    );

    /// Returns the most recently received message.
    fn received_message(env: &Env) -> String;
}
