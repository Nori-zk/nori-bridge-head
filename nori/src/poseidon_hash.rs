use alloy_primitives::ruint::algorithms::div;
use anyhow::Result;
use helios_consensus_core::{
    consensus_spec::MainnetConsensusSpec,
    types::{LightClientHeader, LightClientStore},
};
use kimchi::{
    mina_curves::pasta::Fp,
    mina_poseidon::{
        constants::PlonkSpongeConstantsKimchi,
        pasta::fp_kimchi,
        poseidon::{ArithmeticSponge as Poseidon, Sponge as _},
    },
    o1_utils::FieldHelpers,
};

fn poseidon_hash(input: &[Fp]) -> Fp {
    let mut hash = Poseidon::<Fp, PlonkSpongeConstantsKimchi>::new(fp_kimchi::static_params());
    hash.absorb(input);
    hash.squeeze()
}

// We need to go to bytes somehow
// https://github.com/djkoloski/rust_serialization_benchmark
// bitcode is the winner but not serde compatiable seems like only rmp_serde is the only compatible fastest
// Simple strategy

fn serialize_to_cbor(helios_store: &LightClientStore<MainnetConsensusSpec>) -> Result<Vec<u8>> {
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
    if let Some(best) = &helios_store.best_valid_update {
        result.extend(rmp_serde::to_vec(best)?);
    }

    Ok(result)
}

pub fn poseidon_hash_helios_store(
    helios_store: &LightClientStore<MainnetConsensusSpec>,
) -> Result<Fp> { // Fp = Fp256<...>
    let encoded_store = serialize_to_cbor(helios_store)?;
    let mut fps = Vec::new();

    // Split into 32-byte chunks (Fp256 requires exactly 32 bytes)
    for chunk in encoded_store.chunks(32) {
        let mut bytes = [0u8; 32];
        let bytes_to_copy = chunk.len().min(32);
        bytes[..bytes_to_copy].copy_from_slice(&chunk[..bytes_to_copy]);
        fps.push(Fp::from_bytes(&bytes)?); 
    }

    Ok(poseidon_hash(&fps))
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
