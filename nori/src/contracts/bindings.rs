use alloy::sol;
use alloy_primitives::{Address, Log, B256, U256};
use anyhow::{Context, Result};
use nori_sp1_helios_primitives::types::{
    get_storage_location_for_key, SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX,
};
use std::{
    collections::HashMap,
    env,
};
use NoriStateBridge::TokensLocked;

// Npm install in this folder if the below indicates its not found.
sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    NoriStateBridge,
    "src/contracts/node_modules/@nori-zk/nori-bridge-sdk/contracts/ethereum/artifacts/contracts/NoriTokenBridge.sol/NoriTokenBridge.json"
);

pub fn get_source_contract_address() -> Result<Address> {
    let source_state_bridge_contract_address = env::var("NORI_TOKEN_BRIDGE_ADDRESS")
        .context("Missing NORI_TOKEN_BRIDGE_ADDRESS in environment")?
        .parse::<Address>()
        .context("Invalid Ethereum address format")?;
    Ok(source_state_bridge_contract_address)
}

pub fn addresses_attestation_pair_to_storage_slots(
    locked_token_event: Vec<Log<TokensLocked>>,
) -> HashMap<B256, (Address, U256)> {
    let mut slot_to_address_attestation = HashMap::<B256, (Address, U256)>::new();
    for locked_token_event in locked_token_event.iter() {
        let slot = get_storage_location_for_key(
            locked_token_event.user,
            locked_token_event.attestationHash,
            SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX,
        );
        slot_to_address_attestation.insert(slot, (locked_token_event.user, locked_token_event.attestationHash));
    }
    slot_to_address_attestation
}

// https://ethereum.stackexchange.com/questions/133473/how-to-calculate-the-location-index-slot-in-storage-of-a-mapping-key
/*#[cfg(test)]
mod tests {
    use alloy::hex;

    use super::*;

    #[test]
    fn test_storage_location() {
        // Test address from the comment
        let address =
            Address::from_slice(&hex::decode("6827b8f6cc60497d9bf5210d602C0EcaFDF7C405").unwrap());
        let mapping_index: u8 = 0;

        let storage_slot = get_storage_location_for_key(address, mapping_index);

        // Expected hash from the comment
        let expected = B256::from_slice(
            &hex::decode("86dfc0930cb222883cc0138873d68c1c9864fc2fe59d208c17f3484f489bef04")
                .unwrap(),
        );

        assert_eq!(storage_slot, expected);
    }
}*/

// https://www.rareskills.io/post/solidity-dynamic
// "Now let’s show a code example of getting nested array value from storage using assembly"
#[test]
fn test_nested_mapping_storage_slot() {
    use alloy::hex;
    use alloy_primitives::Uint;
    use std::str::FromStr;
    use super::*;

    let address = Address::from_slice(&hex::decode("0000000000000000000000000000000000000b0b").unwrap());
    let token_id = Uint::<256, 4>::from_str("1111").unwrap();
    let base_slot: u8 = 0;

    let slot = get_storage_location_for_key(address, token_id, base_slot);

    let expected = B256::from_slice(
        &hex::decode("0b061f98898a826aef6fdfc2d8eb981af54b85700e4516b39466540f69aced0f").unwrap(),
    );

    assert_eq!(slot, expected);
}