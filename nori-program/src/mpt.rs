use alloy_primitives::{keccak256, Address, B256, Bytes, FixedBytes, Uint, U256};
use alloy_rlp::Encodable;
use alloy_trie::{proof, Nibbles, TrieAccount, EMPTY_ROOT_HASH};
use anyhow::Result;
use nori_hash::merkle_poseidon_fixed::{
    compute_merkle_tree_depth_and_size, fold_merkle_left, get_merkle_zeros, hash_request_leaf,
    hash_storage_slot, MAX_TREE_DEPTH,
};
use nori_sp1_helios_primitives::types::{
    mapping_entry_location, storage_slot_of_index, struct_word_slot, word_of_address, word_of_b256,
    ContractStorage, QueueStorage, TargetSlotProof, MAX_BATCH, QUEUE_ENTRY_WORDS,
    QUEUE_HEAD_STORAGE_INDEX, QUEUE_REQUESTS_STORAGE_INDEX,
    SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX,
};
use o1_utils::FieldHelpers;
use std::collections::BTreeMap;
use std::fmt;

/// Custom MPT Errors

/// Selects which account `verify_account` is proving, so a failure raises an
/// error naming that account rather than one the caller must disambiguate.
#[derive(Debug, Clone, Copy)]
pub enum ProvenAccount {
    /// The NoriProofRequestQueue itself.
    ProofRequestQueue,
    /// A consumer contract named by a queue entry.
    Target,
}

#[derive(Debug)]
pub enum MptError {
    /// The NoriProofRequestQueue account could not be proven against the
    /// execution state root, so no queue state can be read.
    InvalidProofRequestQueueAccountProof {
        address: Address,
        reason: String,
    },
    /// A consumer contract named by a queue entry could not be proven present
    /// or absent.
    InvalidTargetAccountProof {
        address: Address,
        reason: String,
    },
    InvalidStorageSlotProof {
        slot_key: B256,
        reason: String,
    },
    LeafHashError {
        target: Address,
        slot_key: B256,
        value: Uint<256, 4>,
        reason: String,
    },
    /// Only produced by the superseded `verify_storage_slot_proofs`.
    InvalidStorageSlotCodeChallengeMapping {
        slot_key: B256,
        code_challenge: U256,
        computed_code_challenge_slot_key: B256,
    },
    /// Only produced by the superseded `verify_storage_slot_proofs`.
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
    /// The cursor supplied by the destination chain is ahead of the proven head.
    CursorAheadOfHead {
        cursor: u64,
        head: u64,
    },
    /// The witness supplied a different number of entries than the batch the
    /// queue state derives.
    BatchSizeMismatch {
        expected: u64,
        supplied: usize,
    },
    /// No account proof was supplied for a target an entry references.
    MissingTargetWitness {
        target: Address,
    },
    /// No slot proof was supplied for a slot an entry references.
    MissingSlotWitness {
        target: Address,
        slot_key: B256,
    },
}

impl fmt::Display for MptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MptError::InvalidProofRequestQueueAccountProof { address, reason } => write!(
                f,
                "MPT proof request queue account proof failed for {:?}: {:?}",
                address,
                reason
            ),
            MptError::InvalidTargetAccountProof { address, reason } => write!(
                f,
                "MPT target account proof failed for {:?}: {:?}",
                address,
                reason
            ),
            MptError::InvalidStorageSlotProof { slot_key, reason } => write!(
                f,
                "MPT storage proof failed for slot {:?}: {:?}",
                slot_key,
                reason
            ),
            MptError::LeafHashError { target, slot_key, value, reason } => write!(
                f,
                "MPT error hashing request leaf for target {:?} slot {:?} value {:?}: {:?}",
                target,
                slot_key,
                value,
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
            MptError::CursorAheadOfHead { cursor, head } => write!(
                f,
                "Request cursor {} is ahead of the proven queue head {}",
                cursor,
                head
            ),
            MptError::BatchSizeMismatch { expected, supplied } => write!(
                f,
                "Queue batch size mismatch: state derives {} entries, witness supplied {}",
                expected,
                supplied
            ),
            MptError::MissingTargetWitness { target } => write!(
                f,
                "No account proof supplied for target {:?}",
                target
            ),
            MptError::MissingSlotWitness { target, slot_key } => write!(
                f,
                "No slot proof supplied for target {:?} slot {:?}",
                target,
                slot_key
            ),
        }
    }
}

/// Verifies one storage word against an already-verified storage root.
///
/// The claimed `value` selects which proof must verify: a non-zero value
/// requires an inclusion proof of its RLP encoding, a zero value requires an
/// exclusion proof. Zero is never stored in a storage trie, so "holds zero" and
/// "absent from the trie" are the same fact. For a given root and key exactly
/// one of the two can verify, so the claim is pinned either way.
fn verify_storage_word(
    storage_root: B256,
    key: B256,
    value: U256,
    proof: &[Bytes],
) -> Result<(), MptError> {
    let key_nibbles = Nibbles::unpack(keccak256(key.as_slice()));

    let expected_value = if value.is_zero() {
        None
    } else {
        let mut rlp_encoded_value = Vec::new();
        value.encode(&mut rlp_encoded_value);
        Some(rlp_encoded_value)
    };

    proof::verify_proof(storage_root, key_nibbles, expected_value, proof).map_err(|e| {
        MptError::InvalidStorageSlotProof {
            slot_key: key,
            reason: e.to_string(),
        }
    })
}

