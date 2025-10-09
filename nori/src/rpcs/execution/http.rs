use crate::{
    contracts::bindings::{
        addresses_attestation_pair_to_storage_slots, get_source_contract_address, NoriStateBridge,
    },
    rpcs::query_with_fallback,
};
use alloy::{
    eips::BlockId,
    network::Ethereum,
    providers::{Provider, ProviderBuilder, RootProvider},
    rpc::types::{EIP1186AccountProofResponse, Filter},
    sol_types::SolEvent,
    transports::http::Http,
};
use alloy_primitives::{Address, FixedBytes, Log, B256};
use anyhow::{anyhow, Context, Error, Result};
use futures::FutureExt;
use helios_consensus_core::consensus_spec::ConsensusSpec;
use log::{debug, error, warn};
use nori_sp1_helios_primitives::types::{
    ConsensusProofInputs, ContractStorage, ProofInputs, ProofInputsWithWindow, StorageSlot,
};
use nori_sp1_helios_program::consensus::consensus_mpt_program;
use reqwest::{Client, Url};
use std::{env, marker::PhantomData};
use tokio::time::{sleep, Duration};

const CHUNK_SIZE: u64 = 100;
const MAX_RETRIES: usize = 3;
const RETRY_BASE_DELAY: Duration = Duration::from_secs(1);
const EXECUTION_PROVIDER_TIMEOUT: Duration = Duration::from_secs(20);

pub struct ExecutionHttpProxy<S: ConsensusSpec> {
    principal_provider: RootProvider<Ethereum>,
    backup_providers: Vec<RootProvider<Ethereum>>,
    source_state_bridge_contract_address: Address,
    _marker: PhantomData<S>,
    validation_timeout: Duration,
}

impl<S: ConsensusSpec> ExecutionHttpProxy<S> {
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok();

        // Parsing the proof validation timeout
        let validation_timeout_sec = std::env::var("NORI_EXECUTION_PROOF_INPUT_VALIDATION_TIMEOUT")
            .ok()
            .map(|v| {
                v.parse::<u64>().map_err(|e| {
                    Error::msg(format!(
                        "Failed to parse NORI_EXECUTION_PROOF_INPUT_VALIDATION_TIMEOUT as u64: {}",
                        e
                    ))
                })
            })
            .transpose()?
            .unwrap_or(300);

        let validation_timeout = Duration::from_secs(validation_timeout_sec);

        let source_execution_http_urls = env::var("NORI_SOURCE_EXECUTION_HTTP_RPCS")
            .context("Missing NORI_SOURCE_EXECUTION_HTTP_RPCS in environment")?;

        let mut providers: Vec<RootProvider<Ethereum>> = source_execution_http_urls
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .filter_map(|url_str| match url_str.parse::<Url>() {
                Ok(url) => Some(url),
                Err(err) => {
                    warn!("Skipping invalid URL '{}': {}", url_str, err);
                    None
                }
            })
            .map(|rpc_url| {
                ProviderBuilder::new()
                    .network::<Ethereum>()
                    .connect_http(rpc_url)
                    .root()
                    .clone()
            })
            .collect();

        if providers.is_empty() {
            return Err(anyhow!(
                "No valid execution RPC URLs found in NORI_SOURCE_EXECUTION_HTTP_RPCS."
            ));
        }

        let principal_provider = providers.remove(0);

        let source_state_bridge_contract_address = get_source_contract_address()?;

