use crate::helios::{serialize_helios_store, serialize_helios_store_serde};
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
    //print_helios_store(helios_store)?;

    // Fp = Fp256<...>

    let encoded_store = serialize_helios_store_serde(helios_store)?;
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