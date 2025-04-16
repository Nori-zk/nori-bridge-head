use anyhow::Result;
use helios_consensus_core::{consensus_spec::MainnetConsensusSpec, types::LightClientStore};

pub fn serialize_helios_store_serde(
    helios_store: &LightClientStore<MainnetConsensusSpec>,
) -> Result<Vec<u8>> {
    let mut result = Vec::new();

    // Always present fields
    result.extend(rmp_serde::to_vec(&helios_store.finalized_header)?);
    result.extend(rmp_serde::to_vec(&helios_store.current_sync_committee)?);
    //result.extend(rmp_serde::to_vec(&helios_store.optimistic_header)?); // We only care about finality

    /*result.extend(rmp_serde::to_vec(
        &helios_store.previous_max_active_participants,
    )?);
    result.extend(rmp_serde::to_vec(
        &helios_store.current_max_active_participants,
    )?);*/ // Commented out as when we are reconstructing these values using get_store_with_next_sync_committee we get a mismatch with current_max_active_participants of 511 vs 512
    // Seemingly current_max_active_participants is only used in helios for saftey checks concerning optimism.

    // Optional fields
    if let Some(next) = &helios_store.next_sync_committee {
        result.extend(rmp_serde::to_vec(next)?);
    } // This is reconstructed in all scenarios with the get_store_with_next_sync_committee where nessesary.

    /*if let Some(best) = &helios_store.best_valid_update {
        result.extend(rmp_serde::to_vec(best)?);
    }*/ // This might be able to be reintroduced due to the introduction of the get_store_with_next_sync_committee reconstruction method.

    Ok(result)
}