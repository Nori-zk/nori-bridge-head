use anyhow::{Result,Context};
use helios_consensus_core::{consensus_spec::MainnetConsensusSpec, types::LightClientStore};

// Debugging store print

pub fn print_helios_store(helios_store: &LightClientStore<MainnetConsensusSpec>) -> Result<()> {
    // The easiest way would be to serialise it via serde json
    let str_store =
        serde_json::to_string(&helios_store).context("Failed to serialize helios store")?;
    println!("-----------------------------------------------------");
    println!("{str_store}");
    println!("-----------------------------------------------------");
    Ok(())
}
