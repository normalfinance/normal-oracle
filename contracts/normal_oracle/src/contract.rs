use crate::errors::NormalOracleError;
use crate::interface::{ AdminInterface, NormalOracleTrait };
use crate::storage::GuardRails;
use crate::types::ConfigData;
use access_control::utils::require_operations_admin_or_owner;
use oracle::errors::OracleError;
use oracle::state::{ HistoricalOracleData, OracleValidity };
use soroban_sdk::{
    contract,
    contractimpl,
    contractmeta,
    log,
    panic_with_error,
    Address,
    BytesN,
    Env,
    Map,
    Symbol,
    Vec,
};
use types::oracle::{ OraclePriceData, OracleSource };

use sep_40_oracle::{ Asset, PriceFeedTrait };

// Access control
use access_control::access::{ AccessControl, AccessControlTrait };
use access_control::emergency::{ get_emergency_mode, set_emergency_mode };
use access_control::errors::AccessControlError;
use access_control::events::Events as AccessControlEvents;
use access_control::interface::TransferableContract;
use access_control::management::{ MultipleAddressesManagementTrait, SingleAddressManagementTrait };
use access_control::role::{ Role, SymbolRepresentation };
use access_control::transfer::TransferOwnershipTrait;

// Upgrade
use upgrade::events::Events as UpgradeEvents;
use upgrade::interface::UpgradeableContract;
use upgrade::{ apply_upgrade, commit_upgrade, revert_upgrade };
use utils::constant::{ PERCENTAGE_PRECISION_U64, TWENTY_FOUR_HOUR };

contractmeta!(
    key = "Description",
    val = "A standardized oracle supporting multiple providers and risk measures"
);

#[contract]
pub struct NormalOracle;

#[contractimpl]
impl NormalOracle {
    // __constructor
    // Initializes the oracle by setting the admin roles and storing critical parameters.
    //
    // Arguments:
    //   - e: The Soroban environment.
    //   - config: The address to be assigned the Admin role.
    pub fn __constructor(
        e: Env,
        config: ConfigData
    ) {
        let access_control = AccessControl::new(&e);
        if access_control.get_role_safe(&Role::Admin).is_some() {
            panic_with_error!(&e, NormalOracleError::AlreadyInitialized);
        }
        access_control.set_role_address(&Role::Admin, &config.admin);
        access_control.set_role_address(&Role::EmergencyAdmin, &config.emergency_admin);
        access_control.set_role_address(&Role::PauseAdmin, &config.pause_admin);
        access_control.set_role_addresses(&Role::EmergencyPauseAdmin, &config.emergency_pause_admins);
        access_control.set_role_address(&Role::OperationsAdmin, &config.operations_admin);

  
        crate::storage::set_base_asset(&config.base_asset);
        crate::storage::set_decimals(config.decimals);
        crate::storage::set_resolution(config.resolution);
        crate::storage::set_retention_period(config.period);

        env.storage().instance().set(&DataKey::Gateway, &gateway);
        env.storage().instance().set(&DataKey::GasService, &gas_service);
    }
}

impl CustomAxelarExecutable for NormalOracle {
    type Error = AxelarGMPError;

    fn __gateway(env: &Env) -> Address {
        env.storage().instance().get(&DataKey::Gateway).unwrap()
    }

    fn __execute(
        env: &Env,
        source_chain: String,
        message_id: String,
        source_address: String,
        payload: Bytes,
    ) -> Result<(), Self::Error> {
        let decoded_msg = abi_decode_string(env, payload.clone()).map_err(|_| AxelarGMPError::FailedDecoding)?;

        // Store the received message
        env.storage().instance().set(&DataKey::ReceivedMessage, &decoded_msg);

        // Emit event
        ExecutedEvent {
            source_chain,
            message_id,
            source_address,
            payload,
        }
        .emit(env);

        Ok(())
    }
}

#[contractimpl]
impl AxelarGMPInterface for NormalOracle {
    fn gas_service(e: &Env) -> Address {
        crate::storage::get_gas_service(&e)
    }

    fn send(
        e: &Env,
        caller: Address,
        destination_chain: String,
        destination_address: String,
        message: String,
        gas_token: Option<Token>,
    ) {
        let gateway = AxelarGatewayMessagingClient::new(e, &Self::gateway(e));
        let gas_service = AxelarGasServiceClient::new(e, &Self::gas_service(e));

        caller.require_auth();

        let encoded_msg = abi_encode(e, message).unwrap();

        if let Some(gas_token) = gas_token {
            gas_service.pay_gas(
                &e.current_contract_address(),
                &destination_chain,
                &destination_address,
                &encoded_msg,
                &caller,
                &gas_token,
                &Bytes::new(e),
            );
        }

        gateway.call_contract(
            &e.current_contract_address(),
            &destination_chain,
            &destination_address,
            &encoded_msg,
        );
    }

