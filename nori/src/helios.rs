use alloy_primitives::{FixedBytes, B256};
use anyhow::{Error, Result};
use helios_consensus_core::{
    apply_update, calc_sync_period,
    consensus_spec::MainnetConsensusSpec,
    types::{BeaconBlock, FinalityUpdate, Forks, LightClientStore, Update},
    verify_update,
};
use helios_ethereum::rpc::ConsensusRpc;
use helios_ethereum::{
    config::{checkpoints, networks::Network, Config},
    consensus::Inner,
    rpc::http_rpc::HttpRpc,
};
use log::info;
use nori_hash::sha256_hash::sha256_hash_helios_store;
use std::sync::Arc;
use tokio::sync::{mpsc::channel, watch};
use tree_hash::TreeHash;

pub const MAX_REQUEST_LIGHT_CLIENT_UPDATES: u8 = 128;

/// Updates a cloned `LightClientStore` with next sync committee data from the provided update.
///
/// This function:
/// 1. Creates a clone of the input store
/// 2. Verifies the provided update against the original store
/// 3. Applies the update to the original store (mutating it)
/// 4. Copies the restored next sync committee data and other state to the cloned store
/// 5. Returns the cloned store with restored next sync committee data
///
/// The original store is modified during this process, but the returned store maintains
/// the original slot while containing the new committee information.
///
/// # Arguments
///
/// * `expected_current_slot` - The slot the light client expects to be current
/// * `store` - The store to clone and update (will be modified during verification)
/// * `genesis_root` - The genesis root of the chain
/// * `forks` - The fork schedule for the chain
/// * `first_update` - The update containing the new sync committee information
///
/// # Returns
///
/// Returns a `Result` containing:
/// - `Ok(LightClientStore)` with:
///   - Original slot information
///   - Updated `next_sync_committee` from the applied update
///   - Updated participant counts from the applied update
/// - `Err(anyhow::Error)` if:
///   - Update verification fails
///
/// # Example
///
/// ```rust
/// let updated_store = get_store_with_next_sync_committee(
///     expected_slot,
///     original_store,
///     &genesis_root,
///     &forks,
///     &first_update
/// )?;
/// ```
pub fn get_store_with_next_sync_committee(
    expected_current_slot: u64,
    mut store: LightClientStore<MainnetConsensusSpec>,
    genesis_root: &FixedBytes<32>,
    forks: &Forks,
    first_update: &Update<MainnetConsensusSpec>,
) -> Result<LightClientStore<MainnetConsensusSpec>> {
    // Copy the original store
    let mut store_clone = store.clone();

    // Verify the first update
    verify_update(
        first_update,
        expected_current_slot,
        &store,
        *genesis_root,
        forks,
    )
    .map_err(|e| anyhow::anyhow!("Verify update failed: {}", e))?;

    // Apply first update (get our bootstrapped client in sync, this gives us the missing next_sync_committee etc)
    apply_update(&mut store, first_update);

    // Copy our next sync committee and other store state to the cloned store (which has not advanced compared to the original bootstrapped slot)
    store_clone.next_sync_committee = store.next_sync_committee;
    store_clone.previous_max_active_participants = store.previous_max_active_participants;
    store_clone.current_max_active_participants = store.current_max_active_participants;
    //store_clone.best_valid_update = client.store.best_valid_update; // Perhaps re introduce this

    Ok(store_clone)
}

/// Get the latest slot & store hash from the latest finality checkpoint.
pub async fn get_latest_finality_head_and_store_hash() -> Result<(u64, FixedBytes<32>)> {
    // Get latest beacon checkpoint
    info!("Fetching cold start client from latest checkpoint");
    let latest_checkpoint = get_latest_checkpoint().await?;

    // Get the client from the beacon checkpoint
    let helios_client = get_client(latest_checkpoint).await?;

    // Get slot head from checkpoint
    let slot_head = helios_client.store.finalized_header.clone().beacon().slot;
    info!("Retrieved cold start head: {} ", slot_head);

    // Get sync update
    info!("Getting cold start first update");
    let first_update = get_first_update(&helios_client).await?;

    // Get synced store
    info!("Syncing bootstrapped cold start store");
    let synced_store = get_store_with_next_sync_committee(
        helios_client.expected_current_slot(),
        helios_client.store,
        &helios_client.config.chain.genesis_root,
        &helios_client.config.forks,
        &first_update,
    )?;

    // Get the store hash
    info!("Calculating cold start store hash");
    let store_hash = sha256_hash_helios_store(&synced_store)?;
    info!("Calculated cold start store hash: {}", store_hash);

    Ok((slot_head, store_hash))
}

/// Fetch the latest finality slot height.
pub async fn get_client_latest_finality_head(
    client: &Inner<MainnetConsensusSpec, HttpRpc>,
) -> Result<u64> {
    // Get finality slot head
    let finality_update: FinalityUpdate<MainnetConsensusSpec> = client
        .rpc
        .get_finality_update()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch finality update via RPC: {}", e))?;

    // Extract latest slot
    Ok(finality_update.finalized_header().beacon().slot)
}

/// Fetch updates for client.
pub async fn get_updates(
    client: &Inner<MainnetConsensusSpec, HttpRpc>,
) -> Result<Vec<Update<MainnetConsensusSpec>>> {
    let period =
        calc_sync_period::<MainnetConsensusSpec>(client.store.finalized_header.beacon().slot);

    // Handling the result and converting errors to anyhow::Error
    let updates_result = client
        .rpc
        .get_updates(period, MAX_REQUEST_LIGHT_CLIENT_UPDATES)
        .await
        .map_err(|e| Error::msg(e.to_string())); // Convert error to anyhow::Error

    match updates_result {
        Ok(updates) => Ok(updates.clone()), // Clone the updates if the result is Ok
        Err(e) => Err(e),                   // Propagate error if it's an Err
    }
}

