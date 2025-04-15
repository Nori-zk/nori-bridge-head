use alloy_primitives::{FixedBytes, B256};
use helios_consensus_core::{
    calc_sync_period,
    consensus_spec::MainnetConsensusSpec,
    types::{BeaconBlock, FinalityUpdate, LightClientStore, SyncCommittee, Update},
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
use anyhow::{Error, Result};

pub const MAX_REQUEST_LIGHT_CLIENT_UPDATES: u8 = 128;

pub async fn get_store_with_next_sync_committee(
    input_head: u64,
) -> Result<LightClientStore<MainnetConsensusSpec>> {
    // Get latest beacon checkpoint
    let helios_checkpoint = get_checkpoint(input_head).await?;

    // Re init helios client
    let mut helios_update_client = get_client(helios_checkpoint).await?;

    // Option for next_sync_committee
    let mut next_sync_committee: Option<SyncCommittee<MainnetConsensusSpec>> = None;

    // u64's for counts
    let previous_max_active_participants: u64;
    let current_max_active_participants: u64;

    // Get sync committee updates
    let mut sync_committee_updates = get_finality_updates(&helios_update_client).await?; // FIXME THIS IS GETTING LOTS OF UPDATES

    if !sync_committee_updates.is_empty() {
        let temp_update = sync_committee_updates.remove(0);

        let finalized_beacon_slot = {
            temp_update.finalized_header().beacon().slot
        };

        info!("Frankenstein update finality beacon slot {}", finalized_beacon_slot);

        helios_update_client
            .verify_update(&temp_update)
            .map_err(|e| Error::msg(format!("Proof invalid: {}", e)))?; // FIXME what to do with this!
        helios_update_client.apply_update(&temp_update);

        next_sync_committee = helios_update_client.store.next_sync_committee.clone();
        previous_max_active_participants = helios_update_client.store.previous_max_active_participants; // need to think abuot this refer to helios
        current_max_active_participants = helios_update_client.store.current_max_active_participants;
        // but alteast safety_threshold seems to only apply to optimism checkme

        /*
        store.current_max_active_participants =
        u64::max(store.current_max_active_participants, committee_bits); ??
         */
        //return Ok(helios_update_client.store.next_sync_committee);
    }
    else {
        // FIXME this should be zeros nessesarily
        previous_max_active_participants = 0; // helios_update_client.store.previous_max_active_participants
        current_max_active_participants = 0; // helios_update_client.store.current_max_active_participants
        // perhaps this was the issue with the cold start?! we got an invalid update when we gave it the hash... think but might also be as we didnt remove the 0th update (pre apply)
    }

    // Get latest beacon checkpoint
    let helios_checkpoint = get_checkpoint(input_head).await?;

    // Re init helios client
    let helios_update_client: Inner<MainnetConsensusSpec, HttpRpc> = get_client(helios_checkpoint).await?;

    let mut helios_store = helios_update_client.store.clone();
    helios_store.next_sync_committee = next_sync_committee;
    helios_store.previous_max_active_participants = previous_max_active_participants;
    helios_store.current_max_active_participants = current_max_active_participants;

    Ok(helios_store)
}


pub async fn get_latest_finality_head_and_store_hash(
) -> Result<(u64, FixedBytes<32>)> {
    // Get latest beacon checkpoint
    info!("Fetching cold start client from latest checkpoint");
    let latest_checkpoint = get_latest_checkpoint().await?;

    // Get the client from the beacon checkpoint
    let helios_client = get_client(latest_checkpoint).await?;

    // Get slot head from checkpoint
    let slot_head = helios_client.store.finalized_header.clone().beacon().slot;
    info!("Retrieved cold start head: {} ", slot_head);

    // Get the store hash
    info!("Calculating cold start store hash");
    //let store = get_store_with_next_sync_committee(slot_head).await?; // ensures we put the next sync committee upon the cold start store.
    let store_hash = sha256_hash_helios_store(&helios_client.store)?;
    info!("Calculated cold start store hash: {}", store_hash);

    Ok((slot_head, store_hash))
}

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

pub async fn get_finality_updates(
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

/// Fetch updates for client
pub async fn get_updates(
    client: &Inner<MainnetConsensusSpec, HttpRpc>,
) -> anyhow::Result<Vec<Update<MainnetConsensusSpec>>> {
    let updates_result = client
        .rpc
        .get_updates(
            calc_sync_period::<MainnetConsensusSpec>(client.store.finalized_header.beacon().slot),
            MAX_REQUEST_LIGHT_CLIENT_UPDATES,
        )
        .await
        .map_err(|e| Error::msg(e.to_string())); // Convert the error into anyhow::Error

    match updates_result {
        Ok(updates) => Ok(updates.clone()), // Clone only the Vec<Update> inside the Ok variant
        Err(e) => Err(e),                   // Propagate the error if it's an Err
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
