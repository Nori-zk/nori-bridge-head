use super::{execution::http::ExecutionHttpProxy, multiplex, query_with_fallback};
use alloy_primitives::{FixedBytes, B256};
use anyhow::{anyhow, Error, Result};
use futures::FutureExt;
use helios_consensus_core::{
    apply_update, calc_sync_period,
    consensus_spec::ConsensusSpec,
    types::{BeaconBlock, FinalityUpdate, Forks, LightClientHeader, LightClientStore, Update},
    verify_update,
};
use helios_ethereum::{
    config::{checkpoints, networks::Network, Config},
    consensus::Inner,
    rpc::{http_rpc::HttpRpc, ConsensusRpc}
};
use log::{debug, info, warn};
use nori_hash::sha256_hash::sha256_hash_helios_store;
use nori_sp1_helios_primitives::types::{ConsensusProofInputs, ProofInputs, ProofInputsWithWindow};
use nori_sp1_helios_program::consensus::consensus_program;
use reqwest::Url;
use std::{env, marker::PhantomData, sync::Arc};
use tokio::sync::{mpsc::channel, watch};
use tokio::time::Duration;
use tree_hash::TreeHash;

pub const MAX_REQUEST_LIGHT_CLIENT_UPDATES: u8 = 128;
const CONSENSUS_RPCS_ENV_VAR: &str = "NORI_SOURCE_CONSENSUS_HTTP_RPCS";
pub const CONSENSUS_PROVIDER_TIMEOUT: Duration = Duration::from_secs(20);

pub struct Client<S: ConsensusSpec, R: ConsensusRpc<S>> {
    inner: Inner<S, R>,
}

impl<S: ConsensusSpec, R: ConsensusRpc<S> + std::fmt::Debug> Client<S, R> {
    /// Constructor
    pub fn new(consensus_rpc: &Url) -> Result<Self> {
        // Parsing chain ID and creating network

        let chain_id = std::env::var("NORI_SOURCE_CHAIN_ID")
            .map_err(|e| Error::msg(format!("NORI_SOURCE_CHAIN_ID not set or invalid: {}", e)))?;

        let network = Network::from_chain_id(
            chain_id
                .parse()
                .map_err(|e| Error::msg(format!("Invalid chain ID format: {}", e)))?,
        )
        .map_err(|e| Error::msg(format!("Failed to convert chain ID to network: {}", e)))?;

        let base_config = network.to_base_config();

        // Configuring client
        let config = Config {
            consensus_rpc: consensus_rpc.clone(),
            execution_rpc: None,
            chain: base_config.chain,
            forks: base_config.forks,
            strict_checkpoint_age: false,
            max_checkpoint_age: 604800, // 1 week
            ..Default::default()
        };

        // Creating the channels for the client
        let (block_send, _) = channel(256);
        let (finalized_block_send, _) = watch::channel(None);
        let (channel_send, _) = watch::channel(None);
        let inner = Inner::<S, R>::new(
            consensus_rpc.as_ref(),
            block_send,
            finalized_block_send,
            channel_send,
            Arc::new(config),
        );

        Ok(Self { inner })
    }

    /// Bootstrap the client from a checkpoint
    pub async fn bootstrap_from_checkpoint(consensus_rpc: &Url, checkpoint: B256) -> Result<Self> {
        let mut client = Client::new(consensus_rpc)?;
        // Bootstrap the client with the checkpoint
        client.inner.bootstrap(checkpoint).await.map_err(|e| {
            Error::msg(format!("Failed to bootstrap client with checkpoint: {}", e))
        })?;
        Ok(client)
    }

    /// Bootstrap the client from a slot
    pub async fn bootstrap_from_slot(consensus_rpc: &Url, slot: u64) -> Result<Self> {
        let client: Client<S, R> = Client::new(consensus_rpc)?;

        // Fetching the block of a slot
        let block: BeaconBlock<S> =
            client.inner.rpc.get_block(slot).await.map_err(|e| {
                Error::msg(format!("Failed to fetch block for slot {}: {}", slot, e))
            })?;

        let checkpoint = B256::from_slice(block.tree_hash_root().as_ref());

        let bootstrap_client: Client<S, R> =
            Client::bootstrap_from_checkpoint(consensus_rpc, checkpoint).await?;

        Ok(bootstrap_client)
    }