    fn received_message(e: &Env) -> String {
        env.storage().instance().get(&DataKey::ReceivedMessage)
            .unwrap_or_else(|| String::from_str(env, ""))
    }
}

#[contractimpl]
impl PriceFeedTrait for NormalOracle {
    /// Return the base asset the price is reported in
    fn base(e: Env) -> Asset {
        crate::storage::get_base_asset(&e)
    }

    /// Return all assets quoted by the price feed
    fn assets(e: Env) -> Vec<Asset> {
        crate::storage::get_assets(&e)
    }

    /// Return the number of decimals for all assets quoted by the oracle
    fn decimals(e: Env) -> u32 {
        crate::storage::get_decimals(&e)
    }

    /// Return default tick period timeframe (in seconds)
    fn resolution(e: Env) -> u32 {
        crate::storage::get_resolution(&e) / 1000
    }

    /// Get price in base asset at specific timestamp
    fn price(e: Env, asset: Asset, timestamp: u64) -> Option<PriceData> {
        let current_time = e.ledger().timestamp();

        let oracle_price_data = match crate::storage::get_oracle_source(&e) {
            OracleSource::Reflector =>
                crate::oracle::get_reflector_oracle_price(
                    &e,
                    &crate::storage::get_oracle(&e),
                    &crate::storage::get_asset(&e),
                    current_time
                ),
        };

        oracle_price_data
    }

    /// Get last N price records
    fn prices(env: Env, asset: Asset, records: u32) -> Option<Vec<PriceData>> {
        crate::storage::get_historical_data_raw(&e)
    }

    /// Get the most recent price for an asset
    fn lastprice(env: Env, asset: Asset) -> Option<PriceData> {
        crate::storage::get_historical_data_raw(&e)
    }
}

#[contractimpl]
impl NormalOracleTrait for NormalOracle {
    fn get_guard_rails(e: Env) -> GuardRails {
        GuardRails {
            seconds_before_stale: crate::storage::get_seconds_before_stale(&e),
            too_volatile_ratio: crate::storage::get_too_volatile_ratio(&e),
            sanitize_clamp_denominator: crate::storage::get_sanitize_clamp_denominator(&e),
        }
    }

    // Returns a map of privileged roles.
    //
    // # Returns
    //
    // A map of privileged roles to their respective addresses.
    fn get_privileged_addrs(e: Env) -> Map<Symbol, Vec<Address>> {
        let access_control = AccessControl::new(&e);
        let mut result: Map<Symbol, Vec<Address>> = Map::new(&e);
        for role in [Role::Admin, Role::EmergencyAdmin, Role::OperationsAdmin, Role::PauseAdmin] {
            result.set(role.as_symbol(&e), match access_control.get_role_safe(&role) {
                Some(v) => Vec::from_array(&e, [v]),
                None => Vec::new(&e),
            });
        }

        result.set(
            Role::EmergencyPauseAdmin.as_symbol(&e),
            access_control.get_role_addresses(&Role::EmergencyPauseAdmin)
        );

        result
    }

    // fn update_price(e: Env) -> HistoricalOracleData {
    //     let current_time = e.ledger().timestamp();
    //     let asset = crate::storage::get_asset(&e);
    //     let oracle_addr = crate::storage::get_oracle(&e);
    //     let oracle_source = crate::storage::get_oracle_source(&e);

    //     let oracle_price_data = match oracle_source {
    //         OracleSource::Reflector => {
    //             crate::oracle::get_reflector_oracle_price(&e, &oracle_addr, &asset, current_time)
    //         }
    //     };

    //     let historical_oracle_data = crate::storage::get_historical_data(
    //         &e,
    //         &oracle_price_data, // fallback
    //         current_time
    //     );

    //     let oracle_validity = crate::oracle::oracle_validity(
    //         &e,
    //         &oracle_price_data,
    //         historical_oracle_data.last_price_twap
    //     );

    //     if oracle_validity != OracleValidity::Valid {
    //         log!(&e, "oracle_validity", oracle_validity);
    //         panic_with_error!(&e, OracleError::OracleInvalid);
    //     }

