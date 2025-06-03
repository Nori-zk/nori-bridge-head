use alloy::sol;
use alloy_primitives::{Address, Log, B256};
use anyhow::{Context, Result};
use nori_sp1_helios_primitives::types::{
    get_storage_location_for_key, SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX,
};
use std::{
    collections::HashMap,
    env,
};
use NoriStateBridge::TokensLocked;

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    NoriStateBridge,
    "../nori-contracts/artifacts/contracts/NoriTokenBridge.sol/NoriTokenBridge.json"
);

pub fn get_source_contract_address() -> Result<Address> {
    let source_state_bridge_contract_address = env::var("NORI_TOKEN_BRIDGE_ADDRESS")
        .context("Missing NORI_TOKEN_BRIDGE_ADDRESS in environment")?
        .parse::<Address>()
        .context("Invalid Ethereum address format")?;
    Ok(source_state_bridge_contract_address)
}

pub fn addresses_to_storage_slots(
    locked_token_event: Vec<Log<TokensLocked>>,
) -> HashMap<B256, Address> {
    let mut slot_to_address = HashMap::<B256, Address>::new();
    for locked_token_event in locked_token_event.iter() {
        let slot = get_storage_location_for_key(
            locked_token_event.user,
            SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX,
        );
        slot_to_address.insert(slot, locked_token_event.user);
    }
    slot_to_address
}

// https://ethereum.stackexchange.com/questions/133473/how-to-calculate-the-location-index-slot-in-storage-of-a-mapping-key
#[cfg(test)]
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
}