    /// Get current finalized header
    pub fn get_current_finalized_header(&self) -> LightClientHeader {
        self.inner.store.finalized_header.clone()
    }

    /// Get current finalized header beacon slot
    pub fn get_current_finalizer_header_beacon_slot(&self) -> u64 {
        self.inner.store.finalized_header.beacon().slot
    }

    /// Get current checkpoint
    pub async fn get_current_checkpoint(&self) -> Result<B256> {
        let slot = self.get_current_finalizer_header_beacon_slot();
        // Fetching the block
        let block: BeaconBlock<S> =
            self.inner.rpc.get_block(slot).await.map_err(|e| {
                Error::msg(format!("Failed to fetch block for slot {}: {}", slot, e))
            })?;

        // Returning the tree hash root as B256
        Ok(B256::from_slice(block.tree_hash_root().as_ref()))
    }

    /// Get latest checkpoint
    pub async fn get_latest_checkpoint() -> Result<B256> {
        let cf = checkpoints::CheckpointFallback::new()
            .build()
            .await
            .map_err(|e| Error::msg(format!("Failed to build checkpoint fallback: {}", e)))?;

        let chain_id = std::env::var("NORI_SOURCE_CHAIN_ID").map_err(|e| {
            Error::msg(format!(
                "NORI_SOURCE_CHAIN_ID environment variable not set: {}",
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
    }

    /// Fetch the latest finality update.
    pub async fn get_latest_finality_update(&self) -> Result<FinalityUpdate<S>> {
        // Get finality update
        let finality_update: FinalityUpdate<S> = self
            .inner
            .rpc
            .get_finality_update()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch finality update via RPC: {}", e))?;
        Ok(finality_update)
    }

    /// Fetch the latest finality slot height.
    pub async fn get_latest_finality_slot(&self) -> Result<u64> {
        // Get finality slot
        let finality_update = self.get_latest_finality_update().await?;

        // Extract latest slot
        Ok(finality_update.finalized_header().beacon().slot)
    }

    /// Fetch updates for client
    pub async fn get_updates(&self) -> Result<Vec<Update<S>>> {
        let period = calc_sync_period::<S>(self.inner.store.finalized_header.beacon().slot);

        // Handling the result and converting errors to anyhow::Error
        let updates_result = self
            .inner
            .rpc
            .get_updates(period, MAX_REQUEST_LIGHT_CLIENT_UPDATES)
            .await
            .map_err(|e| Error::msg(e.to_string())); // Convert error to anyhow::Error

        match updates_result {
            Ok(updates) => Ok(updates.clone()), // Clone the updates if the result is Ok
            Err(e) => Err(e),                   // Propagate error if it's an Err
        }
    }

    /// Get expected slot
    pub fn expected_current_slot(&self) -> u64 {
        self.inner.expected_current_slot()
    }

    /// Fetch first update for client
    pub async fn get_first_update(&self) -> Result<Update<S>> {
        let period = calc_sync_period::<S>(self.get_current_finalizer_header_beacon_slot());

        // Handling the result and converting errors to anyhow::Error
        let updates_result = self
            .inner
            .rpc
            .get_updates(period, 1)
            .await
            .map_err(|e| Error::msg(e.to_string())); // Convert error to anyhow::Error

        match updates_result {
            Ok(mut updates) => Ok(updates.get_mut(0).unwrap().clone()), // Clone the updates if the result is Ok
            Err(e) => Err(e), // Propagate error if it's an Err
        }
    }

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
        mut store: LightClientStore<S>,
        genesis_root: &FixedBytes<32>,
        forks: &Forks,
        first_update: &Update<S>,
    ) -> Result<LightClientStore<S>> {
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
    pub async fn get_latest_finality_slot_and_store_hash(
        consensus_rpc: &Url,
    ) -> Result<(u64, FixedBytes<32>)> {
        // Get latest beacon checkpoint
        info!("Fetching cold start client from latest checkpoint");
        let latest_checkpoint = Client::<S, R>::get_latest_checkpoint().await?;

        // Get the client from the beacon checkpoint
        let client =
            Client::<S, R>::bootstrap_from_checkpoint(consensus_rpc, latest_checkpoint).await?;

        // Get slot head from checkpoint
        let slot = client.get_current_finalizer_header_beacon_slot();
        info!("Retrieved cold start slot: {} ", slot);

        // Get sync update
        info!("Getting cold start first update");
        let first_update = client.get_first_update().await?;

        // Get synced store
        info!("Syncing bootstrapped cold start store");
        let synced_store = Client::<S, R>::get_store_with_next_sync_committee(
            client.inner.expected_current_slot(),
            client.inner.store,
            &client.inner.config.chain.genesis_root,
            &client.inner.config.forks,
            &first_update,
        )?;

        // Get the store hash
        info!("Calculating cold start store hash");
        let store_hash = sha256_hash_helios_store(&synced_store)?;
        info!("Calculated cold start store hash: {}", store_hash);

        Ok((slot, store_hash))
    }

    /// Prepares a consensus proof input for a sp1 helios slot transition proof
    /// # Arguments
    /// * `consensus_rpc` - Url of the consensus RPC to use to prepare a store
    /// * `input_head` - Target slot number to prove from up until current finality head
    /// * `store_hash` - The previous hash of the helios client store state at the `input_head` slot
    pub async fn prepare_consensus_proof_inputs(
        consensus_rpc: &Url,
        input_slot: u64,
        store_hash: FixedBytes<32>,
    ) -> Result<ConsensusProofInputs<S>> {
        let mut client: Client<S, R> =
            Client::bootstrap_from_slot(consensus_rpc, input_slot).await?;

        let genesis_root = &client.inner.config.chain.genesis_root;
        let forks = &client.inner.config.forks;

        // Get finality update
        debug!("Getting finality update from input slot {}", input_slot);
        let finality_update = client.get_latest_finality_update().await?;

        // Get sync commitee updates
        debug!("Getting sync commitee updates.");
        let mut updates = client.get_updates().await?;

        // Panic if our updates were empty (not sure how to deal with this yet)
        if updates.is_empty() {
            return Err(anyhow::anyhow!("Error updates were missing 0th update."));
        }

        // Get the expected current slot from the client
        let expected_current_slot = client.expected_current_slot();

        // Get a synced bootstrapped store.
        let store: LightClientStore<S> = {
            // Get a reference to the first update
            let first_update = &updates[0];
            // Get the finalized_header beacon slot of the first update
            let first_update_slot = first_update.finalized_header().beacon().slot;

            // If the 0th updates finalized_header beacon slot is lower than our input_head then we can apply
            // this update to our bootstrapped client to bring it in sync with the input_head. This puts the
            // next_sync_committee onto our store as well as other state and bring the store up to date
            // with its state restored back to the same condition as the terminal ("updated") store state in
            // the last zk programs invocation
            if first_update_slot < input_slot {
                // FIXME should this be <= ?
                debug!("First update finalized header beacon slot is less than our input head.");
                // Remove the zeroth update from the updates.
                let first_update = updates.remove(0);
                // Validate the update.
                verify_update(
                    &first_update,
                    expected_current_slot,
                    &client.inner.store,
                    *genesis_root,
                    forks,
                )
                .map_err(|e| anyhow::anyhow!("Verify update failed: {}", e))?;
                // Apply the update to the client... mutating its store
                apply_update(&mut client.inner.store, &first_update);
                client.inner.store
            } else {
                debug!("First update finalized header beacon slot is greater than or equal to our input head.");
                // The 0th update is ahead of the input_head, the next_sync_committee and other store state
                // need to be restored without advancing the store in terms of slot.
                // This can happen if either we are exactly on a period transition (every 32*256 slots) or
                // if our client is very behind due to being offline while a period transition (or more than one)
                // has occured.
                Client::<S, R>::get_store_with_next_sync_committee(
                    expected_current_slot,
                    client.inner.store,
                    genesis_root,
                    forks,
                    first_update,
                )?
            }
        };

        // Create program inputs
        debug!("Building sp1 proof inputs.");
        let proof_inputs = ConsensusProofInputs {
            updates,
            finality_update,
            expected_current_slot,
            store,
            genesis_root: *genesis_root,
            forks: forks.clone(),
            store_hash,
        };
        debug!("Built sp1 proof inputs.");

        Ok(proof_inputs)
    }
}

// Ok now need the multiplexing logic so we can run multiple operations over multiple RPCS

pub struct ConsensusHttpProxy<S: ConsensusSpec, R: ConsensusRpc<S>> {
    principal_provider_url: Url,
    backup_providers_urls: Vec<Url>,
    all_providers_urls: Vec<Url>,
    _marker: PhantomData<(S, R)>,
    validation_timeout: Duration,
}

impl<S: ConsensusSpec, R: ConsensusRpc<S> + std::fmt::Debug> ConsensusHttpProxy<S, R> {
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok();

        // Parsing the proof validation timeout
        let validation_timeout_sec = std::env::var("NORI_CONSENSUS_PROOF_INPUT_VALIDATION_TIMEOUT")
            .ok()
            .map(|v| v.parse::<u64>().map_err(|e| Error::msg(format!("Failed to parse NORI_CONSENSUS_PROOF_INPUT_VALIDATION_TIMEOUT as u64: {}", e))))
            .transpose()?
            .unwrap_or(300);

        let validation_timeout = Duration::from_secs(validation_timeout_sec);

        let urls: Vec<Url> = env::var(CONSENSUS_RPCS_ENV_VAR)?
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

        let backup_providers_urls = urls[1..].to_vec();

        Ok(ConsensusHttpProxy::<S, R> {
            principal_provider_url,
            backup_providers_urls,
            all_providers_urls: urls,
            _marker: PhantomData,
            validation_timeout
        })
    }

    pub fn try_from_env() -> Self {
        ConsensusHttpProxy::<S, R>::from_env().unwrap()
    }

    /// Queries RPC providers for raw state updates, locally constructs proof inputs,
    /// and runs the zkVM consensus and mpt logic in native code (not in a VM or zk prover),
    /// simulating the state transition and verifying its validity.
    ///
    /// This validation ensures the transition from `input_slot` to `output_slot`
    /// is consistent with consensus and mpt rules before any zk proof is generated.
    ///
    /// # Arguments
    /// * `input_slot` - The starting slot number for the state transition.
    /// * `store_hash` - The hash of the client store state at the `input_slot`.
    /// * `validate` - Whether or not validation rules of output_slot > input_slot,
    ///   next_sync_commitee is non zero and output_slot % 32 is non zero are applied.
    ///
    /// # Returns
    /// Tuple of (input slot, output slot, validated proof inputs).
    ///
    /// # Errors
    /// Returns an error if the consensus / mpt proof is invalid or if all providers fail to supply
    /// valid raw updates that produce a consistent state transition.
    pub async fn prepare_consensus_mpt_proof_inputs(
        &self,
        input_slot: u64,
        store_hash: FixedBytes<32>,
        validate: bool
    ) -> Result<ProofInputsWithWindow<S>> {
        // TODO move this function out of here its a bit strange to have the consensus and execution rpcs here
        // Deserves it own location
        let (input_slot, output_slot, validated_consensus_proof_inputs, expected_output_store_hash) = multiplex(
            |url| {
                async move {
                    // Fetch proof_inputs
                    let consensus_proof_inputs = Client::<S, R>::prepare_consensus_proof_inputs(
                        &url, input_slot, store_hash,
                    )
                    .await?;

                    // Run the CPU-heavy program and slot validation inside spawn_blocking
                    let (output_slot, validated_proof_inputs, expected_output_store_hash) =
                        tokio::task::spawn_blocking(move || {
                            // Run program logic
                            let proof_outputs = consensus_program(consensus_proof_inputs.clone())?;

                            // Convert newHead to u64
                            let output_slot = proof_outputs.output_slot;

                            // Validate progression
                            if validate && output_slot <= input_slot {
                                return Err(anyhow::anyhow!(
                                    "Output slot {} was not greater than input slot {}",
                                    output_slot,
                                    input_slot
                                ));
                            }

                            // The below didnt work as well as we had hoped https://github.com/Nori-zk/nori-bridge-head/issues/10
                            // Block non-checkpoint slots (they prevent bootstrapping on restart)
                            if validate && output_slot % 32 > 0 { 
                                // FIXME might need the validate_progress guard as its used as a flag to allow
                                // the proof anyway. And for vk building we need to be able to arbirarily bypass this 
                                // sort of validation.
                                return Err(anyhow::anyhow!(
                                    "Output slot {} was a non-checkpoint slot. Preventing this as it prevents bootstrapping if we go offline.",
                                    output_slot,
                                ));
                            }

                            if validate && proof_outputs.next_sync_committee_hash == B256::ZERO {
                                return Err(anyhow::anyhow!(
                                    "Next sync committee was zero. Preventing this as it could stop the recovery of the input_store_hash when restarting.",
                                ));
                            }

                            // what about??
                            //proof_outputs.output_store_hash;

                            Ok((output_slot, consensus_proof_inputs, proof_outputs.output_store_hash))
                        })
                        .await??;

                        // Block non-checkpoint slots
                        // This replaces the output_slot % 32 > 0 validation check.
                        // Instead of checking the output_slot number % 32 lets try to bootstrap from this slot explicitly
                        // NOTE THIS STRATEGY DID NOT WORK....
                        if validate {
                            Client::<S, R>::bootstrap_from_slot(&url, output_slot).await
                            .map_err(|e| anyhow::anyhow!(
                                "Failed to bootstrap from slot {} using {}. Preventing the use of this output_slot as it could lead to a stall of the bridge:\n{}",
                                output_slot, url, e
                            ))?;
                        }


                    Ok((input_slot, output_slot, validated_proof_inputs, expected_output_store_hash))
                }
                .boxed()
            },
            &self.all_providers_urls,
            self.validation_timeout,
        )
        .await?;

        // Get input and output block numbers
        let finalized_input_block_number = *validated_consensus_proof_inputs
            .store
            .finalized_header
            .execution()
            .map_err(|_| {
                anyhow::Error::msg("Failed to get input finalized execution header".to_string())
            })?
            .block_number();

        let finalized_output_block_number = *validated_consensus_proof_inputs
            .finality_update
            .finalized_header()
            .execution()
            .map_err(|_| {
                anyhow::Error::msg("Failed to get output finalized execution header".to_string())
            })?
            .block_number();

        // Get Execution Proxy (Note this is a bit messy to do this here now FIXME)
        let validated_consensus_mpt_proof_input_with_window = ExecutionHttpProxy::<S>::try_from_env()
            .prepare_consensus_mpt_proof_inputs(
                input_slot,
                output_slot,
                finalized_input_block_number,
                finalized_output_block_number,
                validated_consensus_proof_inputs,
                expected_output_store_hash
            )
            .await?;

        //validated_consensus_mpt_proof_input_with_window. what about the output store hash?

        Ok(validated_consensus_mpt_proof_input_with_window)
    }

    /// Get the latest slot & store hash from the latest finality checkpoint.
    pub async fn get_latest_finality_slot_and_store_hash(&self) -> Result<(u64, FixedBytes<32>)> {
        // This is used in cold start procedure which is a trusted operation (hence the principle trusted endpoint).
        // FIXME this bootstrap needs to be more strictly defined
        // but leaving this for now.
        query_with_fallback(
            &self.principal_provider_url,
            &Vec::new(),
            |url| {
                async move { Client::<S, R>::get_latest_finality_slot_and_store_hash(&url).await }
                    .boxed()
            },
            CONSENSUS_PROVIDER_TIMEOUT,
        )
        .await
    }

    // Get latest_finality_slot
    pub async fn get_latest_finality_slot(&self) -> Result<u64> {
        multiplex(
            |url| {
                async move {
                    let checkpoint = Client::<S, R>::get_latest_checkpoint().await?;
                    let client =
                        Client::<S, R>::bootstrap_from_checkpoint(&url, checkpoint).await?;
                    client.get_latest_finality_slot().await
                }
                .boxed()
            },
            &self.all_providers_urls,
            CONSENSUS_PROVIDER_TIMEOUT,
        )
        .await
    }
}
