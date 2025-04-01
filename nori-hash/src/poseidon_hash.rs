use crate::helios::serialize_helios_store_serde as ser;
use alloy_primitives::FixedBytes;
use anyhow::{Context, Result};
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

//use kimchi::ark_ff::{PrimeField, MontFp, Field};

// Kimchi poseidon hash

fn poseidon_hash(input: &[Fp]) -> Fp {
    let mut hash = Poseidon::<Fp, PlonkSpongeConstantsKimchi>::new(fp_kimchi::static_params());
    hash.absorb(input);
    hash.squeeze()
}

// Poesidon hash serialized helios store.

pub fn poseidon_hash_helios_store(
    helios_store: &LightClientStore<MainnetConsensusSpec>,
) -> Result<FixedBytes<32>> {
    // DEBUGGING PRINT REMOVE WHEN HASH TRANSITION ISSUE IS FIXED
    // print_helios_store(helios_store)?;

    // Fp = Fp256<...>

    let encoded_store = ser(helios_store)?;
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

// Debugging logic

pub fn print_helios_store(helios_store: &LightClientStore<MainnetConsensusSpec>) -> Result<()> {
    // The easiest way would be to serialise it via serde json
    let str_store =
        serde_json::to_string(&helios_store).context("Failed to serialize helios store")?;
    println!("-----------------------------------------------------");
    println!("{str_store}");
    println!("-----------------------------------------------------");
    Ok(())
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
