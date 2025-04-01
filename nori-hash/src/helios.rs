// We need to go to bytes somehow
// https://github.com/djkoloski/rust_serialization_benchmark
// bitcode is the winner but not serde compatiable seems like only rmp_serde is the only compatible fastest
// Tree hash strategy...

use anyhow::Result;
use helios_consensus_core::{consensus_spec::MainnetConsensusSpec, types::LightClientStore};
use tree_hash::TreeHash;

pub fn serialize_helios_store_serde(
    helios_store: &LightClientStore<MainnetConsensusSpec>,
) -> Result<Vec<u8>> {
    let mut result = Vec::new();

    // Required fields
    result.extend(rmp_serde::to_vec(&helios_store.finalized_header)?);
    result.extend(rmp_serde::to_vec(&helios_store.current_sync_committee)?);
    result.extend(rmp_serde::to_vec(&helios_store.optimistic_header)?);
    result.extend(rmp_serde::to_vec(
        &helios_store.previous_max_active_participants,
    )?);
    result.extend(rmp_serde::to_vec(
        &helios_store.current_max_active_participants,
    )?);

    // Optional fields
    if let Some(next) = &helios_store.next_sync_committee {
        result.extend(rmp_serde::to_vec(next)?);
    }
    /*if let Some(best) = &helios_store.best_valid_update {
        result.extend(rmp_serde::to_vec(best)?);
    }*/

    Ok(result)
}

pub fn serialize_helios_store(
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
    println!("Packing LightClientStore into Vec<u8>... ");
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

    // Pack best_valid_update (god i hope this dosent ever happen... it does actually!) PLEASE FORGIVE ME
    /*if let Some(best) = &helios_store.best_valid_update {
        //pub struct GenericUpdate<S: ConsensusSpec> {
        //    pub attested_header: LightClientHeader,
        //    pub sync_aggregate: SyncAggregate<S>,
        //    pub signature_slot: u64,
        //    pub next_sync_committee: Option<SyncCommittee<S>>,
        //    pub next_sync_committee_branch: Option<FixedVector<B256, typenum::U5>>,
        //    pub finalized_header: Option<LightClientHeader>,
        //    pub finality_branch: Option<FixedVector<B256, typenum::U6>>,
        //}

        // Pack best_valid_update.attested_header

        //pub struct LightClientHeader {
        //    pub beacon: BeaconBlockHeader,
        //    #[superstruct(only(Capella, Deneb))]
        //    pub execution: ExecutionPayloadHeader,
        //    #[superstruct(only(Capella, Deneb))]
        //    pub execution_branch: FixedVector<B256, typenum::U4>,
        //}

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
            nb.iter().for_each(|fb| {
                result.extend_from_slice(fb.as_ref());
            })
        }

        // Pack best_valid_update.finalized_header
        if let Some(fh) = &best.finalized_header {
            //  pub struct LightClientHeader {
            //      pub beacon: BeaconBlockHeader,
            //      #[superstruct(only(Capella, Deneb))]
            //      pub execution: ExecutionPayloadHeader,
            //      #[superstruct(only(Capella, Deneb))]
            //      pub execution_branch: FixedVector<B256, typenum::U4>,
            //  }

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
            fbb.iter().for_each(|fb| {
                result.extend_from_slice(fb.as_ref());
            })
        }
    }*/

    println!("Packing LightClientStore  DONE... ");
    Ok(result)
}