    //     crate::oracle::update_twap(
    //         &e,
    //         &historical_oracle_data,
    //         &oracle_price_data,
    //         crate::storage::get_sanitize_clamp_denominator(&e),
    //         current_time
    //     )
    // }
}

#[contractimpl]
impl AdminInterface for NormalOracle {
    fn set_seconds_before_stale(e: Env, admin: Address, stale_limit: u64) {
        admin.require_auth();
        require_operations_admin_or_owner(&e, &admin);

        if stale_limit == 0 || stale_limit > TWENTY_FOUR_HOUR {
            panic_with_error!(&e, NormalOracleError::InvalidInput);
        }

        crate::storage::set_seconds_before_stale(&e, &stale_limit);
    }

    fn set_too_volatile_ratio(e: Env, admin: Address, too_volatile_ratio: u64) {
        admin.require_auth();
        require_operations_admin_or_owner(&e, &admin);

        if too_volatile_ratio > PERCENTAGE_PRECISION_U64 {
            panic_with_error!(&e, NormalOracleError::InvalidInput);
        }

        crate::storage::set_too_volatile_ratio(&e, &too_volatile_ratio);
    }

    fn set_sanitize_clamp_denominator(e: Env, admin: Address, sanitize_clamp_denominator: u128) {
        admin.require_auth();
        require_operations_admin_or_owner(&e, &admin);

        if sanitize_clamp_denominator > 10_000 {
            panic_with_error!(&e, NormalOracleError::InvalidInput);
        }

        crate::storage::set_sanitize_clamp_denominator(&e, &sanitize_clamp_denominator);
    }

    // Sets the privileged addresses.
    //
    // # Arguments
    //
    // * `admin` - The address of the admin.
    // * `operations_admin` - The address of the operations admin.
    // * `pause_admin` - The address of the pause admin.
    // * `emergency_pause_admin` - The addresses of the emergency pause admins.
    fn set_privileged_addrs(
        e: Env,
        admin: Address,
        operations_admin: Address,
        pause_admin: Address,
        emergency_pause_admins: Vec<Address>
    ) {
        admin.require_auth();
        let access_control = AccessControl::new(&e);
        access_control.assert_address_has_role(&admin, &Role::Admin);

        access_control.set_role_address(&Role::OperationsAdmin, &operations_admin);
        access_control.set_role_address(&Role::PauseAdmin, &pause_admin);
        access_control.set_role_addresses(&Role::EmergencyPauseAdmin, &emergency_pause_admins);
        AccessControlEvents::new(&e).set_oracle_privileged_addrs(
            operations_admin,
            pause_admin,
            emergency_pause_admins
        );
    }
}

#[contractimpl]
impl UpgradeableContract for NormalOracle {
    // version
    // Returns the current version number of the contract.
    //
    // Returns:
    //   - A u32 representing the version.
    fn version() -> u32 {
        110
    }

    // Get contract type symbolic name
    fn contract_name(e: Env) -> Symbol {
        Symbol::new(&e, "NormalOracle")
    }

    // commit_upgrade
    // Commits a new WASM hash as a pending upgrade.
    //
    // Arguments:
    //   - e: The Soroban environment.
    //   - admin: The admin address (must be authorized).
    //   - new_wasm_hash: The new WASM hash (BytesN<32>) to be committed.
    fn commit_upgrade(e: Env, admin: Address, new_wasm_hash: BytesN<32>) {
        admin.require_auth();
        AccessControl::new(&e).assert_address_has_role(&admin, &Role::Admin);
        commit_upgrade(&e, &new_wasm_hash);
        UpgradeEvents::new(&e).commit_upgrade(Vec::from_array(&e, [new_wasm_hash.clone()]));
    }

    // apply_upgrade
    // Applies the previously committed upgrade.
    //
    // Arguments:
    //   - e: The Soroban environment.
    //   - admin: The admin address (must be authorized).
    //
    // Returns:
    //   - The new WASM hash (BytesN<32>) that was applied.
    fn apply_upgrade(e: Env, admin: Address) -> BytesN<32> {
        admin.require_auth();
        AccessControl::new(&e).assert_address_has_role(&admin, &Role::Admin);
        let new_wasm_hash = apply_upgrade(&e);
        UpgradeEvents::new(&e).apply_upgrade(Vec::from_array(&e, [new_wasm_hash.clone()]));
        new_wasm_hash
    }

