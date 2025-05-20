use std::{collections::HashMap, env};
use alloy::sol;
use alloy_primitives::{keccak256, Address, Log, B256};
use anyhow::{Context, Result};
use NoriStateBridge::TokensLocked;

const SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX: u8 = 1u8;

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    NoriStateBridge,
    "../nori-contracts/artifacts/contracts/NoriTokenBridge.sol/NoriTokenBridge.json"
);

pub fn get_source_contract_address() -> Result<Address> {
    let source_state_bridge_contract_address = env::var("NORI_SOURCE_STATE_BRIDGE_CONTACT_ADDRESS")
        .context("Missing NORI_SOURCE_STATE_BRIDGE_CONTACT_ADDRESS in environment")?
        .parse::<Address>()
        .context("Invalid Ethereum address format")?;
    Ok(source_state_bridge_contract_address)
}

/// Returns the storage slot for a given address in a mapping at the specified index
///
/// This follows Solidity's ABI encoding rules:
/// 1. Each value is padded to 32 bytes
/// 2. Address is left-padded with zeros (12 bytes of zeros, then 20 bytes of address)
/// 3. Uint8 is right-padded with zeros (1 byte value, then 31 bytes of zeros)
/// 4. The encoding is big-endian
pub fn get_storage_location_for_key(address: Address, mapping_index: u8) -> B256 {
    // Encode the key and index as Solidity's abi.encode would
    let mut encoded = vec![0u8; 64]; // 2 × 32 bytes
    
    // Place address (padded to 32 bytes) in first slot
    // Address is already 20 bytes, so we need to place it at offset 12 (32-20)
    encoded[12..32].copy_from_slice(address.as_slice());
    
    // Place mapping_index (padded to 32 bytes) in second slot
    encoded[63] = mapping_index;
    
    // Hash the encoded data to get the storage slot
    keccak256(&encoded)
}

pub fn addresses_to_storage_slots(locked_token_event: Vec<Log<TokensLocked>>) -> Result<HashMap::<Address, B256>> {
    let mut storage_slots_to_prove = HashMap::<Address, B256>::new();

    for locked_token_event in locked_token_event.iter() {
        let slot = get_storage_location_for_key(locked_token_event.user, SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX);
        storage_slots_to_prove.insert(locked_token_event.user, slot);
    }
    
    Ok(storage_slots_to_prove)
}

// https://ethereum.stackexchange.com/questions/133473/how-to-calculate-the-location-index-slot-in-storage-of-a-mapping-key
#[cfg(test)]
mod tests {
    use alloy::hex;

    use super::*;
    
    #[test]
    fn test_storage_location() {
        // Test address from the comment
        let address = Address::from_slice(
            &hex::decode("6827b8f6cc60497d9bf5210d602C0EcaFDF7C405").unwrap()
        );
        let mapping_index: u8 = 0;
        
        let storage_slot = get_storage_location_for_key(address, mapping_index);
        
        // Expected hash from the comment
        let expected = B256::from_slice(
            &hex::decode("86dfc0930cb222883cc0138873d68c1c9864fc2fe59d208c17f3484f489bef04").unwrap()
        );
        
        assert_eq!(storage_slot, expected);
    }
}