        Ok(ExecutionHttpProxy {
            source_state_bridge_contract_address,
            principal_provider,
            backup_providers: providers,
            _marker: PhantomData,
            validation_timeout,
        })
    }

    pub fn try_from_env() -> Self {
        ExecutionHttpProxy::from_env().unwrap()
    }

    async fn _get_source_contract_event_chunk<T>(
        provider: &RootProvider<Ethereum>,
        source_state_bridge_contract_address: &Address,
        start: u64,
        end: u64,
    ) -> Result<Vec<Log<T>>>
    where
        T: SolEvent + 'static,
    {
        let event_signature = T::SIGNATURE;

        let filter = Filter::new()
            .address(*source_state_bridge_contract_address)
            .event(event_signature)
            .from_block(start)
            .to_block(end);

        let logs = provider.get_logs(&filter).await?;

        let events: Vec<Log<T>> = logs
            .into_iter()
            .filter_map(|log| T::decode_log(&log.inner).ok())
            .collect();

        Ok(events)
    }

    async fn _get_source_contract_events<T>(
        provider: &RootProvider<Ethereum>,
        source_state_bridge_contract_address: &Address,
        start_block: u64,
        end_block: u64,
    ) -> Result<Vec<Log<T>>>
    where
        T: SolEvent + 'static,
    {
        let mut all_events = Vec::new();
        let mut current_block = start_block;

        while current_block <= end_block {
            let chunk_end = (current_block + CHUNK_SIZE).min(end_block);

            debug!(
                "Loading source contract event '{}' from blocks '{}'->'{}'.",
                T::SIGNATURE,
                current_block,
                chunk_end
            );

            let mut retries = 0;
            let events = loop {
                match Self::_get_source_contract_event_chunk(
                    provider,
                    source_state_bridge_contract_address,
                    current_block,
                    chunk_end,
                )
                .await
                {
                    Ok(events) => break events,
                    Err(e) if retries < MAX_RETRIES => {
                        let delay = RETRY_BASE_DELAY * 2u32.pow(retries as u32);
                        error!(
                            "Error fetching chunk (retry {} in {:?}): {:?}",
                            retries + 1,
                            delay,
                            e
                        );
                        sleep(delay).await;
                        retries += 1;
                    }
                    Err(e) => return Err(e),
                }
            };

            all_events.extend(events);
            current_block = chunk_end + 1; // Move to next chunk immediately
        }

        Ok(all_events)
    }

    async fn _get_proof(
        provider: &RootProvider<Ethereum>,
        source_state_bridge_contract_address: &Address,
        storage_keys: Vec<B256>,
        block_id: BlockId,
    ) -> Result<EIP1186AccountProofResponse> {
        let proof = provider
            .get_proof(*source_state_bridge_contract_address, storage_keys)
            .block_id(block_id)
            .await;

        match proof {
            Ok(proof) => Ok(proof),
            Err(e) => Err(anyhow!("ExecutionHttp RPC error: {e}")),
        }
    }

    // TODO Doc string
    async fn _prepare_consensus_mpt_proof_inputs(
        provider: &RootProvider<Ethereum>,
        source_state_bridge_contract_address: &Address,
        input_block_number: u64,
        output_block_number: u64,
        validated_consensus_proof_inputs: ConsensusProofInputs<S>,
    ) -> Result<ProofInputs<S>> {
        let contract_events = Self::_get_source_contract_events::<NoriStateBridge::TokensLocked>(
            provider,
            source_state_bridge_contract_address,
            input_block_number,
            output_block_number,
        )
        .await?;

        let storage_slot_address_map = addresses_attestation_pair_to_storage_slots(contract_events);

        for (storage_slot, address) in storage_slot_address_map.iter() {
            debug!(
                "Storage slots obtained address '{:?}' storage_slot '{:?}'",
                address, storage_slot
            );
        }

        // Get mpt proof
        let mpt_account_proof = Self::_get_proof(
            provider,
            source_state_bridge_contract_address, //get_source_contract_address()?,
            storage_slot_address_map.keys().cloned().collect(),
            BlockId::number(output_block_number),
        )
        .await?;

        debug!(
            "mpt_account_proof {:?}",
            serde_json::to_string(&mpt_account_proof)
        );

        let mut storage_slots: Vec<StorageSlot> = mpt_account_proof
            .storage_proof
            .iter()
            .map(|slot| {
                let address_attestation_pair = storage_slot_address_map
                    .get(&slot.key.as_b256())
                    .copied()
                    .expect("Missing address attestation pair for storage slot");
                StorageSlot {
                    slot_key_address: address_attestation_pair.0,
                    slot_nested_key_attestation_hash: address_attestation_pair.1,
                    key: slot.key.as_b256(),
                    expected_value: slot.value,
                    mpt_proof: slot.proof.clone(),
                }
            })
            .collect();

        // Sort by address order for stability (in case rpc returns strange order)
        //storage_slots.sort_by_key(|s| s.slot_key_address);
        storage_slots.sort_by(|a, b| {
            a.slot_key_address.cmp(&b.slot_key_address).then_with(|| {
                a.slot_nested_key_attestation_hash
                    .cmp(&b.slot_nested_key_attestation_hash)
            })
        });

        let contract_storage = ContractStorage {
            address: mpt_account_proof.address,
            expected_value: alloy_trie::TrieAccount {
                nonce: mpt_account_proof.nonce,
                balance: mpt_account_proof.balance,
                storage_root: mpt_account_proof.storage_hash,
                code_hash: mpt_account_proof.code_hash,
            },
            mpt_proof: mpt_account_proof.account_proof,
            storage_slots,
        };

        let consensus_mpt_proof_input: ProofInputs<S> = ProofInputs::<S> {
            updates: validated_consensus_proof_inputs.updates,
            finality_update: validated_consensus_proof_inputs.finality_update,
            expected_current_slot: validated_consensus_proof_inputs.expected_current_slot,
            store: validated_consensus_proof_inputs.store,
            genesis_root: validated_consensus_proof_inputs.genesis_root,
            forks: validated_consensus_proof_inputs.forks,
            store_hash: validated_consensus_proof_inputs.store_hash,
            contract_storage,
        };

        let consensus_mpt_proof_input_clone = consensus_mpt_proof_input.clone();

        let enable_debug = match std::env::var("RUST_LOG") {
            Ok(val) => val.to_lowercase().contains("debug"),
            Err(_) => false,
        };

        // Dry run this proof
        let _ = tokio::task::spawn_blocking(move || {
            // Run program logic
            consensus_mpt_program(consensus_mpt_proof_input_clone, enable_debug)
            // enable_debug
        })
        .await??;

        Ok(consensus_mpt_proof_input)
    }

    // TODO Doc string
    pub async fn prepare_consensus_mpt_proof_inputs(
        &self,
        input_slot: u64,
        output_slot: u64,
        input_block_number: u64,
        output_block_number: u64,
        validated_consensus_proof_inputs: ConsensusProofInputs<S>,
        expected_output_store_hash: FixedBytes<32>,
    ) -> Result<ProofInputsWithWindow<S>> {
        let source_state_bridge_contract_address = self.source_state_bridge_contract_address;
        let output = query_with_fallback(
            &self.principal_provider,
            &self.backup_providers,
            |provider| {
                let validated_consensus_proof_inputs = validated_consensus_proof_inputs.clone();
                // use provider as the client here
                async move {
                    Self::_prepare_consensus_mpt_proof_inputs(
                        &provider,
                        &source_state_bridge_contract_address,
                        input_block_number,
                        output_block_number,
                        validated_consensus_proof_inputs,
                    )
                    .await
                }
                .boxed()
            },
            self.validation_timeout,
        )
        .await?;

        let output_with_blocks = ProofInputsWithWindow::<S> {
            input_slot,
            expected_output_slot: output_slot,
            input_block_number,
            expected_output_block_number: output_block_number,
            proof_inputs: output,
            expected_output_store_hash,
        };

        Ok(output_with_blocks)
    }

    pub async fn get_source_contract_events<T>(
        &self,
        start_block: u64,
        end_block: u64,
    ) -> Result<Vec<Log<T>>>
    where
        T: SolEvent + Send + 'static,
    {
        let source_state_bridge_contract_address = self.source_state_bridge_contract_address;
        query_with_fallback(
            &self.principal_provider,
            &self.backup_providers,
            |provider| {
                async move {
                    Self::_get_source_contract_events(
                        &provider,
                        &source_state_bridge_contract_address,
                        start_block,
                        end_block,
                    )
                    .await
                }
                .boxed()
            },
            EXECUTION_PROVIDER_TIMEOUT,
        )
        .await
    }
}