    // revert_upgrade
    // Reverts a pending upgrade that has not yet been applied.
    //
    // Arguments:
    //   - e: The Soroban environment.
    //   - admin: The admin address (must be authorized).
    fn revert_upgrade(e: Env, admin: Address) {
        admin.require_auth();
        AccessControl::new(&e).assert_address_has_role(&admin, &Role::Admin);
        revert_upgrade(&e);
        UpgradeEvents::new(&e).revert_upgrade();
    }

    // set_emergency_mode
    // Sets or unsets emergency mode for instant upgrades.
    //
    // Arguments:
    //   - e: The Soroban environment.
    //   - emergency_admin: The emergency admin address (must be authorized).
    //   - value: Boolean indicating whether to enable (true) or disable (false) emergency mode.
    fn set_emergency_mode(e: Env, emergency_admin: Address, value: bool) {
        emergency_admin.require_auth();
        AccessControl::new(&e).assert_address_has_role(&emergency_admin, &Role::EmergencyAdmin);
        set_emergency_mode(&e, &value);
        AccessControlEvents::new(&e).set_emergency_mode(value);
    }

    // get_emergency_mode
    // Returns the current emergency mode state.
    //
    // Arguments:
    //   - e: The Soroban environment.
    //
    // Returns:
    //   - A boolean indicating whether emergency mode is active.
    fn get_emergency_mode(e: Env) -> bool {
        get_emergency_mode(&e)
    }
}

// The `TransferableContract` trait provides the interface for transferring ownership of the contract.
#[contractimpl]
impl TransferableContract for NormalOracle {
    // Commits an ownership transfer.
    //
    // # Arguments
    //
    // * `admin` - The address of the admin.
    // * `role_name` - The name of the role to transfer ownership of. The role must be one of the following:
    //     * `Admin`
    //     * `EmergencyAdmin`
    // * `new_address` - New address for the role
    fn commit_transfer_ownership(e: Env, admin: Address, role_name: Symbol, new_address: Address) {
        admin.require_auth();
        let access_control = AccessControl::new(&e);
        access_control.assert_address_has_role(&admin, &Role::Admin);

        let role = Role::from_symbol(&e, role_name);
        access_control.commit_transfer_ownership(&role, &new_address);
        AccessControlEvents::new(&e).commit_transfer_ownership(role, new_address);
    }

    // Applies the committed ownership transfer.
    //
    // # Arguments
    //
    // * `admin` - The address of the admin.
    // * `role_name` - The name of the role to transfer ownership of. The role must be one of the following:
    //     * `Admin`
    //     * `EmergencyAdmin`
    fn apply_transfer_ownership(e: Env, admin: Address, role_name: Symbol) {
        admin.require_auth();
        let access_control = AccessControl::new(&e);
        access_control.assert_address_has_role(&admin, &Role::Admin);

        let role = Role::from_symbol(&e, role_name);
        let new_address = access_control.apply_transfer_ownership(&role);
        AccessControlEvents::new(&e).apply_transfer_ownership(role, new_address);
    }

    // Reverts the committed ownership transfer.
    //
    // # Arguments
    //
    // * `admin` - The address of the admin.
    // * `role_name` - The name of the role to transfer ownership of. The role must be one of the following:
    //     * `Admin`
    //     * `EmergencyAdmin`
    fn revert_transfer_ownership(e: Env, admin: Address, role_name: Symbol) {
        admin.require_auth();
        let access_control = AccessControl::new(&e);
        access_control.assert_address_has_role(&admin, &Role::Admin);

        let role = Role::from_symbol(&e, role_name);
        access_control.revert_transfer_ownership(&role);
        AccessControlEvents::new(&e).revert_transfer_ownership(role);
    }

    // Returns the future address for the role.
    // The future address is the address that the ownership of the role will be transferred to.
    // The future address is set using the `commit_transfer_ownership` function.
    // The address will be defaulted to the current address if the transfer is not committed.
    //
    // # Arguments
    //
    // * `role_name` - The name of the role to get the future address for. The role must be one of the following:
    //    * `Admin`
    //    * `EmergencyAdmin`
    fn get_future_address(e: Env, role_name: Symbol) -> Address {
        let access_control = AccessControl::new(&e);
        let role = Role::from_symbol(&e, role_name);
        match access_control.get_transfer_ownership_deadline(&role) {
            0 =>
                match access_control.get_role_safe(&role) {
                    Some(address) => address,
                    None => panic_with_error!(&e, AccessControlError::RoleNotFound),
                }
            _ => access_control.get_future_address(&role),
        }
    }
}
