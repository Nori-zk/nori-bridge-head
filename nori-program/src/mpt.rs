use alloy_primitives::{keccak256, Address, B256, Bytes, FixedBytes, Uint, U256};
use alloy_rlp::Encodable;
use alloy_trie::{proof, Nibbles};
use anyhow::Result;
use nori_hash::merkle_poseidon_fixed::{
    compute_merkle_tree_depth_and_size, fold_merkle_left, get_merkle_zeros, hash_storage_slot,
    MAX_TREE_DEPTH,
};
use nori_sp1_helios_primitives::types::{
    get_storage_location_for_key, ContractStorage, SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX,
};
use o1_utils::FieldHelpers;
use std::fmt;

/// Custom MPT Errors

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
    InvalidStorageSlotCodeChallengeMapping {
        slot_key: B256,
        code_challenge: U256,
        computed_code_challenge_slot_key: B256,
    },
    MerkleHashError {
        code_challenge: U256,
        value: Uint<256, 4>,
        reason: String,
    },
    ExceedsMaxTreeDepth {
        slots: usize,
        requested_depth: usize,
        max_depth: usize,
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
            MptError::InvalidStorageSlotCodeChallengeMapping {slot_key, code_challenge, computed_code_challenge_slot_key} => write!(
                f,
                "MPT invalid storage slot code challenge, expected {:?}, but for code_challenge '{:?}' this slot '{:?}' was computed",
                slot_key,
                code_challenge,
                computed_code_challenge_slot_key
            ),
            MptError::MerkleHashError { code_challenge, value , reason} => write!(
                f,
                "MPT error computing merkle hash of verified slots, code_challenge {:?} and value {:?}: {:?}",
                code_challenge,
                value,
                reason
            ),
            MptError::ExceedsMaxTreeDepth {
                slots,
                requested_depth,
                max_depth,
            } => write!(
                f,
                "Merkle tree depth {} (derived from contract storage slots = {}) exceeds the maximum allowed depth of {}",
                requested_depth,
                slots,
                max_depth
            ),
        }
    }
}

/// Verifies the Merkle Patricia Trie (MPT) proofs for a contract's account and storage slots
/// against the execution state root, then computes and returns the Merkle root of the verified storage slots.
///
/// This function performs:
/// 1. **Account Verification** (unconditional): Validates that the contract's `TrieAccount` (RLP-encoded) is present
///    in the global state trie by verifying the provided MPT proof against the `execution_state_root`. The contract's
///    address is hashed with `keccak256` and converted to nibbles to traverse the trie. This always runs, even with
///    0 storage slots, ensuring the contract exists at the proven execution state root.
/// 2. **Tree Depth Validation** (skipped if 0 slots): Checks that the number of storage slots does not
///    exceed `MAX_TREE_DEPTH`.
/// 3. **Storage Slot Verification** (skipped if 0 slots): For each storage slot:
///    a. Verifies the code-challenge-to-slot-key mapping is correct (recomputes the storage location from the
///       code challenge and asserts it matches the provided slot key).
///    b. Verifies the slot exists in the contract's storage trie using the `storage_root` from the verified
///       `TrieAccount`. The slot key is hashed with `keccak256` and converted to nibbles for the proof.
///    c. Hashes the verified slot details (code challenge + value) into a Poseidon Merkle leaf.
/// 4. **Merkle Root Computation**: Computes the Merkle root from leaves via in-place folding.
///
/// # Parameters
/// - `execution_state_root`: The root hash of the Ethereum global state trie.
/// - `contract_storage`: Contains the contract's address, MPT proof for the account, storage slots, and expected values.
///
/// # Returns
/// - `FixedBytes::default()` (zero hash) if the contract exists but has 0 storage slots in this window.
/// - The Merkle root of the verified storage slot details as `FixedBytes<32>` otherwise.
///
/// # Errors
/// - `MptError::InvalidAccountProof` if the account proof verification fails (contract not in state trie)
/// - `MptError::InvalidStorageSlotCodeChallengeMapping` if code-challenge-to-slot mapping is invalid
/// - `MptError::InvalidStorageSlotProof` if any storage slot proof is invalid
/// - `MptError::MerkleHashError` if hashing a storage slot leaf fails
/// - `MptError::ExceedsMaxTreeDepth` if the number of storage slots yields a merkle tree
///   which is too large.
pub fn verify_storage_slot_proofs(
    execution_state_root: FixedBytes<32>,
    contract_storage: ContractStorage,
) -> Result<FixedBytes<32>, MptError> {
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

    // Optimisation, skip doing the MPT proof if we have no storage slots in this window
    let n_leaves = contract_storage.storage_slots.len();
    if n_leaves == 0 {
        return Ok(FixedBytes::default())
    }

    // Calculate tree depth which is ceil(log2(number)) and padded size (leaves to the nearest power of 2)
    let (depth, padded_size) = compute_merkle_tree_depth_and_size(n_leaves);

    // Validate
    if depth > MAX_TREE_DEPTH {
        return Err(MptError::ExceedsMaxTreeDepth {
            slots: n_leaves,
            requested_depth: depth,
            max_depth: MAX_TREE_DEPTH,
        });
    }

    // 2) Now that we've verified the contract's `TrieAccount`, use it to verify each storage slot proof
    let mut merkle_nodes = Vec::with_capacity(padded_size);

    for slot in contract_storage.storage_slots {
        let key = slot.key;
        let value = slot.expected_value;
        // We need to keccak256 the slot key before converting to nibbles for the MPT proof
        let key_hash = keccak256(key.as_slice());
        let key_nibbles = Nibbles::unpack(Bytes::copy_from_slice(key_hash.as_ref()));
        // RLP-encode expected value. This is what's actually stored in the contract MPT
        let mut rlp_encoded_value = Vec::new();
        value.encode(&mut rlp_encoded_value);

        // Verify slot code challenge mapping
        let code_challenge = slot.slot_key_code_challenge;
        let computed_code_challenge_slot_key =
            get_storage_location_for_key(code_challenge, SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX);
        if computed_code_challenge_slot_key != key {
            return Err(MptError::InvalidStorageSlotCodeChallengeMapping {
                slot_key: key,
                code_challenge,
                computed_code_challenge_slot_key,
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

        let slot_merkle_leaf_result = hash_storage_slot(&code_challenge, &value);
        let slot_merkle_leaf = match slot_merkle_leaf_result {
            Ok(val) => val,
            Err(error) => {
                return Err(MptError::MerkleHashError {
                    code_challenge,
                    value,
                    reason: error.to_string(),
                })
            }
        };
        merkle_nodes.push(slot_merkle_leaf);
    }

    // Calculate the root hash
    let root = fold_merkle_left(&mut merkle_nodes, padded_size, depth, &get_merkle_zeros());

    let mut fixed_bytes = [0u8; 32];
    fixed_bytes[..32].copy_from_slice(&root.to_bytes());

    Ok(FixedBytes::new(fixed_bytes))
}
