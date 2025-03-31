#![no_main]
sp1_zkvm::entrypoint!(main);

use alloy_primitives::{FixedBytes, B256, U256};
use alloy_sol_types::SolValue;
use helios_consensus_core::{
    apply_finality_update, apply_update, verify_finality_update, verify_update,
};

use sp1_helios_primitives::types::{ProofInputs, ProofOutputs};
use tree_hash::TreeHash;

//use alloy_primitives::FixedBytes;
use anyhow::Result;
use helios_consensus_core::{consensus_spec::MainnetConsensusSpec, types::LightClientStore};
use kimchi::{
    mina_curves::pasta::Fp,
    mina_poseidon::{
        constants::PlonkSpongeConstantsKimchi,
        pasta::fp_kimchi,
        poseidon::{ArithmeticSponge as Poseidon, Sponge as _},
    },
    o1_utils::FieldHelpers,
};
//use tree_hash::TreeHash;

// Kimchi poseidon hash

fn poseidon_hash(input: &[Fp]) -> Fp {
    let mut hash = Poseidon::<Fp, PlonkSpongeConstantsKimchi>::new(fp_kimchi::static_params());
    hash.absorb(input);
    hash.squeeze()
}

// We need to go to bytes somehow
// https://github.com/djkoloski/rust_serialization_benchmark
// bitcode is the winner but not serde compatiable seems like only rmp_serde is the only compatible fastest
// Tree hash strategy...

fn serialize_helios_store(
    helios_store: &LightClientStore<MainnetConsensusSpec>,
) -> Result<Vec<u8>> {
    // Pack Light Client Store into Vec<u8>

    /*

        pub struct LightClientStore<S: ConsensusSpec> {
            pub finalized_header: LightClientHeader,
            pub current_sync_committee: SyncCommittee<S>,
            pub next_sync_committee: Option<SyncCommittee<S>>,
            pub optimistic_header: LightClientHeader,
            pub previous_max_active_participants: u64,
            pub current_max_active_participants: u64,
            pub best_valid_update: Option<GenericUpdate<S>>,
        }

    */

    let mut result = Vec::new();

    // Required fields ---------------------------------------------------------------------

    // Pack finalized_header

    /*

        pub struct LightClientHeader {
            pub beacon: BeaconBlockHeader,
            #[superstruct(only(Capella, Deneb))]
            pub execution: ExecutionPayloadHeader,
            #[superstruct(only(Capella, Deneb))]
            pub execution_branch: FixedVector<B256, typenum::U4>,
        }

     */

    result.extend_from_slice(
        helios_store
            .finalized_header
            .beacon()
            .tree_hash_root()
            .as_ref(),
    );
    result.extend_from_slice(
        helios_store
            .finalized_header
            .execution()
            .ok()
            .map(|e| e.tree_hash_root())
            .unwrap_or_default()
            .as_ref(),
    );
    result.extend_from_slice(
        helios_store
            .finalized_header
            .execution_branch()
            .ok()
            .map(|b| b.tree_hash_root())
            .unwrap_or_default()
            .as_ref(),
    );

    // Pack current_sync_committee

    result.extend_from_slice(
        helios_store
            .current_sync_committee
            .tree_hash_root()
            .as_ref(),
    );

    // Pack optimistic_header??
    // result.extend(rmp_serde::to_vec(&helios_store.optimistic_header)?); Do we care about this? We are only concerned with finality... if we do try tree hash method in future

    let mut active_participants = [0u8; 16]; // 16 bytes = 2×u64
    active_participants[0..8]
        .copy_from_slice(&helios_store.previous_max_active_participants.to_be_bytes());
    active_participants[8..16]
        .copy_from_slice(&helios_store.current_max_active_participants.to_be_bytes());
    result.extend_from_slice(&active_participants);

    // Optional fields -----------------------------------------------------------------------

    // Pack next_sync_committee
    if let Some(next) = &helios_store.next_sync_committee {
        result.extend_from_slice(next.tree_hash_root().as_ref());
    }

    // Pack best_valid_update (god i hope this dosent ever happen)
    if let Some(best) = &helios_store.best_valid_update {
        /*

        pub struct GenericUpdate<S: ConsensusSpec> {
            pub attested_header: LightClientHeader,
            pub sync_aggregate: SyncAggregate<S>,
            pub signature_slot: u64,
            pub next_sync_committee: Option<SyncCommittee<S>>,
            pub next_sync_committee_branch: Option<FixedVector<B256, typenum::U5>>,
            pub finalized_header: Option<LightClientHeader>,
            pub finality_branch: Option<FixedVector<B256, typenum::U6>>,
        }

        */

        // Pack best_valid_update.attested_header

        /*

            pub struct LightClientHeader {
                pub beacon: BeaconBlockHeader,
                #[superstruct(only(Capella, Deneb))]
                pub execution: ExecutionPayloadHeader,
                #[superstruct(only(Capella, Deneb))]
                pub execution_branch: FixedVector<B256, typenum::U4>,
            }

        */

        result.extend_from_slice(best.attested_header.beacon().tree_hash_root().as_ref());
        result.extend_from_slice(
            best.attested_header
                .execution()
                .ok()
                .map(|e| e.tree_hash_root())
                .unwrap_or_default()
                .as_ref(),
        );
        result.extend_from_slice(
            best.attested_header
                .execution_branch()
                .ok()
                .map(|b| b.tree_hash_root())
                .unwrap_or_default()
                .as_ref(),
        );

        // Pack best_valid_update.sync_aggregate
        result.extend_from_slice(best.sync_aggregate.tree_hash_root().as_ref());

        // Pack best_valid_update.signature_slot
        result.extend_from_slice(&best.signature_slot.to_be_bytes()); // u64

        // Pack best_valid_update.next_sync_committee
        if let Some(next) = &best.next_sync_committee {
            result.extend_from_slice(next.tree_hash_root().as_ref());
        }

        if let Some(nb) = &best.next_sync_committee_branch {
            nb.iter().for_each(|fb|{
                result.extend_from_slice(fb.as_ref());
            })
        }

        // Pack best_valid_update.finalized_header
        if let Some(fh) = &best.finalized_header {

            /*

                pub struct LightClientHeader {
                    pub beacon: BeaconBlockHeader,
                    #[superstruct(only(Capella, Deneb))]
                    pub execution: ExecutionPayloadHeader,
                    #[superstruct(only(Capella, Deneb))]
                    pub execution_branch: FixedVector<B256, typenum::U4>,
                }

            */

            result.extend_from_slice(fh.beacon().tree_hash_root().as_ref());
            result.extend_from_slice(
                fh.execution()
                    .ok()
                    .map(|e| e.tree_hash_root())
                    .unwrap_or_default()
                    .as_ref(),
            );
            result.extend_from_slice(
                fh.execution_branch()
                    .ok()
                    .map(|e| e.tree_hash_root())
                    .unwrap_or_default()
                    .as_ref(),
            );
        }

        if let Some(fbb) = &best.finality_branch {
            //result.extend_from_slice(fb.tree_hash_root().as_ref());
            fbb.iter().for_each(|fb|{
                result.extend_from_slice(fb.as_ref());
            })
        }

    }

    Ok(result)
}

