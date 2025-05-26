use alloy_primitives::{keccak256, Address, Bytes, FixedBytes, B256};
use alloy_rlp::Encodable;
use alloy_trie::{proof, Nibbles};

use nori_sp1_helios_primitives::types::{get_storage_location_for_key, ContractStorage, VerifiedContractStorageSlot, SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX};
use std::fmt;

#[derive(Debug)]
pub enum MptError {
    InvalidAccountProof {
        address: Address,
        reason: String,
    },
    InvalidStorageSlotProof {
        slot_key: B256,
        reason: String,
    },
    InvalidStorageSlotAddressMapping {
        slot_key: B256,
        address: Address,
        computed_address_slot_key: B256,
    },
}

impl fmt::Display for MptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MptError::InvalidAccountProof { address, reason } => write!(
                f,
                "MPT account proof failed for {:?}: {:?}",
                address,
                reason
            ),
            MptError::InvalidStorageSlotProof { slot_key, reason } => write!(
                f,
                "MPT storage proof failed for slot {:?}: {:?}",
                slot_key,
                reason
            ),
            MptError::InvalidStorageSlotAddressMapping {slot_key, address, computed_address_slot_key} => write!(
                f,
                "MPT invalid invalid storage slot address, expected {:?}, but for address '{:?}' this slot '{:?}' was computed",
                slot_key,
                address,
                computed_address_slot_key
            )
        }
    }
}

/// Verifies the Merkle Patricia Trie (MPT) proofs for a contract's storage slots against the execution state root with Result type.
///
/// This function performs two main verifications:
/// 1. **Account Verification**: Validates that the contract's `TrieAccount` (RLP-encoded) is present in the global state trie
///    by verifying the provided MPT proof against the `execution_state_root`. The contract's address is hashed with `keccak256`
///    and converted to nibbles to traverse the trie.
/// 2. **Storage Slot Verification**: For each storage slot, verifies its existence in the contract's storage trie using the
///    `storage_root` from the verified `TrieAccount`. The slot key is hashed with `keccak256` and converted to nibbles for the proof.
///
/// # Parameters
/// - `execution_state_root`: The root hash of the Ethereum global state trie.
/// - `contract_storage`: Contains the contract's address, MPT proof for the account, storage slots, and expected values.
///
/// # Returns
/// A vector of [`VerifiedStorageSlot`] structs, each containing the verified `key`, `value`, and `contractAddress`.
///
/// # Errors
/// - If the MPT proof for the contract's `TrieAccount` is invalid.
/// - If any address vs slot mapping does not match.
/// - If any storage slot's MPT proof fails verification.
///
/// # Steps
/// 1. Hashes the contract address with `keccak256` and converts it to nibbles for the account proof.
/// 2. RLP-encodes the expected `TrieAccount` and verifies its MPT proof against the `execution_state_root`.
/// 3. For each storage slot:
///    - Hashes the slot key with `keccak256` and converts it to nibbles.
///    - RLP-encodes the expected slot value.
///    - Verifies the slot address mapping.
///    - Verifies the slot's MPT proof against the contract's `storage_root` from the verified `TrieAccount`.
pub fn verify_storage_slot_proofs(
    execution_state_root: FixedBytes<32>,
    contract_storage: ContractStorage,
) -> Result<Vec<VerifiedContractStorageSlot>, MptError> {
    // Convert the contract address into nibbles for the global MPT proof
    // We need to keccak256 the address before converting to nibbles for the MPT proof
    let address_hash = keccak256(contract_storage.address.as_slice());
    let address_nibbles = Nibbles::unpack(Bytes::copy_from_slice(address_hash.as_ref()));
    // RLP-encode the `TrieAccount`. This is what's actually stored in the global MPT
    let mut rlp_encoded_trie_account = Vec::new();
    contract_storage
        .expected_value
        .encode(&mut rlp_encoded_trie_account);

    // 1) Verify the contract's account node in the global MPT:
    //    We expect to find `rlp_encoded_trie_account` as the trie value for this address.
    proof::verify_proof(
        execution_state_root,
        address_nibbles,
        Some(rlp_encoded_trie_account),
        &contract_storage.mpt_proof,
    )
    .map_err(|e| MptError::InvalidAccountProof {
        address: contract_storage.address,
        reason: e.to_string(),
    })?;

    // 2) Now that we've verified the contract's `TrieAccount`, use it to verify each storage slot proof
    let mut verified_slots = Vec::with_capacity(contract_storage.storage_slots.len());
    for slot in contract_storage.storage_slots {
        let key = slot.key;
        let value = slot.expected_value;
        // We need to keccak256 the slot key before converting to nibbles for the MPT proof
        let key_hash = keccak256(key.as_slice());
        let key_nibbles = Nibbles::unpack(Bytes::copy_from_slice(key_hash.as_ref()));
        // RLP-encode expected value. This is what's actually stored in the contract MPT
        let mut rlp_encoded_value = Vec::new();
        value.encode(&mut rlp_encoded_value);

        // Verify slot address mapping
        let address = slot.slot_key_address;
        let computed_address_slot_key =
            get_storage_location_for_key(address, SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX);
        if computed_address_slot_key != key {
            return Err(MptError::InvalidStorageSlotAddressMapping {
                slot_key: key,
                address,
                computed_address_slot_key,
            });
        }

        // Verify the storage proof under the *contract's* storage root
        proof::verify_proof(
            contract_storage.expected_value.storage_root,
            key_nibbles,
            Some(rlp_encoded_value),
            &slot.mpt_proof,
        )
        .map_err(|e| MptError::InvalidStorageSlotProof {
            slot_key: key,
            reason: e.to_string(),
        })?;

        verified_slots.push(VerifiedContractStorageSlot {
            key,
            slotKeyAddress: address,
            value: FixedBytes(value.to_be_bytes()),
            contractAddress: contract_storage.address,
        });
    }

    Ok(verified_slots)
}