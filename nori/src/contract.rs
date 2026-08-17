use nori_sp1_helios_primitives::types::{
    mapping_entry_location, SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX,
};
use nori_contract_bindings::NoriTokenBridge::TokensLocked;
use alloy_primitives::{Address, Log, B256, U256};
use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    env,
};

/// Address of the NoriProofRequestQueue, the account every storage proof is
/// anchored on and the address committed to Mina.
pub fn get_proof_queue_address() -> Result<Address> {
    let proof_queue_address = env::var("NORI_PROOF_QUEUE_ADDRESS")
        .context("Missing NORI_PROOF_QUEUE_ADDRESS in environment")?
        .parse::<Address>()
        .context("Invalid Ethereum address format")?;
    Ok(proof_queue_address)
}

#[deprecated(
    note = "Superseded by the proof request queue; the prover anchors on NORI_PROOF_QUEUE_ADDRESS (get_proof_queue_address). Its result now only feeds the deprecated source-contract event path."
)]
pub fn get_source_contract_address() -> Result<Address> {
    let source_state_bridge_contract_address = env::var("NORI_TOKEN_BRIDGE_ADDRESS")
        .context("Missing NORI_TOKEN_BRIDGE_ADDRESS in environment")?
        .parse::<Address>()
        .context("Invalid Ethereum address format")?;
    Ok(source_state_bridge_contract_address)
}

#[deprecated(
    note = "Superseded by the proof request queue, which derives each slot at enqueue time on Ethereum. No longer used in the proving path."
)]
#[allow(deprecated)]
pub fn code_challenge_to_storage_slots(
    locked_token_event: Vec<Log<TokensLocked>>,
) -> HashMap<B256, U256> {
    let mut slot_to_code_challenge = HashMap::<B256, U256>::new();
    for locked_token_event in locked_token_event.iter() {
        let slot = mapping_entry_location(
            locked_token_event.codeChallenge,
            SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX,
        );
        slot_to_code_challenge.insert(slot, locked_token_event.codeChallenge);
    }
    slot_to_code_challenge
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

        let storage_slot = mapping_entry_location(address, mapping_index);

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
// FIXME(request-queue): move this to the nori-primitives crate in the new storage_layout file.
#[test]
fn test_single_mapping_storage_slot() {
    use alloy::hex;
    use alloy_primitives::{keccak256, Uint};
    use std::str::FromStr;
    use super::*;

    // For mapping(uint256 => uint256) at storage index 0:
    // slot = keccak256(abi.encode(key, mappingIndex))
    let code_challenge = Uint::<256, 4>::from_str("1111").unwrap();
    let mapping_index: u8 = 0;

    let slot = mapping_entry_location(code_challenge, mapping_index);

    // Manually compute expected: keccak256(code_challenge ++ padding(0))
    let mut encoded = [0u8; 64];
    encoded[0..32].copy_from_slice(&code_challenge.to_be_bytes::<32>());
    encoded[63] = mapping_index;
    let expected = keccak256(encoded);

    assert_eq!(slot, expected);
}