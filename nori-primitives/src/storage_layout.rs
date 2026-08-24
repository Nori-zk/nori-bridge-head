//! NoriProofRequestQueue storage layout: the constants pinning it to the
//! deployed contract, and the Solidity storage-arithmetic they're computed
//! with.
//!
//! Shared by the guest, which derives the keys it verifies, and the host, which
//! fetches those same keys over RPC.

use alloy_primitives::{keccak256, Address, B256, U256};

// -----------------------------------------------------------------------------
// NoriProofRequestQueue storage layout.
//
// Mirrors NoriProofRequestQueue.sol. The guest derives storage keys from these
// indices, so they must match the deployed contract exactly.
// -----------------------------------------------------------------------------

/// Slot index of `head`.
pub const QUEUE_HEAD_STORAGE_INDEX: u8 = 0;
/// Slot index of the `_requests` mapping.
pub const QUEUE_REQUESTS_STORAGE_INDEX: u8 = 1;
/// Consecutive storage words per entry: target, slotKey, count, key0, key1.
pub const QUEUE_ENTRY_WORDS: usize = 5;
/// Collection keys carried by one request.
pub const MAX_COLLECTION_KEYS: usize = 2;

// -----------------------------------------------------------------------------
// Solidity storage layout arithmetic.
// -----------------------------------------------------------------------------

/// Storage slot of a value-type state variable declared at `index`.
pub fn storage_slot_of_index(index: u8) -> B256 {
    B256::from(U256::from(index))
}

/// Storage slot of `mapping(uint256 => T)` entry `key`, for a mapping declared
/// at `mapping_slot_index`. For a struct value this is the entry's first word.
///
/// Solidity's rule: `keccak256(abi.encode(key, mapping_slot_index))`, where
/// `abi.encode` of two `uint256` is their 32-byte big-endian words concatenated.
pub fn mapping_entry_location(key: U256, mapping_slot_index: u8) -> B256 {
    let mut encoded = [0u8; 64];
    encoded[0..32].copy_from_slice(&key.to_be_bytes::<32>());
    encoded[32..64].copy_from_slice(&U256::from(mapping_slot_index).to_be_bytes::<32>());
    keccak256(encoded)
}

/// Slot of word `word_index` of a struct stored at `base`. Struct members
/// occupy consecutive slots, so this is integer addition on the key.
pub fn struct_word_slot(base: B256, word_index: u8) -> B256 {
    B256::from(U256::from_be_bytes(base.0).wrapping_add(U256::from(word_index)))
}

/// Storage-word value of an `address` member, which Solidity stores
/// right-aligned.
pub fn word_of_address(address: Address) -> U256 {
    U256::from_be_slice(address.as_slice())
}

/// Storage-word value of a `bytes32` member.
pub fn word_of_b256(word: B256) -> U256 {
    U256::from_be_bytes(word.0)
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

        let storage_slot = mapping_entry_location(word_of_address(address), mapping_index);

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
fn test_single_mapping_storage_slot() {
    use alloy::hex;
    use alloy_primitives::Uint;
    use std::str::FromStr;

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
