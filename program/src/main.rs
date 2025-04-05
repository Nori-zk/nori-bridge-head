#![no_main]
sp1_zkvm::entrypoint!(main);
use alloy_primitives::{B256, U256};
use alloy_sol_types::SolValue;
use helios_consensus_core::{
    apply_finality_update, apply_update, verify_finality_update, verify_update,
};
use nori_hash::{poseidon_hash::poseidon_hash_helios_store, sha256_hash::sha256_hash_helios_store};
use sp1_helios_primitives::types::{ProofInputs, ProofOutputs};
use tree_hash::TreeHash;

/// Program flow:
/// 0. Calculate and assert old store state hashes
/// 1. Apply sync committee updates, if any
/// 2. Apply finality update
/// 3. Verify execution state root proof
/// 4. Calculate the state hash of the updated store
/// 5. Asset all updates are valid
/// 6. Commit new state root, header, and sync committee for usage in the on-chain contract
pub fn main() {
    let encoded_inputs = sp1_zkvm::io::read_vec();

    let ProofInputs {
        sync_committee_updates,
        finality_update,
        expected_current_slot,
        mut store,
        genesis_root,
        forks,
        store_hash: prev_store_hash,
    } = serde_cbor::from_slice(&encoded_inputs).unwrap();

    // 0. Calculate old store hash and assert equality
    println!("Hashing old store state and comparing.");
    let calculated_prev_store_hash = poseidon_hash_helios_store(&store).unwrap();
    assert_eq!(calculated_prev_store_hash, prev_store_hash);
    print!("Old store hash is valid.");

    let start_sync_committee_hash = store.current_sync_committee.tree_hash_root();
    let prev_header: B256 = store.finalized_header.beacon().tree_hash_root();
    let prev_head = store.finalized_header.beacon().slot;

    // 1. Apply sync committee updates, if any
    for (index, update) in sync_committee_updates.iter().enumerate() {
        println!(
            "Processing update {} of {}.",
            index + 1,
            sync_committee_updates.len()
        );
        let update_is_valid =
            verify_update(update, expected_current_slot, &store, genesis_root, &forks).is_ok();

        if !update_is_valid {
            panic!("Update {} is invalid!", index + 1);
        }
        println!("Update {} is valid.", index + 1);
        apply_update(&mut store, update);
    }

    // 2. Apply finality update
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

    // 3. Commit new state root, header, and sync committee for usage in the on-chain contract
    let header: B256 = store.finalized_header.beacon().tree_hash_root();
    let sync_committee_hash: B256 = store.current_sync_committee.tree_hash_root();
    let next_sync_committee_hash: B256 = match &mut store.next_sync_committee {
        Some(next_sync_committee) => next_sync_committee.tree_hash_root(),
        None => B256::ZERO,
    };
    let head = store.finalized_header.beacon().slot;

    // 4. Calculated updated store hash to be validated in the next round
    println!("Hashing updated store.");
    let store_hash = poseidon_hash_helios_store(&store).unwrap();

    let proof_outputs = ProofOutputs {
        executionStateRoot: *store
            .finalized_header
            .execution()
            .expect("Execution payload doesn't exist.")
            .state_root(),
        newHeader: header,
        nextSyncCommitteeHash: next_sync_committee_hash,
        newHead: U256::from(head),
        prevHeader: prev_header,
        prevHead: U256::from(prev_head), // Matthew is this nessesary (he said no)
        syncCommitteeHash: sync_committee_hash,
        startSyncCommitteeHash: start_sync_committee_hash,
        prevStoreHash: prev_store_hash,
        storeHash: store_hash,
    };
    sp1_zkvm::io::commit_slice(&proof_outputs.abi_encode());
}
