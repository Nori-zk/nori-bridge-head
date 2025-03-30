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
use tree_hash::TreeHash;
//use kimchi::ark_ff::{PrimeField, MontFp, Field};

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

        if let Some(fb) = &best.finality_branch {
            result.extend_from_slice(fb.tree_hash_root().as_ref());
        }

    }

    Ok(result)
}

// Poesidon hash serialized helios store.

pub fn poseidon_hash_helios_store(
    helios_store: &LightClientStore<MainnetConsensusSpec>,
) -> Result<Vec<u8>> {
    // Fp = Fp256<...>

    let encoded_store = serialize_helios_store(helios_store)?;
    let mut fps = Vec::new();

    for chunk in encoded_store.chunks(31) {
        let mut bytes = [0u8; 32];
        bytes[..chunk.len()].copy_from_slice(chunk);
        fps.push(Fp::from_bytes(&bytes)?);
    }

    Ok(poseidon_hash(&fps).to_bytes())
}

// SP1 LOGIC -------------------------------------------------------------------

/*
PSEUDO CODE

ZK sp1

(old_store, previous_hash)

calculcated_old_hash = hash(serialise(old_store))
assert previous_hash == calculcated_old_hash

do updates

(store is modified now)
new_store = old_store (after updates)

next_hash = hash(serialise(new_store))

return next_hash + other public outputs
*/

// Then for the ETH PROCESSOR -------------------------------------------------

// we have the converted proof outputs

// CHECK SP1 outputs are the same (via hashing) as the ETH INPUTS

/*

        executionStateRoot: *store
            .finalized_header
            .execution()
            .expect("Execution payload doesn't exist.")
            .state_root(),
        newHeader: header,
        nextSyncCommitteeHash: next_sync_committee_hash,
        newHead: U256::from(head),
        prevHeader: prev_header,
        prevHead: U256::from(prev_head),
        syncCommitteeHash: sync_committee_hash,
        startSyncCommitteeHash: start_sync_committee_hash,
        store_hash

        bytes = bytes.concat(input.executionStateRoot.bytes);
        bytes = bytes.concat(input.newHeader.bytes);
        bytes = bytes.concat(input.nextSyncCommitteeHash.bytes);
        bytes = bytes.concat(padUInt64To32Bytes(input.newHead));
        bytes = bytes.concat(input.prevHeader.bytes);
        bytes = bytes.concat(padUInt64To32Bytes(input.prevHead));
        bytes = bytes.concat(input.syncCommitteeHash.bytes);
        bytes = bytes.concat(input.startSyncComitteHash.bytes);
        bytes = bytes.concat(input.store_hash)

        // Check that zkporgraminput is same as passed to the SP1 program
        const pi0 = ethPlonkVK;
        const pi1 = parsePlonkPublicInputsProvable(Bytes.from(bytes));
        const piDigest = Poseidon.hashPacked(
            Provable.Array(FrC.provable, 2),
            [pi0, pi1]
        );
        Provable.log('piDigest', piDigest);
        Provable.log(
            'proof.publicOutput.rightOut',
            proof.publicOutput.rightOut
        );
        piDigest.assertEquals(proof.publicOutput.rightOut);

        return new_hash
*/

/*

1.

return type for proof outputs SP1
prevHead: U256::from(prev_head), // Matthew is this nessesary prev_head is u64 and perhaps isnt provable?? I think this
is an artifact of the solidity stuff and perhaps not relevant. Todo as well move away from the solidity encoding as we dont
care about it.

2.

question for noah

pub finalized_header: LightClientHeader,
pub current_sync_committee: SyncCommittee<S>,
pub next_sync_committee: Option<SyncCommittee<S>>,
pub optimistic_header: LightClientHeader,
pub previous_max_active_participants: u64,
pub current_max_active_participants: u64,
pub best_valid_update: Option<GenericUpdate<S>>,

can we omit these Optional fields / how would they behave..... if the optional members maybe or may not be present at any given time ( this is probably sorted by guareteeing we have definitely elapsed on epoch)

3.

for ourselved how to we serialise it (2.) whats the encoding.... bincode (probably as it is the most efficient) => bytes

4. store obj (bincode) -> bytes -> FIELDS-RUST -> (poseidon hash slice of rust fields) -> field (rust not serialisable perhaps hashable [maybe this helps])-> needs to go to a provable exportable type so that we can proof convert and to give to eth processor

*/
