use anyhow::Result;
use helios_consensus_core::{
    consensus_spec::MainnetConsensusSpec,
    types::{LightClientHeader, LightClientStore},
};
/*use kimchi::{
    mina_curves::pasta::Fp,
    mina_poseidon::{
        constants::PlonkSpongeConstantsKimchi,
        pasta::fp_kimchi,
        poseidon::{ArithmeticSponge as Poseidon, Sponge as _},
    },
};*/
use serde::Serialize;

/*fn poseidon_hash(input: &[Fp]) -> Fp {
    let mut hash = Poseidon::<Fp, PlonkSpongeConstantsKimchi>::new(fp_kimchi::static_params());
    hash.absorb(input);
    hash.squeeze()
}*/

/*fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut hex_string = String::with_capacity(bytes.len() * 2);

    for &byte in bytes {
        hex_string.push(HEX_CHARS[(byte >> 4) as usize] as char);
        hex_string.push(HEX_CHARS[(byte & 0x0F) as usize] as char);
    }

    hex_string
}*/

/*fn serialize_to_hex(helios_store: &LightClientStore<MainnetConsensusSpec>) -> Result<String> {
    let mut serializer = Vec::new();
    helios_store.finalized_header.serialize(&mut serializer)?; // Not going to work as we cannot go to vec8 directly perhaps use bincode which has typescript support
    // https://github.com/4t145/bincode-ts
    let hex_string = bytes_to_hex(&serializer);
    Ok(hex_string)
}*/

// we need to go to bytes somehow?

pub fn poseidon_hash_helios_store(helios_store: LightClientStore<MainnetConsensusSpec>) {
    

    // let finalized_header = helios_store.finalized_header.serialize(serializer);
    //let _ = helios_store.finalized_header.beacon().slot;

    // helios_store.finalized_header.beacon().slot;

    // Fp::from(value)

    /*

        pub finalized_header: LightClientHeader,
    pub current_sync_committee: SyncCommittee<S>,
    pub next_sync_committee: Option<SyncCommittee<S>>,
    pub optimistic_header: LightClientHeader,
    pub previous_max_active_participants: u64,
    pub current_max_active_participants: u64,
    pub best_valid_update: Option<GenericUpdate<S>>,

     */

     // member1 leaf1 concat leaf2 concact
    // 

    // Unpack values and cast to fields
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
prevHead: U256::from(prev_head), // Matthew is this nessesary prev_head is u64 and perhaps isnt provable??

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