/// Verifies an account claim against the execution state root and returns the
/// storage root to use for that account's slot proofs.
///
/// `Some(account)` requires an inclusion proof of the account's RLP encoding,
/// which promotes its `storage_root` to a verified anchor. `None` requires an
/// exclusion proof and yields `EMPTY_ROOT_HASH`, the root of an empty trie,
/// under which only zero-valued slot proofs verify.
fn verify_account(
    execution_state_root: B256,
    proven_account: ProvenAccount,
    address: Address,
    account: &Option<TrieAccount>,
    proof: &[Bytes],
) -> Result<B256, MptError> {
    let address_nibbles = Nibbles::unpack(keccak256(address.as_slice()));

    let expected_value = account.as_ref().map(|account| {
        let mut rlp_encoded_trie_account = Vec::new();
        account.encode(&mut rlp_encoded_trie_account);
        rlp_encoded_trie_account
    });

    proof::verify_proof(execution_state_root, address_nibbles, expected_value, proof).map_err(
        |e| match proven_account {
            ProvenAccount::ProofRequestQueue => {
                MptError::InvalidProofRequestQueueAccountProof {
                    address,
                    reason: e.to_string(),
                }
            }
            ProvenAccount::Target => MptError::InvalidTargetAccountProof {
                address,
                reason: e.to_string(),
            },
        },
    )?;

    Ok(account
        .as_ref()
        .map_or(EMPTY_ROOT_HASH, |account| account.storage_root))
}

