use crate::helios::{serialize_helios_store, serialize_helios_store_serde};
use alloy_primitives::FixedBytes;
use anyhow::{Result,Context};
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

    Ok(FixedBytes::new(fixed_bytes)) // try hash in here we we trust this!
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
