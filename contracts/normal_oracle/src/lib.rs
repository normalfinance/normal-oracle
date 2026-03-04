#![no_std]

mod abi;
mod contract;
pub mod errors;
mod events;
mod interface;
mod oracle;
mod storage;
mod types;
mod test;
mod test_permissions;
mod testutils;

pub use crate::contract::{NormalOracle, NormalOracleClient};