/// Verifies a batch of proof requests against the execution state root and
/// returns the new cursor and the Merkle root of the verified requests.
///
/// The batch is derived from proven state rather than supplied: `head` is
/// MPT-proven against the queue's storage, `input_cursor` is pinned by the
/// destination chain's contract, and every entry's storage location is computed
/// from its index. The prover chooses nothing about which requests an update
/// covers.
///
/// Each entry yields exactly one leaf. A request naming an empty slot, or a
/// target that does not exist, yields a leaf with value zero rather than being
/// skipped, so no enqueued entry can stall the queue. Verification failure is
/// never a committable outcome: it aborts the run.
///
/// # Returns
/// `(output_cursor, requests_root)`. With an empty batch the root is the zero
/// hash, which is only reachable when `head == input_cursor`.
pub fn verify_queue(
    execution_state_root: FixedBytes<32>,
    queue_storage: QueueStorage,
) -> Result<(u64, FixedBytes<32>), MptError> {
    // The queue account itself; a lying witness fails the RLP comparison.
    let queue_storage_root = verify_account(
        execution_state_root,
        ProvenAccount::ProofRequestQueue,
        queue_storage.proof_request_queue_address,
        &Some(queue_storage.proof_request_queue_account),
        &queue_storage.proof_request_queue_account_mpt_proof,
    )?;

    // `head` at slot 0. A head of zero is absent from the trie, so this is an
    // exclusion proof at genesis.
    verify_storage_word(
        queue_storage_root,
        storage_slot_of_index(QUEUE_HEAD_STORAGE_INDEX),
        U256::from(queue_storage.head),
        &queue_storage.head_mpt_proof,
    )?;

    if queue_storage.input_cursor > queue_storage.head {
        return Err(MptError::CursorAheadOfHead {
            cursor: queue_storage.input_cursor,
            head: queue_storage.head,
        });
    }

    let batch = core::cmp::min(
        queue_storage.head - queue_storage.input_cursor,
        MAX_BATCH as u64,
    );
    if queue_storage.entries.len() as u64 != batch {
        return Err(MptError::BatchSizeMismatch {
            expected: batch,
            supplied: queue_storage.entries.len(),
        });
    }

    // Entry fields, at locations computed from the index rather than supplied.
    for (offset, entry) in queue_storage.entries.iter().enumerate() {
        let index = queue_storage.input_cursor + offset as u64;
        let base = mapping_entry_location(U256::from(index), QUEUE_REQUESTS_STORAGE_INDEX);

        let entry_words: [(B256, U256); QUEUE_ENTRY_WORDS] = [
            (base, word_of_address(entry.target)),
            (struct_word_slot(base, 1), word_of_b256(entry.slot_key)),
            (
                struct_word_slot(base, 2),
                U256::from(entry.collection_keys_count),
            ),
            (
                struct_word_slot(base, 3),
                word_of_b256(entry.collection_keys[0]),
            ),
            (
                struct_word_slot(base, 4),
                word_of_b256(entry.collection_keys[1]),
            ),
        ];

        for (word_index, (slot, value)) in entry_words.iter().enumerate() {
            verify_storage_word(
                queue_storage_root,
                *slot,
                *value,
                &entry.word_proofs[word_index],
            )?;
        }
    }

    // Target accounts. An absent account yields EMPTY_ROOT_HASH, under which
    // only zero-valued slots verify.
    let mut target_storage_roots: BTreeMap<Address, B256> = BTreeMap::new();
    let mut target_slots: BTreeMap<(Address, B256), &TargetSlotProof> = BTreeMap::new();
    for target in &queue_storage.targets {
        let storage_root = verify_account(
            execution_state_root,
            ProvenAccount::Target,
            target.target_address,
            &target.account,
            &target.account_mpt_proof,
        )?;
        target_storage_roots.insert(target.target_address, storage_root);

        for slot in &target.slots {
            target_slots.insert((target.target_address, slot.key), slot);
        }
    }

    // One leaf per entry, always.
    let mut merkle_nodes = Vec::with_capacity(queue_storage.entries.len());
    for entry in &queue_storage.entries {
        let storage_root = *target_storage_roots.get(&entry.target).ok_or(
            MptError::MissingTargetWitness {
                target: entry.target,
            },
        )?;

        let slot = *target_slots.get(&(entry.target, entry.slot_key)).ok_or(
            MptError::MissingSlotWitness {
                target: entry.target,
                slot_key: entry.slot_key,
            },
        )?;

        verify_storage_word(storage_root, entry.slot_key, slot.value, &slot.mpt_proof)?;

        let leaf = hash_request_leaf(
            &entry.target,
            entry.collection_keys_count,
            &entry.collection_keys[0],
            &entry.collection_keys[1],
            &slot.value,
        )
        .map_err(|e| MptError::LeafHashError {
            target: entry.target,
            slot_key: entry.slot_key,
            value: slot.value,
            reason: e.to_string(),
        })?;

        merkle_nodes.push(leaf);
    }

    let (depth, padded_size) = compute_merkle_tree_depth_and_size(merkle_nodes.len());
    if depth > MAX_TREE_DEPTH {
        return Err(MptError::ExceedsMaxTreeDepth {
            slots: merkle_nodes.len(),
            requested_depth: depth,
            max_depth: MAX_TREE_DEPTH,
        });
    }

    let root = fold_merkle_left(&mut merkle_nodes, padded_size, depth, &get_merkle_zeros());

    let mut root_bytes = [0u8; 32];
    root_bytes.copy_from_slice(&root.to_bytes());

    Ok((
        queue_storage.input_cursor + batch,
        FixedBytes::new(root_bytes),
    ))
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
///    code challenge and asserts it matches the provided slot key).
///    b. Verifies the slot exists in the contract's storage trie using the `storage_root` from the verified
///    `TrieAccount`. The slot key is hashed with `keccak256` and converted to nibbles for the proof.
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
#[deprecated(
    note = "Superseded by verify_queue. The prover supplies the storage keys here, so the committed root is not constrained to be complete."
)]
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
    .map_err(|e| MptError::InvalidTargetAccountProof {
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
            mapping_entry_location(code_challenge, SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX);
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

#[cfg(test)]
mod storage_proof_tests {
    use super::*;
    use alloy_rlp::Decodable;
    use alloy_trie::{nodes::TrieNode, proof::ProofRetainer, HashBuilder};
    use std::collections::HashMap;

    /// Builds a trie over `entries` and returns its root plus a proof for
    /// `target_preimage`. The target need not be present: an absent key yields
    /// an exclusion proof.
    ///
    /// Keys are the pre-image bytes that the trie hashes: a 32-byte slot key
    /// for a storage trie, a 20-byte address for the state trie.
    fn trie_with_proof(
        entries: &[(Vec<u8>, Vec<u8>)],
        target_preimage: &[u8],
    ) -> (B256, Vec<Bytes>) {
        let target_nibbles = Nibbles::unpack(keccak256(target_preimage));
        let retainer = ProofRetainer::new(vec![target_nibbles]);
        let mut builder = HashBuilder::default().with_proof_retainer(retainer);

        // HashBuilder requires leaves in ascending hashed-key order.
        let mut leaves: Vec<(B256, Vec<u8>)> = entries
            .iter()
            .map(|(preimage, value)| (keccak256(preimage), value.clone()))
            .collect();
        leaves.sort_by_key(|(hashed_key, _)| *hashed_key);

        for (hashed_key, value) in &leaves {
            builder.add_leaf(Nibbles::unpack(hashed_key), value);
        }

        let root = builder.root();
        let proof = builder
            .take_proof_nodes()
            .matching_nodes_sorted(&target_nibbles)
            .into_iter()
            .map(|(_, node)| node)
            .collect();

        (root, proof)
    }

    fn rlp_of(value: U256) -> Vec<u8> {
        let mut encoded = Vec::new();
        value.encode(&mut encoded);
        encoded
    }

    fn slot(byte: u8) -> B256 {
        B256::repeat_byte(byte)
    }

    /// A storage trie holding slot 0xaa = 42 and slot 0xbb = 7.
    fn populated_storage_trie(target_key: B256) -> (B256, Vec<Bytes>) {
        trie_with_proof(
            &[
                (slot(0xaa).to_vec(), rlp_of(U256::from(42u64))),
                (slot(0xbb).to_vec(), rlp_of(U256::from(7u64))),
            ],
            target_key.as_slice(),
        )
    }


    #[test]
    fn populated_slot_verifies_with_its_true_value() {
        let (root, proof) = populated_storage_trie(slot(0xaa));
        verify_storage_word(root, slot(0xaa), U256::from(42u64), &proof).unwrap();
    }

    #[test]
    fn populated_slot_claimed_zero_is_rejected() {
        let (root, proof) = populated_storage_trie(slot(0xaa));
        verify_storage_word(root, slot(0xaa), U256::ZERO, &proof).unwrap_err();
    }

    #[test]
    fn populated_slot_claimed_wrong_value_is_rejected() {
        let (root, proof) = populated_storage_trie(slot(0xaa));
        verify_storage_word(root, slot(0xaa), U256::from(43u64), &proof).unwrap_err();
    }

    #[test]
    fn empty_slot_claimed_zero_verifies_by_exclusion() {
        let (root, proof) = populated_storage_trie(slot(0xcc));
        verify_storage_word(root, slot(0xcc), U256::ZERO, &proof).unwrap();
    }

    #[test]
    fn empty_slot_claimed_nonzero_is_rejected() {
        let (root, proof) = populated_storage_trie(slot(0xcc));
        verify_storage_word(root, slot(0xcc), U256::from(1u64), &proof).unwrap_err();
    }

    /// A zero-valued entry word is simply absent from the trie.
    #[test]
    fn zero_entry_word_reads_as_zero() {
        let (root, proof) = populated_storage_trie(B256::ZERO);
        verify_storage_word(root, B256::ZERO, U256::ZERO, &proof).unwrap();
    }

    #[test]
    fn every_slot_of_an_empty_trie_is_zero() {
        verify_storage_word(EMPTY_ROOT_HASH, slot(0xaa), U256::ZERO, &[]).unwrap();
        verify_storage_word(EMPTY_ROOT_HASH, slot(0xaa), U256::from(1u64), &[]).unwrap_err();
    }

    fn address_of(byte: u8) -> Address {
        Address::repeat_byte(byte)
    }

    fn account_with_storage_root(storage_root: B256) -> TrieAccount {
        TrieAccount {
            nonce: 1,
            balance: U256::from(1_000u64),
            storage_root,
            code_hash: keccak256([0xfeu8]),
        }
    }

    /// A state trie holding accounts 0x11 and 0x22.
    fn populated_state_trie(target: Address) -> (B256, Vec<Bytes>, TrieAccount) {
        let account = account_with_storage_root(slot(0x99));
        let other = TrieAccount::default();

        let mut account_rlp = Vec::new();
        account.encode(&mut account_rlp);
        let mut other_rlp = Vec::new();
        other.encode(&mut other_rlp);

        let (root, proof) = trie_with_proof(
            &[
                (address_of(0x11).to_vec(), account_rlp),
                (address_of(0x22).to_vec(), other_rlp),
            ],
            target.as_slice(),
        );

        (root, proof, account)
    }

    #[test]
    fn present_account_verifies_and_returns_its_storage_root() {
        let (root, proof, account) = populated_state_trie(address_of(0x11));
        let storage_root =
            verify_account(root, ProvenAccount::Target, address_of(0x11), &Some(account), &proof).unwrap();
        assert_eq!(storage_root, slot(0x99));
    }

    #[test]
    fn present_account_claimed_absent_is_rejected() {
        let (root, proof, _) = populated_state_trie(address_of(0x11));
        verify_account(root, ProvenAccount::Target, address_of(0x11), &None, &proof).unwrap_err();
    }

    #[test]
    fn present_account_with_doctored_storage_root_is_rejected() {
        let (root, proof, account) = populated_state_trie(address_of(0x11));
        let doctored = TrieAccount {
            storage_root: slot(0x77),
            ..account
        };
        verify_account(root, ProvenAccount::Target, address_of(0x11), &Some(doctored), &proof).unwrap_err();
    }

    #[test]
    fn absent_account_verifies_and_yields_the_empty_storage_root() {
        let (root, proof, _) = populated_state_trie(address_of(0x33));
        let storage_root = verify_account(root, ProvenAccount::Target, address_of(0x33), &None, &proof).unwrap();
        assert_eq!(storage_root, EMPTY_ROOT_HASH);
    }

    #[test]
    fn absent_account_claimed_present_is_rejected() {
        let (root, proof, account) = populated_state_trie(address_of(0x33));
        verify_account(root, ProvenAccount::Target, address_of(0x33), &Some(account), &proof).unwrap_err();
    }

    /// An absent target contributes only zero-valued slots.
    #[test]
    fn absent_account_admits_no_nonzero_slot() {
        let (root, proof, _) = populated_state_trie(address_of(0x33));
        let storage_root = verify_account(root, ProvenAccount::Target, address_of(0x33), &None, &proof).unwrap();
        verify_storage_word(storage_root, slot(0xaa), U256::ZERO, &[]).unwrap();
        verify_storage_word(storage_root, slot(0xaa), U256::from(1u64), &[]).unwrap_err();
    }

    // -------------------------------------------------------------------------
    // Extension node coverage.
    //
    // `verify_proof` can conclude absence three ways: an empty branch child, a
    // different key's leaf on the path, or a diverging extension node. Trie keys
    // are keccak hashes, so they rarely share leading nibbles and extension
    // nodes do not occur by chance; the pair below is searched for explicitly.
    // -------------------------------------------------------------------------

    fn nibble_prefix(hashed_key: B256, nibble_count: usize) -> Vec<u8> {
        (0..nibble_count)
            .map(|index| {
                let byte = hashed_key[index / 2];
                if index % 2 == 0 {
                    byte >> 4
                } else {
                    byte & 0x0f
                }
            })
            .collect()
    }

    /// Searches for two slot keys whose hashed paths share `nibble_count`
    /// leading nibbles. A trie holding only these two keys is rooted at an
    /// extension node covering the shared prefix.
    fn keys_sharing_nibble_prefix(nibble_count: usize) -> (B256, B256) {
        let mut seen: HashMap<Vec<u8>, B256> = HashMap::new();

        for candidate in 0u64..1_000_000 {
            let key = B256::from(U256::from(candidate));
            let prefix = nibble_prefix(keccak256(key.as_slice()), nibble_count);

            if let Some(previous) = seen.insert(prefix, key) {
                return (previous, key);
            }
        }

        panic!("no key pair sharing {nibble_count} nibbles found");
    }

    fn proof_node_kinds(proof: &[Bytes]) -> Vec<&'static str> {
        proof
            .iter()
            .map(
                |node| match TrieNode::decode(&mut node.as_ref()).expect("decodable node") {
                    TrieNode::EmptyRoot => "EmptyRoot",
                    TrieNode::Branch(_) => "Branch",
                    TrieNode::Extension(_) => "Extension",
                    TrieNode::Leaf(_) => "Leaf",
                },
            )
            .collect()
    }

    /// A two-leaf trie whose keys share a nibble prefix, plus a third key that
    /// does not and therefore diverges inside the extension node.
    fn extension_rooted_trie(target_key: B256) -> (B256, Vec<Bytes>, B256, B256) {
        let (first, second) = keys_sharing_nibble_prefix(4);
        let (root, proof) = trie_with_proof(
            &[
                (first.to_vec(), rlp_of(U256::from(11u64))),
                (second.to_vec(), rlp_of(U256::from(22u64))),
            ],
            target_key.as_slice(),
        );
        (root, proof, first, second)
    }

    #[test]
    fn shared_prefix_keys_produce_an_extension_node() {
        let (first, _) = keys_sharing_nibble_prefix(4);
        let (_, proof, _, _) = extension_rooted_trie(first);
        assert!(
            proof_node_kinds(&proof).contains(&"Extension"),
            "expected an extension node, got {:?}",
            proof_node_kinds(&proof)
        );
    }

    #[test]
    fn key_diverging_inside_an_extension_verifies_as_absent() {
        let diverging = slot(0xdd);
        let (root, proof, first, _) = extension_rooted_trie(diverging);

        // The absent key must not share the extension's prefix, otherwise it
        // would diverge at the branch below instead.
        assert_ne!(
            nibble_prefix(keccak256(diverging.as_slice()), 4),
            nibble_prefix(keccak256(first.as_slice()), 4)
        );
        assert_eq!(proof_node_kinds(&proof), vec!["Extension"]);

        verify_storage_word(root, diverging, U256::ZERO, &proof).unwrap();
    }

    #[test]
    fn key_diverging_inside_an_extension_cannot_claim_a_value() {
        let diverging = slot(0xdd);
        let (root, proof, _, _) = extension_rooted_trie(diverging);
        verify_storage_word(root, diverging, U256::from(1u64), &proof).unwrap_err();
    }

    #[test]
    fn slot_under_an_extension_verifies_with_its_true_value() {
        let (first, _) = keys_sharing_nibble_prefix(4);
        let (root, proof, _, _) = extension_rooted_trie(first);
        verify_storage_word(root, first, U256::from(11u64), &proof).unwrap();
        verify_storage_word(root, first, U256::from(12u64), &proof).unwrap_err();
    }

    // -------------------------------------------------------------------------
    // Malformed proofs.
    //
    // Every node is authenticated against the hash its parent commits to, so a
    // tampered proof cannot resolve to a value.
    // -------------------------------------------------------------------------

    #[test]
    fn truncated_proof_is_rejected() {
        let (root, proof) = populated_storage_trie(slot(0xaa));
        let truncated = &proof[..proof.len() - 1];
        verify_storage_word(root, slot(0xaa), U256::from(42u64), truncated).unwrap_err();
    }

    #[test]
    fn empty_proof_against_a_populated_root_is_rejected() {
        let (root, _) = populated_storage_trie(slot(0xaa));
        verify_storage_word(root, slot(0xaa), U256::from(42u64), &[]).unwrap_err();
        verify_storage_word(root, slot(0xaa), U256::ZERO, &[]).unwrap_err();
    }

    #[test]
    fn reordered_proof_is_rejected() {
        let (root, proof) = populated_storage_trie(slot(0xaa));
        let mut reordered = proof.clone();
        reordered.reverse();
        verify_storage_word(root, slot(0xaa), U256::from(42u64), &reordered).unwrap_err();
    }

    #[test]
    fn foreign_node_prepended_to_a_proof_is_rejected() {
        let (root, proof) = populated_storage_trie(slot(0xaa));
        let (_, other_proof) = trie_with_proof(
            &[(slot(0x01).to_vec(), rlp_of(U256::from(5u64)))],
            slot(0x01).as_slice(),
        );

        let mut tampered = other_proof;
        tampered.extend(proof);
        verify_storage_word(root, slot(0xaa), U256::from(42u64), &tampered).unwrap_err();
    }

    #[test]
    fn proof_verified_against_the_wrong_root_is_rejected() {
        let (_, proof) = populated_storage_trie(slot(0xaa));
        verify_storage_word(
            B256::repeat_byte(0xde),
            slot(0xaa),
            U256::from(42u64),
            &proof,
        )
        .unwrap_err();
    }

    #[test]
    fn proof_for_another_key_is_rejected() {
        let (root, proof) = populated_storage_trie(slot(0xaa));
        verify_storage_word(root, slot(0xbb), U256::from(7u64), &proof).unwrap_err();
    }

    #[test]
    fn tampered_account_proof_is_rejected() {
        let (root, proof, account) = populated_state_trie(address_of(0x11));
        let truncated = &proof[..proof.len() - 1];
        verify_account(root, ProvenAccount::Target, address_of(0x11), &Some(account), truncated).unwrap_err();
        verify_account(
            B256::repeat_byte(0xde),
            ProvenAccount::Target,
            address_of(0x11),
            &Some(account),
            &proof,
        )
        .unwrap_err();
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;
    use alloy_trie::{proof::ProofRetainer, HashBuilder};
    use nori_sp1_helios_primitives::types::{QueueEntryProof, TargetStorageProof};

    const QUEUE_ADDRESS: Address = Address::repeat_byte(0x0e);

    fn rlp_of(value: U256) -> Vec<u8> {
        let mut encoded = Vec::new();
        value.encode(&mut encoded);
        encoded
    }

    /// Builds a storage trie and returns its root plus one proof per target.
    ///
    /// Zero-valued entries are omitted, matching Ethereum: a slot holding zero
    /// is absent from the trie, so its proof is an exclusion proof.
    fn storage_trie(entries: &[(B256, U256)], targets: &[B256]) -> (B256, Vec<Vec<Bytes>>) {
        let target_nibbles: Vec<Nibbles> = targets
            .iter()
            .map(|key| Nibbles::unpack(keccak256(key.as_slice())))
            .collect();
        let mut builder =
            HashBuilder::default().with_proof_retainer(ProofRetainer::new(target_nibbles.clone()));

        let mut leaves: Vec<(B256, Vec<u8>)> = entries
            .iter()
            .filter(|(_, value)| !value.is_zero())
            .map(|(key, value)| (keccak256(key.as_slice()), rlp_of(*value)))
            .collect();
        leaves.sort_by_key(|(hashed_key, _)| *hashed_key);
        for (hashed_key, value) in &leaves {
            builder.add_leaf(Nibbles::unpack(hashed_key), value);
        }

        let root = builder.root();
        let nodes = builder.take_proof_nodes();
        let proofs = target_nibbles
            .iter()
            .map(|target| {
                nodes
                    .matching_nodes_sorted(target)
                    .into_iter()
                    .map(|(_, node)| node)
                    .collect()
            })
            .collect();

        (root, proofs)
    }

    /// Builds the state trie over `accounts` and returns one proof per target.
    fn state_trie(
        accounts: &[(Address, TrieAccount)],
        targets: &[Address],
    ) -> (B256, Vec<Vec<Bytes>>) {
        let target_nibbles: Vec<Nibbles> = targets
            .iter()
            .map(|address| Nibbles::unpack(keccak256(address.as_slice())))
            .collect();
        let mut builder =
            HashBuilder::default().with_proof_retainer(ProofRetainer::new(target_nibbles.clone()));

        let mut leaves: Vec<(B256, Vec<u8>)> = accounts
            .iter()
            .map(|(address, account)| {
                let mut encoded = Vec::new();
                account.encode(&mut encoded);
                (keccak256(address.as_slice()), encoded)
            })
            .collect();
        leaves.sort_by_key(|(hashed_key, _)| *hashed_key);
        for (hashed_key, value) in &leaves {
            builder.add_leaf(Nibbles::unpack(hashed_key), value);
        }

        let root = builder.root();
        let nodes = builder.take_proof_nodes();
        let proofs = target_nibbles
            .iter()
            .map(|target| {
                nodes
                    .matching_nodes_sorted(target)
                    .into_iter()
                    .map(|(_, node)| node)
                    .collect()
            })
            .collect();

        (root, proofs)
    }

    #[derive(Clone)]
    struct Request {
        target: Address,
        slot_key: B256,
        collection_keys_count: u8,
        collection_keys: [B256; 2],
        /// Value held at `slot_key`. Zero means the slot is empty.
        value: U256,
        /// When false the target account is absent from the state trie.
        target_exists: bool,
    }

    fn request(target_byte: u8, slot_byte: u8, value: u64) -> Request {
        Request {
            target: Address::repeat_byte(target_byte),
            slot_key: B256::repeat_byte(slot_byte),
            collection_keys_count: 1,
            collection_keys: [B256::repeat_byte(slot_byte), B256::ZERO],
            value: U256::from(value),
            target_exists: true,
        }
    }

    /// Assembles a queue whose entries are `requests`, with `head` equal to the
    /// request count, and a witness covering `[cursor, head)`.
    fn fixture(requests: &[Request], cursor: u64) -> (B256, QueueStorage) {
        let head = requests.len() as u64;

        // Queue storage: head at slot 0, then five words per entry.
        let mut queue_entries: Vec<(B256, U256)> = vec![(
            storage_slot_of_index(QUEUE_HEAD_STORAGE_INDEX),
            U256::from(head),
        )];
        for (index, request) in requests.iter().enumerate() {
            let base = mapping_entry_location(U256::from(index as u64), QUEUE_REQUESTS_STORAGE_INDEX);
            queue_entries.extend([
                (base, word_of_address(request.target)),
                (struct_word_slot(base, 1), word_of_b256(request.slot_key)),
                (
                    struct_word_slot(base, 2),
                    U256::from(request.collection_keys_count),
                ),
                (
                    struct_word_slot(base, 3),
                    word_of_b256(request.collection_keys[0]),
                ),
                (
                    struct_word_slot(base, 4),
                    word_of_b256(request.collection_keys[1]),
                ),
            ]);
        }

        let queue_proof_targets: Vec<B256> =
            queue_entries.iter().map(|(key, _)| *key).collect();
        let (queue_storage_root, queue_proofs) =
            storage_trie(&queue_entries, &queue_proof_targets);

        // One storage trie per distinct target that exists.
        let batch: Vec<&Request> = requests.iter().skip(cursor as usize).collect();
        let mut target_addresses: Vec<Address> = Vec::new();
        for request in &batch {
            if !target_addresses.contains(&request.target) {
                target_addresses.push(request.target);
            }
        }

        let mut accounts: Vec<(Address, TrieAccount)> = vec![(
            QUEUE_ADDRESS,
            TrieAccount {
                storage_root: queue_storage_root,
                ..TrieAccount::default()
            },
        )];
        let mut target_tries: Vec<(Address, B256, Vec<B256>, Vec<Vec<Bytes>>, bool)> = Vec::new();

        for address in &target_addresses {
            // Distinct slots only: repeat requests for one slot share a proof.
            let mut slots: Vec<(B256, U256)> = Vec::new();
            for request in batch.iter().filter(|request| request.target == *address) {
                if !slots.iter().any(|(key, _)| *key == request.slot_key) {
                    slots.push((request.slot_key, request.value));
                }
            }
            let slot_keys: Vec<B256> = slots.iter().map(|(key, _)| *key).collect();
            let (root, proofs) = storage_trie(&slots, &slot_keys);

            let exists = batch
                .iter()
                .find(|request| request.target == *address)
                .map(|request| request.target_exists)
                .unwrap_or(true);

            if exists {
                accounts.push((
                    *address,
                    TrieAccount {
                        storage_root: root,
                        ..TrieAccount::default()
                    },
                ));
            }
            target_tries.push((*address, root, slot_keys, proofs, exists));
        }

        let mut account_proof_targets = vec![QUEUE_ADDRESS];
        account_proof_targets.extend(target_addresses.iter().copied());
        let (execution_state_root, account_proofs) =
            state_trie(&accounts, &account_proof_targets);

        let entries: Vec<QueueEntryProof> = requests
            .iter()
            .enumerate()
            .skip(cursor as usize)
            .map(|(index, request)| {
                // Proof 0 is head; entry `index` occupies the next five.
                let first = 1 + index * QUEUE_ENTRY_WORDS;
                let word_proofs: Vec<Vec<Bytes>> =
                    queue_proofs[first..first + QUEUE_ENTRY_WORDS].to_vec();
                QueueEntryProof {
                    target: request.target,
                    slot_key: request.slot_key,
                    collection_keys_count: request.collection_keys_count,
                    collection_keys: request.collection_keys,
                    word_proofs: word_proofs.try_into().unwrap(),
                }
            })
            .collect();

        let targets: Vec<TargetStorageProof> = target_tries
            .iter()
            .enumerate()
            .map(|(position, (address, root, slot_keys, proofs, exists))| TargetStorageProof {
                target_address: *address,
                account: exists.then(|| TrieAccount {
                    storage_root: *root,
                    ..TrieAccount::default()
                }),
                account_mpt_proof: account_proofs[position + 1].clone(),
                slots: slot_keys
                    .iter()
                    .zip(proofs.iter())
                    .map(|(key, proof)| TargetSlotProof {
                        key: *key,
                        value: batch
                            .iter()
                            .find(|request| {
                                request.target == *address && request.slot_key == *key
                            })
                            .map(|request| request.value)
                            .unwrap_or(U256::ZERO),
                        mpt_proof: proof.clone(),
                    })
                    .collect(),
            })
            .collect();

        let queue_storage = QueueStorage {
            proof_request_queue_address: QUEUE_ADDRESS,
            proof_request_queue_account: TrieAccount {
                storage_root: queue_storage_root,
                ..TrieAccount::default()
            },
            proof_request_queue_account_mpt_proof: account_proofs[0].clone(),
            head,
            head_mpt_proof: queue_proofs[0].clone(),
            input_cursor: cursor,
            entries,
            targets,
        };

        (execution_state_root, queue_storage)
    }

    #[test]
    fn drains_the_whole_queue() {
        let requests = [request(0x11, 0xaa, 42), request(0x22, 0xbb, 7)];
        let (state_root, queue) = fixture(&requests, 0);

        let (output_cursor, root) = verify_queue(state_root, queue).unwrap();
        assert_eq!(output_cursor, 2);
        assert_ne!(root, FixedBytes::default());
    }

    #[test]
    fn resumes_from_a_partial_cursor() {
        let requests = [
            request(0x11, 0xaa, 42),
            request(0x22, 0xbb, 7),
            request(0x33, 0xcc, 9),
        ];
        let (state_root, queue) = fixture(&requests, 1);

        let (output_cursor, _) = verify_queue(state_root, queue).unwrap();
        assert_eq!(output_cursor, 3);
    }

    #[test]
    fn empty_queue_yields_the_zero_root() {
        let (state_root, queue) = fixture(&[], 0);

        let (output_cursor, root) = verify_queue(state_root, queue).unwrap();
        assert_eq!(output_cursor, 0);
        assert_eq!(root, FixedBytes::default());
    }

    #[test]
    fn a_drained_queue_yields_the_zero_root() {
        let requests = [request(0x11, 0xaa, 42)];
        let (state_root, queue) = fixture(&requests, 1);

        let (output_cursor, root) = verify_queue(state_root, queue).unwrap();
        assert_eq!(output_cursor, 1);
        assert_eq!(root, FixedBytes::default());
    }

    #[test]
    fn cursor_ahead_of_head_is_rejected() {
        let requests = [request(0x11, 0xaa, 42)];
        let (state_root, mut queue) = fixture(&requests, 0);
        queue.input_cursor = 5;

        verify_queue(state_root, queue).unwrap_err();
    }

    #[test]
    fn withholding_an_entry_is_rejected() {
        let requests = [request(0x11, 0xaa, 42), request(0x22, 0xbb, 7)];
        let (state_root, mut queue) = fixture(&requests, 0);
        queue.entries.pop();

        verify_queue(state_root, queue).unwrap_err();
    }

    #[test]
    fn adding_an_entry_is_rejected() {
        let requests = [request(0x11, 0xaa, 42)];
        let (state_root, mut queue) = fixture(&requests, 0);
        let duplicate = queue.entries[0].clone();
        queue.entries.push(duplicate);

        verify_queue(state_root, queue).unwrap_err();
    }

    #[test]
    fn lying_about_head_is_rejected() {
        let requests = [request(0x11, 0xaa, 42)];
        let (state_root, mut queue) = fixture(&requests, 0);
        queue.head = 2;

        verify_queue(state_root, queue).unwrap_err();
    }

    #[test]
    fn tampering_with_an_entry_field_is_rejected() {
        let requests = [request(0x11, 0xaa, 42)];

        let (state_root, mut queue) = fixture(&requests, 0);
        queue.entries[0].target = Address::repeat_byte(0x99);
        verify_queue(state_root, queue).unwrap_err();

        let (state_root, mut queue) = fixture(&requests, 0);
        queue.entries[0].slot_key = B256::repeat_byte(0x99);
        verify_queue(state_root, queue).unwrap_err();

        let (state_root, mut queue) = fixture(&requests, 0);
        queue.entries[0].collection_keys_count = 2;
        verify_queue(state_root, queue).unwrap_err();

        let (state_root, mut queue) = fixture(&requests, 0);
        queue.entries[0].collection_keys[0] = B256::repeat_byte(0x99);
        verify_queue(state_root, queue).unwrap_err();
    }

    #[test]
    fn withholding_a_target_witness_is_rejected() {
        let requests = [request(0x11, 0xaa, 42)];
        let (state_root, mut queue) = fixture(&requests, 0);
        queue.targets.clear();

        verify_queue(state_root, queue).unwrap_err();
    }

    #[test]
    fn withholding_a_slot_witness_is_rejected() {
        let requests = [request(0x11, 0xaa, 42)];
        let (state_root, mut queue) = fixture(&requests, 0);
        queue.targets[0].slots.clear();

        verify_queue(state_root, queue).unwrap_err();
    }

    #[test]
    fn claiming_a_different_slot_value_is_rejected() {
        let requests = [request(0x11, 0xaa, 42)];
        let (state_root, mut queue) = fixture(&requests, 0);
        queue.targets[0].slots[0].value = U256::from(43u64);

        verify_queue(state_root, queue).unwrap_err();
    }

    /// An empty slot is a no-op leaf, not a stall.
    #[test]
    fn an_empty_slot_yields_a_zero_value_leaf() {
        let requests = [request(0x11, 0xaa, 0)];
        let (state_root, queue) = fixture(&requests, 0);

        let (output_cursor, root) = verify_queue(state_root, queue).unwrap();
        assert_eq!(output_cursor, 1);
        assert_ne!(root, FixedBytes::default());
    }

    /// A target that never existed, or was destroyed, is also a no-op leaf.
    #[test]
    fn an_absent_target_yields_a_zero_value_leaf() {
        let mut absent = request(0x11, 0xaa, 0);
        absent.target_exists = false;
        let (state_root, queue) = fixture(&[absent], 0);

        let (output_cursor, _) = verify_queue(state_root, queue).unwrap();
        assert_eq!(output_cursor, 1);
    }

    /// Repeat deposits name the same slot; one proof serves both leaves.
    #[test]
    fn duplicate_requests_share_a_single_slot_proof() {
        let requests = [request(0x11, 0xaa, 42), request(0x11, 0xaa, 42)];
        let (state_root, queue) = fixture(&requests, 0);
        assert_eq!(queue.targets.len(), 1);

        let (output_cursor, root) = verify_queue(state_root, queue).unwrap();
        assert_eq!(output_cursor, 2);
        assert_ne!(root, FixedBytes::default());
    }

    /// Entry order is fixed by index, so a swapped witness fails.
    #[test]
    fn reordering_entries_is_rejected() {
        let requests = [request(0x11, 0xaa, 42), request(0x22, 0xbb, 7)];
        let (state_root, mut queue) = fixture(&requests, 0);
        queue.entries.swap(0, 1);

        verify_queue(state_root, queue).unwrap_err();
    }
}