/// Fetch first update for client
pub async fn get_first_update(
    client: &Inner<MainnetConsensusSpec, HttpRpc>,
) -> Result<Update<MainnetConsensusSpec>> {
    let period =
        calc_sync_period::<MainnetConsensusSpec>(client.store.finalized_header.beacon().slot);

    // Handling the result and converting errors to anyhow::Error
    let updates_result = client
        .rpc
        .get_updates(period, 1)
        .await
        .map_err(|e| Error::msg(e.to_string())); // Convert error to anyhow::Error

    match updates_result {
        Ok(mut updates) => Ok(updates.get_mut(0).unwrap().clone()), // Clone the updates if the result is Ok
        Err(e) => Err(e), // Propagate error if it's an Err
    }
}

/// Fetch latest checkpoint from chain to bootstrap client to the latest state.
pub async fn get_latest_checkpoint() -> Result<B256> {
    let cf = checkpoints::CheckpointFallback::new()
        .build()
        .await
        .map_err(|e| Error::msg(format!("Failed to build checkpoint fallback: {}", e)))?;

    let chain_id = std::env::var("SOURCE_CHAIN_ID").map_err(|e| {
        Error::msg(format!(
            "SOURCE_CHAIN_ID environment variable not set: {}",
            e
        ))
    })?;

    let network = Network::from_chain_id(
        chain_id
            .parse()
            .map_err(|e| Error::msg(format!("Invalid chain ID format: {}", e)))?,
    )
    .map_err(|e| Error::msg(format!("Failed to convert chain ID to network: {}", e)))?;

    cf.fetch_latest_checkpoint(&network)
        .await
        .map_err(|e| Error::msg(format!("Failed to fetch latest checkpoint: {}", e)))
    // Convert error with context
}

/// Fetch checkpoint from a slot number.
pub async fn get_checkpoint(slot: u64) -> Result<B256> {
    // Fetching environment variables
    let consensus_rpc = std::env::var("SOURCE_CONSENSUS_RPC_URL").map_err(|e| {
        Error::msg(format!(
            "SOURCE_CONSENSUS_RPC_URL not set or invalid: {}",
            e
        ))
    })?;

    let chain_id = std::env::var("SOURCE_CHAIN_ID")
        .map_err(|e| Error::msg(format!("SOURCE_CHAIN_ID not set or invalid: {}", e)))?;

    // Parsing chain ID and creating network
    let network = Network::from_chain_id(
        chain_id
            .parse()
            .map_err(|e| Error::msg(format!("Invalid chain ID format: {}", e)))?,
    )
    .map_err(|e| Error::msg(format!("Failed to convert chain ID to network: {}", e)))?;

    let base_config = network.to_base_config();

    // Configuring client
    let config = Config {
        consensus_rpc: consensus_rpc.to_string(),
        execution_rpc: String::new(),
        chain: base_config.chain,
        forks: base_config.forks,
        strict_checkpoint_age: false,
        ..Default::default()
    };

    // Creating the channels for the client
    let (block_send, _) = channel(256);
    let (finalized_block_send, _) = watch::channel(None);
    let (channel_send, _) = watch::channel(None);
    let client = Inner::<MainnetConsensusSpec, HttpRpc>::new(
        &consensus_rpc,
        block_send,
        finalized_block_send,
        channel_send,
        Arc::new(config),
    );

    // Fetching the block
    let block: BeaconBlock<MainnetConsensusSpec> = client
        .rpc
        .get_block(slot)
        .await
        .map_err(|e| Error::msg(format!("Failed to fetch block for slot {}: {}", slot, e)))?;

    // Returning the tree hash root as B256
    Ok(B256::from_slice(block.tree_hash_root().as_ref()))
}

/// Setup a client from a checkpoint.
pub async fn get_client(checkpoint: B256) -> Result<Inner<MainnetConsensusSpec, HttpRpc>> {
    // Fetching environment variables
    let consensus_rpc = std::env::var("SOURCE_CONSENSUS_RPC_URL").map_err(|e| {
        Error::msg(format!(
            "SOURCE_CONSENSUS_RPC_URL not set or invalid: {}",
            e
        ))
    })?;

    let chain_id = std::env::var("SOURCE_CHAIN_ID")
        .map_err(|e| Error::msg(format!("SOURCE_CHAIN_ID not set or invalid: {}", e)))?;

    // Parsing chain ID and creating network
    let network = Network::from_chain_id(
        chain_id
            .parse()
            .map_err(|e| Error::msg(format!("Invalid chain ID format: {}", e)))?,
    )
    .map_err(|e| Error::msg(format!("Failed to convert chain ID to network: {}", e)))?;

    let base_config = network.to_base_config();

    // Configuring client
    let config = Config {
        consensus_rpc: consensus_rpc.to_string(),
        execution_rpc: String::new(),
        chain: base_config.chain,
        forks: base_config.forks,
        strict_checkpoint_age: false,
        ..Default::default()
    };

    // Creating the channels for the client
    let (block_send, _) = channel(256);
    let (finalized_block_send, _) = watch::channel(None);
    let (channel_send, _) = watch::channel(None);
    let mut client = Inner::<MainnetConsensusSpec, HttpRpc>::new(
        &consensus_rpc,
        block_send,
        finalized_block_send,
        channel_send,
        Arc::new(config),
    );

    // Bootstrap the client with the checkpoint
    client
        .bootstrap(checkpoint)
        .await
        .map_err(|e| Error::msg(format!("Failed to bootstrap client with checkpoint: {}", e)))?;

    // Return the initialized client
    Ok(client)
}
