use alloy_primitives::{B256, U256};
use helios_consensus_core::{
    apply_finality_update, apply_update, verify_finality_update, verify_update,
};
use nori_hash::sha256_hash::sha256_hash_helios_store;
use nori_sp1_helios_primitives::types::{ProofInputs, ProofOutputs};
use tree_hash::TreeHash;

/// Zero-knowledge program for processing Ethereum light client updates.
///
/// This program takes encoded inputs containing light client updates (sync committee updates and a finality update),
/// verifies their validity against the current state, applies the updates, and outputs the new state commitments.
///
/// # Inputs
/// - `encoded_inputs`: CBOR-encoded `ProofInputs` containing:
///   * Sync committee updates (vector of updates to be processed in order)
///   * Finality update (single update marking finalized head)
///   * Expected current slot (for validation)
///   * Current store (light client state)
///   * Genesis root (for chain validation)
///   * Fork information (for chain validation)
///
/// # Operations
/// 1. Processes each sync committee update in sequence:
///    - Verifies each update is valid given current state
///    - Applies valid updates to the store
/// 2. Processes the finality update:
///    - Verifies the finality proof is valid
///    - Applies the finality update to the store
/// 3. Computes new state commitments:
///    - New header hash
///    - New sync committee hashes
///    - State root from execution payload
///    - Slot information for new/previous heads
///
/// # Outputs
/// Returns `ProofOutputs` containing:
/// - Execution state root
/// - New header hash
/// - Next sync committee hash (or zero if none exists)
/// - New head slot
/// - Previous header hash
/// - Previous head slot
/// - Current sync committee hash
/// - Starting sync committee hash (for verification)
///
/// # Panics
/// - If any sync committee update fails verification
/// - If the finality update fails verification
/// - If the execution payload is missing in the finalized header
/// 
/// # Program flow:
/// 0. Calculate and assert old store state hashes
/// 1. Apply sync committee updates, if any
/// 2. Apply finality update
/// 3. Commit execution state root
/// 4. Calculate the state hash of the updated store
/// 5. Asset all updates are valid
/// 6. Commit new state root, header, and sync committee for usage in the on-chain contract
pub fn zk_program(encoded_inputs: Vec<u8>) -> ProofOutputs {
    println!("Decoding inputs");
    let ProofInputs {
        updates: sync_committee_updates,
        finality_update,
        expected_current_slot,
        mut store,
        genesis_root,
        forks,
        store_hash: prev_store_hash,
    } = serde_cbor::from_slice(&encoded_inputs).unwrap();
    println!("Decoded inputs");

    // 0. Calculate old store hash and assert equality
    println!("Hashing old store state and comparing with proof inputs store hash.");
    let calculated_prev_store_hash = sha256_hash_helios_store(&store).unwrap();
    assert_eq!(calculated_prev_store_hash, prev_store_hash);
    println!(
        "Old store state hash is valid: {}",
        calculated_prev_store_hash
    );

    println!("Committing prev_header, prev_head and start_sync_committee_hash.");
    let prev_head = store.finalized_header.beacon().slot;
    let prev_header: B256 = store.finalized_header.beacon().tree_hash_root(); // Could deprecate
    let start_sync_committee_hash: alloy_primitives::FixedBytes<32> =
        store.current_sync_committee.tree_hash_root(); // Could deprecate
    println!("prev_head, prev_header and start_sync_committee_hash committed.");

    // 1. Apply sync committee updates, if any
    for (index, update) in sync_committee_updates.iter().enumerate() {
        // update.finalized_header.beacon().slot; introduce printing this so we can see if we are applying updates beyond our head
        let finalized_beacon_slot = { update.finalized_header().beacon().slot };
        println!(
            "Processing update {} of {}. Update beacon finalized slot: {}",
            index + 1,
            sync_committee_updates.len(),
            finalized_beacon_slot
        );
        let update_is_valid =
            verify_update(update, expected_current_slot, &store, genesis_root, &forks).is_ok();

        if !update_is_valid {
            panic!("Update {} is invalid!", index + 1);
        }
        println!("Update {} is valid.", index + 1);
        apply_update(&mut store, update);
        println!("Applied update {}.", index + 1);
    }

    // 2. Apply finality update
    println!("Processing finality update.");
    let finality_update_is_valid = verify_finality_update(
        &finality_update,
        expected_current_slot,
        &store,
        genesis_root,
        &forks,
    )
    .is_ok();
    if !finality_update_is_valid {
        panic!("Finality update is invalid!");
    }
    println!("Finality update is valid.");
    apply_finality_update(&mut store, &finality_update);
    println!("Applied finality update.");

    // Should do an assertion here to ensure we have increased our head (we do check this downstream later)

    // 3. Commit new state root, header, and sync committee for usage in the on-chain contract
    println!("Committing head, header, sync_committee_hash, next_sync_committee_hash and execution_state_root.");
    let head = store.finalized_header.beacon().slot;
    let header: B256 = store.finalized_header.beacon().tree_hash_root(); // Could deprecate
    let sync_committee_hash: B256 = store.current_sync_committee.tree_hash_root(); // Could deprecate
    let next_sync_committee_hash: B256 = match &mut store.next_sync_committee {
        Some(next_sync_committee) => next_sync_committee.tree_hash_root(),
        None => B256::ZERO,
    };
    let execution_state_root = *store
        .finalized_header
        .execution()
        .expect("Execution payload doesn't exist.")
        .state_root();
    println!("head, header, sync_committee_hash, next_sync_committee_hash and execution_state_root committed.");

    // 4. Calculated updated store hash to be validated in the next round
    println!("Hashing updated store.");
    let store_hash = sha256_hash_helios_store(&store).unwrap();
    println!("Hashing updated store complete: {}", store_hash);

    println!("Encoding outputs");
    let proof_outputs = ProofOutputs {
        executionStateRoot: execution_state_root,
        newHeader: header,
        nextSyncCommitteeHash: next_sync_committee_hash,
        newHead: U256::from(head),
        prevHeader: prev_header,
        prevHead: U256::from(prev_head),
        syncCommitteeHash: sync_committee_hash,
        startSyncCommitteeHash: start_sync_committee_hash,
        prevStoreHash: prev_store_hash,
        storeHash: store_hash,
    };
    println!("Encoded outputs");

    proof_outputs
}
