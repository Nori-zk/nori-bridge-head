use alloy_primitives::FixedBytes;
use anyhow::{anyhow, Result};
use helios_consensus_core::{
    apply_update,
    consensus_spec::MainnetConsensusSpec,
    types::LightClientStore,
    verify_update,
};
use helios_ethereum::rpc::http_rpc::HttpRpc;
use log::{debug, info, warn};
use nori::rpcs::consensus::Client;
use nori_hash::sha256_hash::sha256_hash_helios_store;
use reqwest::Url;

use std::env;

const CONSENSUS_RPCS_ENV_VAR: &str = "NORI_SOURCE_CONSENSUS_HTTP_RPCS";

/// Builds the same `store` that `Client::prepare_consensus_proof_inputs` would build for
/// `input_slot`, then hashes it. The runtime feeds this hash into the zk program as
/// `input_store_hash`; the program independently rebuilds the store and asserts the hash
/// matches — so this script must mirror `prepare_consensus_proof_inputs` exactly.
pub async fn generate_inital_store_hash_from_input_slot(input_slot: u64) -> Result<FixedBytes<32>> {
    dotenv::dotenv().ok();

    let urls: Vec<Url> = env::var(CONSENSUS_RPCS_ENV_VAR)
        .map_err(|e| anyhow!("{} not set: {}", CONSENSUS_RPCS_ENV_VAR, e))?
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| match s.parse::<Url>() {
            Ok(u) => Some(u),
            Err(e) => {
                warn!("Skipping invalid URL '{}': {}", s, e);
                None
            }
        })
        .collect();

    let principal_provider_url = urls.first().cloned().ok_or_else(|| {
        anyhow!(
            "No valid consensus RPC URLs found in {}",
            CONSENSUS_RPCS_ENV_VAR
        )
    })?;

    let client: Client<MainnetConsensusSpec, HttpRpc> =
        Client::bootstrap_from_slot(&principal_provider_url, input_slot).await?;

    let inner = client.get_inner_client();
    let genesis_root = inner.config.chain.genesis_root;
    let forks = inner.config.forks.clone();
    let expected_current_slot = client.expected_current_slot();
    let mut bootstrap_store = inner.store.clone();

    info!("Fetching sync committee updates");
    let mut updates = client.get_updates().await?;
    if updates.is_empty() {
        return Err(anyhow!("No sync committee updates returned for input slot"));
    }

    // Mirrors `prepare_consensus_proof_inputs` (nori/src/rpcs/consensus/mod.rs):
    // when the first update's finalized slot is older than `input_slot`, the runtime
    // applies it directly into the bootstrap store; otherwise it reconstructs the
    // committee while preserving the bootstrap slot.
    let store: LightClientStore<MainnetConsensusSpec> = {
        let first_update_slot = updates[0].finalized_header().beacon().slot;

        if first_update_slot < input_slot {
            debug!("First update slot {} < input slot {}: applying update", first_update_slot, input_slot);
            let first_update = updates.remove(0);
            verify_update(
                &first_update,
                expected_current_slot,
                &bootstrap_store,
                genesis_root,
                &forks,
            )
            .map_err(|e| anyhow!("Verify update failed: {}", e))?;
            apply_update(&mut bootstrap_store, &first_update);
            bootstrap_store
        } else {
            debug!("First update slot {} >= input slot {}: reconstructing committee", first_update_slot, input_slot);
            Client::<MainnetConsensusSpec, HttpRpc>::get_store_with_next_sync_committee(
                expected_current_slot,
                bootstrap_store,
                &genesis_root,
                &forks,
                &updates[0],
            )?
        }
    };

    info!("Calculating store hash");
    let store_hash = sha256_hash_helios_store(&store)?;
    info!("Store hash for slot {}: {}", input_slot, store_hash);
    Ok(store_hash)
}

#[tokio::main]
async fn main() -> Result<()> {
    let input_slot = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("Please provide an input slot as the first argument"))?
        .parse::<u64>()
        .map_err(|e| anyhow!("Failed to parse input slot as u64: {}", e))?;
    let store_hash = generate_inital_store_hash_from_input_slot(input_slot).await?;
    println!("Initial store hash for slot {}: {}", input_slot, store_hash);
    Ok(())
}
