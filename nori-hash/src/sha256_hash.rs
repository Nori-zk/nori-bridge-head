#[allow(unused_imports)]
use crate::helios::{serialize_helios_store, serialize_helios_store_serde};
use alloy_primitives::FixedBytes;
use anyhow::Result;
use helios_consensus_core::{consensus_spec::MainnetConsensusSpec, types::LightClientStore};
use sha2_v0_10_8::{Digest, Sha256};

pub fn sha256_hash_helios_store(
    helios_store: &LightClientStore<MainnetConsensusSpec>,
) -> Result<FixedBytes<32>> {
    //print_helios_store(helios_store)?;

    let encoded_store = serialize_helios_store_serde(helios_store)?;

    let hash = Sha256::digest(encoded_store);

    let mut fixed_bytes = [0u8; 32];
    fixed_bytes[..32].copy_from_slice(hash.as_slice());

    Ok(FixedBytes::new(fixed_bytes))
}