// Poesidon hash serialized helios store.

pub fn poseidon_hash_helios_store(
    helios_store: &LightClientStore<MainnetConsensusSpec>,
) -> Result<FixedBytes<32>> {
    // Fp = Fp256<...>

    let encoded_store = serialize_helios_store(helios_store)?;
    let mut fps = Vec::new();

    for chunk in encoded_store.chunks(31) {
        let mut bytes = [0u8; 32];
        bytes[..chunk.len()].copy_from_slice(chunk);
        fps.push(Fp::from_bytes(&bytes)?);
    }

    let mut fixed_bytes = [0u8; 32];
    fixed_bytes[..32].copy_from_slice(&poseidon_hash(&fps).to_bytes());
    Ok(FixedBytes::new(fixed_bytes))
}


/// Program flow:
/// 1. Apply sync committee updates, if any
/// 2. Apply finality update
/// 3. Verify execution state root proof
/// 4. Asset all updates are valid
/// 5. Commit new state root, header, and sync committee for usage in the on-chain contract
pub fn main() {
    let encoded_inputs = sp1_zkvm::io::read_vec();

    let ProofInputs {
        sync_committee_updates,
        finality_update,
        expected_current_slot,
        mut store,
        genesis_root,
        forks,
        store_hash: prev_store_hash
    } = serde_cbor::from_slice(&encoded_inputs).unwrap();

    // Calculate old store hash
    let calculated_prev_store_hash = poseidon_hash_helios_store(&store).unwrap();
    // Assert 
    assert_eq!(calculated_prev_store_hash, prev_store_hash);

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

    // Calculated updated stores hash
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
        storeHash: store_hash        
    };
    sp1_zkvm::io::commit_slice(&proof_outputs.abi_encode());
}
