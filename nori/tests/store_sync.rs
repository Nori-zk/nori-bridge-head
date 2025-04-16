use alloy_primitives::{B256, U256};
use anyhow::Result;
use helios_consensus_core::{apply_finality_update, apply_update, consensus_spec::MainnetConsensusSpec, types::LightClientStore, verify_finality_update, verify_update};
use nori::sp1_prover::prepare_zk_program_input;
use nori_hash::sha256_hash::sha256_hash_helios_store;
use nori_sp1_helios_primitives::types::{ProofInputs, ProofOutputs};
use tree_hash::TreeHash;

static OLD_STORE_JSON: &str = include_str!("./data/store.11457376.json");

// This is from test with output normal-before-sync-committe-change.3.finalitychanges "Hashing updated store." 
// Its the serialized state of the store before the linear hash
// This represents an interesting scenario of an old checkpoint where the call to the RPC
// yields a zeroth update which has a finalized header slot of '11459104'
// which represents a period of 11459104/(32*256) = 1398.81640625
// This is very late in the period and is after the checkpoint value.
// This test aims to ensure we can sync our client and apply updates correctly.
// Previously the code erroneously applied the 0th update which was ahead of the checkpoint
// outside of the ZK, this had security implications.

#[tokio::test]
async fn next_sync_hack() -> Result<()> {
    dotenv::dotenv().ok();
    // Reconstruct the old stores state...

    let old_store: LightClientStore<MainnetConsensusSpec> =
        serde_json::from_str(OLD_STORE_JSON).unwrap();

    // We need to hash this and compare it to our reconstructed store....(reconstructed from the checkpoint using the hack method)

    let old_store_hash = sha256_hash_helios_store(&old_store)?;

    // Prepare zk input

    let input_slot: u64 = 11457376;

    // [2025-04-10T19:37:17Z INFO  nori::sp1_prover] Getting finality update from input_head 11457376, last next sync committee 0x645786a3a13bd020081a5cd29c88d891a069647f693eeac0d1e68d8f5bc1723c, store hash 0x18803bbc98959e260477aa6478e5f3bb162120ff20de9afcf85cc393ad710f3f

    let zk_input = prepare_zk_program_input(input_slot, old_store_hash).await?;

    // Now do the zk logic INLINE:

    // ----------------------------------------------------------------------------------------------------------

    println!("Decoding inputs");
    let ProofInputs {
        sync_committee_updates,
        finality_update,
        expected_current_slot,
        mut store,
        genesis_root,
        forks,
        store_hash: prev_store_hash,
    } = serde_cbor::from_slice(&zk_input).unwrap();
    println!("Decoded inputs");

    // 0. Calculate old store hash and assert equality
    println!("Hashing old store state and comparing with proof inputs store hash.");
    let calculated_prev_store_hash = sha256_hash_helios_store(&store).unwrap();
    assert_eq!(calculated_prev_store_hash, prev_store_hash);
    println!("Old store state hash is valid: {}", calculated_prev_store_hash);

    println!("Committing prev_header, prev_head and start_sync_committee_hash.");
    let prev_head = store.finalized_header.beacon().slot;
    let prev_header: B256 = store.finalized_header.beacon().tree_hash_root(); // Could deprecate
    let start_sync_committee_hash: alloy_primitives::FixedBytes<32> = store.current_sync_committee.tree_hash_root(); // Could deprecate
    println!("prev_head, prev_header and start_sync_committee_hash committed.");

    // 1. Apply sync committee updates, if any
    for (index, update) in sync_committee_updates.iter().enumerate() {
        // update.finalized_header.beacon().slot; introduce printing this so we can see if we are applying updates beyond our head
        let finalized_beacon_slot = {
            update.finalized_header().beacon().slot
        };
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

    Ok(())
}
