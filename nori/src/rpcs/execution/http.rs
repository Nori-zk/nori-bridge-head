use crate::{
    contract::{get_proof_queue_address, get_source_contract_address},
    rpcs::query_with_fallback,
};
use alloy::{
    eips::BlockId,
    network::Ethereum,
    providers::{Provider, ProviderBuilder, RootProvider},
    rpc::types::{EIP1186AccountProofResponse, Filter},
    sol_types::SolEvent,
};
use alloy_primitives::{keccak256, Address, Bytes, FixedBytes, Log, B256, U256};
use alloy_rlp::Encodable;
use alloy_trie::{Nibbles, TrieAccount};
use anyhow::{anyhow, Context, Error, Result};
use futures::FutureExt;
use helios_consensus_core::consensus_spec::ConsensusSpec;
use log::{debug, error, warn};
use nori_sp1_helios_primitives::types::{
    mapping_entry_location, storage_slot_of_index, struct_word_slot, ConsensusProofInputs,
    ProofInputs, ProofInputsWithWindow, QueueEntryProof, QueueStorage, TargetSlotProof,
    TargetStorageProof, MAX_BATCH, QUEUE_ENTRY_WORDS, QUEUE_HEAD_STORAGE_INDEX,
    QUEUE_REQUESTS_STORAGE_INDEX,
};
use nori_sp1_helios_program::consensus::consensus_mpt_program;
use reqwest::Url;
use std::{collections::HashMap, env, marker::PhantomData};
use tokio::time::{sleep, Duration};

const CHUNK_SIZE: u64 = 100;
const MAX_RETRIES: usize = 3;
const RETRY_BASE_DELAY: Duration = Duration::from_secs(1);
const EXECUTION_PROVIDER_TIMEOUT: Duration = Duration::from_secs(20);

pub struct ExecutionHttpProxy<S: ConsensusSpec> {
    principal_provider: RootProvider<Ethereum>,
    backup_providers: Vec<RootProvider<Ethereum>>,
    #[deprecated(
        note = "Superseded by the proof request queue; only read by the deprecated source-contract event path. The prover anchors on proof_queue_address."
    )]
    source_state_bridge_contract_address: Address,
    proof_queue_address: Address,
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

        // FIXME(request-queue): only the superseded event path reads this field.
        // Remove this line and the field once that path is deleted.
        let source_state_bridge_contract_address = get_source_contract_address()?;
        let proof_queue_address = get_proof_queue_address()?;

        Ok(ExecutionHttpProxy {
            source_state_bridge_contract_address,
            proof_queue_address,
            principal_provider,
            backup_providers: providers,
            _marker: PhantomData,
            validation_timeout,
        })
    }

    pub fn try_from_env() -> Self {
        ExecutionHttpProxy::from_env().unwrap()
    }

    #[deprecated(
        note = "Superseded by the proof request queue; the host reads queue storage directly instead of scanning TokensLocked events. No longer used in the proving path."
    )]
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

    #[deprecated(
        note = "Superseded by the proof request queue, which reads queue storage directly instead of scanning TokensLocked events. No longer used in the proving path."
    )]
    #[allow(deprecated)]
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

    // This is the bulk eth_getProof helper: it forwards storage_keys.len() keys
    // in one request with no chunking or cap. Two distinct RPC concerns apply.
    //
    // FIXME(request-queue): request batching. The queue caller can pass up to
    // 1 + MAX_BATCH * QUEUE_ENTRY_WORDS = 327,681 keys, roughly a 20 MB JSON
    // body, far over what vendors accept.
    //   Request size, what vendors allow:
    //     Chainstack: request body capped at 1 MB.
    //       https://docs.chainstack.com/docs/limits
    //     Infura and general JSON-RPC: batch payload near 1 MB.
    //       https://docs.infura.io/api/networks/ethereum/json-rpc-methods
    //     Alchemy: 1000 requests per JSON-RPC batch over HTTP.
    //       https://www.alchemy.com/docs/reference/batch-requests
    //   Request size, recommendation:
    //     Ethereum execution-apis issue 752 (eth_getStorageValues) proposes a
    //     default cap of 1024 storage slots per bulk request.
    //       https://github.com/ethereum/execution-apis/issues/752
    //   Rate limits, what vendors allow:
    //     Chainstack: plan RPS tier is 25, 100, 250, 500, or 1000 RPS (Enterprise
    //       unlimited); 1000 requests per HTTP connection; 500 open HTTP
    //       connections. https://docs.chainstack.com/docs/limits
    //     Infura: returns HTTP 429 when rate limited.
    //       https://docs.infura.io/api/networks/ethereum/json-rpc-methods
    //   Action: chunk storage_keys, then pace the chunks under the RPS tier.
    //
    // FIXME(request-queue): historical state. Proving an old block needs an
    //   archive node, and eth_getProof history is itself capped by some backends.
    //     Chainstack: on Erigon eth_getProof reaches 100,000 blocks back,
    //       unbounded on Geth.
    //       https://docs.chainstack.com/docs/deep-dive-into-merkle-proofs-and-eth-getproof-ethereum-rpc-method
    async fn _get_proof(
        provider: &RootProvider<Ethereum>,
        address: &Address,
        storage_keys: Vec<B256>,
        block_id: BlockId,
    ) -> Result<EIP1186AccountProofResponse> {
        let proof = provider
            .get_proof(*address, storage_keys)
            .block_id(block_id)
            .await;

        match proof {
            Ok(proof) => Ok(proof),
            Err(e) => Err(anyhow!("ExecutionHttp RPC error: {e}")),
        }
    }

    /// Reads a single storage word at `block`.
    async fn _get_storage_word(
        provider: &RootProvider<Ethereum>,
        address: &Address,
        key: B256,
        block: BlockId,
    ) -> Result<U256> {
        provider
            .get_storage_at(*address, key.into())
            .block_id(block)
            .await
            .map_err(|e| anyhow!("ExecutionHttp RPC error reading storage: {e}"))
    }

    /// Reads the proof request queue `head` at `block` on the given provider, so
    /// a caller assembling one preparation reads it from the same provider as the
    /// rest of its reads.
    async fn _get_queue_head(
        provider: &RootProvider<Ethereum>,
        proof_queue_address: &Address,
        block: BlockId,
    ) -> Result<u64> {
        let head_key = storage_slot_of_index(QUEUE_HEAD_STORAGE_INDEX);
        Self::_get_storage_word(provider, proof_queue_address, head_key, block)
            .await?
            .try_into()
            .map_err(|_| anyhow!("Queue head exceeds u64"))
    }

    /// Reads the proof request queue `head` at `block`, with provider fallback,
    /// for standalone callers that have no provider of their own.
    pub async fn get_queue_head(&self, block: BlockId) -> Result<u64> {
        let proof_queue_address = self.proof_queue_address;
        query_with_fallback(
            &self.principal_provider,
            &self.backup_providers,
            |provider| {
                async move { Self::_get_queue_head(&provider, &proof_queue_address, block).await }
                    .boxed()
            },
            EXECUTION_PROVIDER_TIMEOUT,
        )
        .await
    }

    /// Decides whether an account exists by verifying its proof both ways.
    ///
    /// Clients disagree on what an absent account looks like in an
    /// `eth_getProof` response, so the response is not trusted: the inclusion
    /// proof is checked first, then the exclusion proof. Whichever verifies is
    /// what the guest will be given.
    fn _account_witness(
        proof: &EIP1186AccountProofResponse,
        execution_state_root: B256,
    ) -> Result<Option<TrieAccount>> {
        let account = TrieAccount {
            nonce: proof.nonce,
            balance: proof.balance,
            storage_root: proof.storage_hash,
            code_hash: proof.code_hash,
        };
        let address_nibbles = Nibbles::unpack(keccak256(proof.address.as_slice()));

        let mut rlp_encoded_account = Vec::new();
        account.encode(&mut rlp_encoded_account);

        if alloy_trie::proof::verify_proof(
            execution_state_root,
            address_nibbles,
            Some(rlp_encoded_account),
            &proof.account_proof,
        )
        .is_ok()
        {
            return Ok(Some(account));
        }

        alloy_trie::proof::verify_proof(
            execution_state_root,
            address_nibbles,
            None,
            &proof.account_proof,
        )
        .map_err(|e| {
            anyhow!(
                "Account proof for {:?} verifies neither as present nor absent: {e}",
                proof.address
            )
        })?;

        Ok(None)
    }

    /// Builds the queue witness for entries `queue_cursor` up to `head`, exclusive of `head`.
    ///
    /// The key set is not chosen here: `head` is read from queue storage,
    /// entry locations are derived from their index, and each entry names its
    /// own target and slot. The host only fetches what the queue already
    /// committed to, so a bug here yields an unprovable input rather than a
    /// silently pruned tree.
    async fn _prepare_consensus_mpt_proof_inputs(
        provider: &RootProvider<Ethereum>,
        proof_queue_address: &Address,
        input_queue_cursor: u64,
        output_block_number: u64,
        validated_consensus_proof_inputs: ConsensusProofInputs<S>,
    ) -> Result<ProofInputs<S>> {
        let block = BlockId::number(output_block_number);
        let execution_state_root = validated_consensus_proof_inputs
            .store
            .finalized_header
            .execution()
            .map_err(|e| anyhow!("Finalized header has no execution payload: {e:?}"))?
            .state_root()
            .to_owned();

        // 1. Queue head, and the batch it derives with the cursor.
        let queue_head_key = storage_slot_of_index(QUEUE_HEAD_STORAGE_INDEX);
        let queue_head = Self::_get_queue_head(provider, proof_queue_address, block).await?;

        if input_queue_cursor > queue_head {
            return Err(anyhow!(
                "Request cursor {input_queue_cursor} is ahead of queue head {queue_head}"
            ));
        }
        let batch = std::cmp::min(queue_head - input_queue_cursor, MAX_BATCH as u64);
        debug!("Queue head {queue_head}, cursor {input_queue_cursor}, batch {batch}");

        // 2. Entry fields, read from their index-derived locations.
        let mut entry_word_keys: Vec<B256> = Vec::with_capacity(batch as usize * QUEUE_ENTRY_WORDS);
        for offset in 0..batch {
            let base = mapping_entry_location(
                U256::from(input_queue_cursor + offset),
                QUEUE_REQUESTS_STORAGE_INDEX,
            );
            entry_word_keys.push(base);
            for word_index in 1..QUEUE_ENTRY_WORDS as u8 {
                entry_word_keys.push(struct_word_slot(base, word_index));
            }
        }

        // 3. One proof request covering head and every entry word. eth_getProof
        //    returns each key's value alongside its proof, so the entry fields
        //    are read from this response rather than fetched separately. With an
        //    empty batch this is a single key: the proof that head == cursor,
        //    which the circuit still requires before it will commit an empty
        //    root.
        let mut queue_keys = vec![queue_head_key];
        queue_keys.extend(entry_word_keys.iter().copied());
        let queue_proof =
            Self::_get_proof(provider, proof_queue_address, queue_keys, block).await?;

        let queue_words: HashMap<B256, (U256, Vec<Bytes>)> = queue_proof
            .storage_proof
            .iter()
            .map(|slot| (slot.key.as_b256(), (slot.value, slot.proof.clone())))
            .collect();
        let word_at = |key: &B256| -> Result<(U256, Vec<Bytes>)> {
            queue_words
                .get(key)
                .cloned()
                .ok_or_else(|| anyhow!("Queue proof missing storage key {key:?}"))
        };

        // 4. Assemble entries, and collect the slots each target must prove.
        let mut entries: Vec<QueueEntryProof> = Vec::with_capacity(batch as usize);
        let mut slots_by_target: HashMap<Address, Vec<B256>> = HashMap::new();
        for offset in 0..batch as usize {
            let keys =
                &entry_word_keys[offset * QUEUE_ENTRY_WORDS..(offset + 1) * QUEUE_ENTRY_WORDS];
            let words = keys.iter().map(word_at).collect::<Result<Vec<_>>>()?;

            let target = Address::from_slice(&words[0].0.to_be_bytes::<32>()[12..32]);
            let slot_key = B256::from(words[1].0.to_be_bytes::<32>());

            entries.push(QueueEntryProof {
                target,
                slot_key,
                collection_keys_count: words[2].0.to::<u8>(),
                collection_keys: [
                    B256::from(words[3].0.to_be_bytes::<32>()),
                    B256::from(words[4].0.to_be_bytes::<32>()),
                ],
                word_proofs: words
                    .into_iter()
                    .map(|(_, proof)| proof)
                    .collect::<Vec<_>>()
                    .try_into()
                    .map_err(|_| anyhow!("Expected {QUEUE_ENTRY_WORDS} word proofs"))?,
            });

            let target_slots = slots_by_target.entry(target).or_default();
            if !target_slots.contains(&slot_key) {
                target_slots.push(slot_key);
            }
        }

        // 5. One proof request per distinct target. None are issued for an
        //    empty batch.
        let mut targets: Vec<TargetStorageProof> = Vec::with_capacity(slots_by_target.len());
        for (address, slot_keys) in slots_by_target {
            let target_proof =
                Self::_get_proof(provider, &address, slot_keys.clone(), block).await?;
            let account = Self::_account_witness(&target_proof, execution_state_root)?;

            let proved: HashMap<B256, (U256, Vec<Bytes>)> = target_proof
                .storage_proof
                .iter()
                .map(|slot| (slot.key.as_b256(), (slot.value, slot.proof.clone())))
                .collect();

            let slots = slot_keys
                .iter()
                .map(|key| {
                    let (value, mpt_proof) = proved
                        .get(key)
                        .cloned()
                        .ok_or_else(|| anyhow!("Target proof missing storage key {key:?}"))?;
                    Ok(TargetSlotProof {
                        key: *key,
                        value,
                        mpt_proof,
                    })
                })
                .collect::<Result<Vec<_>>>()?;

            targets.push(TargetStorageProof {
                target_address: address,
                account,
                account_mpt_proof: target_proof.account_proof,
                slots,
            });
        }

        // The queue always exists; a wrong claim fails in-circuit.
        let queue_storage = QueueStorage {
            proof_request_queue_address: queue_proof.address,
            proof_request_queue_account: TrieAccount {
                nonce: queue_proof.nonce,
                balance: queue_proof.balance,
                storage_root: queue_proof.storage_hash,
                code_hash: queue_proof.code_hash,
            },
            proof_request_queue_account_mpt_proof: queue_proof.account_proof,
            head: queue_head,
            head_mpt_proof: word_at(&queue_head_key)?.1,
            input_cursor: input_queue_cursor,
            entries,
            targets,
        };

        let consensus_mpt_proof_input: ProofInputs<S> = ProofInputs::<S> {
            updates: validated_consensus_proof_inputs.updates,
            finality_update: validated_consensus_proof_inputs.finality_update,
            expected_current_slot: validated_consensus_proof_inputs.expected_current_slot,
            store: validated_consensus_proof_inputs.store,
            genesis_root: validated_consensus_proof_inputs.genesis_root,
            forks: validated_consensus_proof_inputs.forks,
            store_hash: validated_consensus_proof_inputs.store_hash,
            queue_storage,
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
        input_queue_cursor: u64,
        validated_consensus_proof_inputs: ConsensusProofInputs<S>,
        expected_output_store_hash: FixedBytes<32>,
    ) -> Result<ProofInputsWithWindow<S>> {
        let proof_queue_address = self.proof_queue_address;
        let output = query_with_fallback(
            &self.principal_provider,
            &self.backup_providers,
            |provider| {
                let validated_consensus_proof_inputs = validated_consensus_proof_inputs.clone();
                // use provider as the client here
                async move {
                    Self::_prepare_consensus_mpt_proof_inputs(
                        &provider,
                        &proof_queue_address,
                        input_queue_cursor,
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

        // Output cursor the proof will commit: the input cursor plus the entries
        // this batch drains. entries.len() is the MAX_BATCH-capped batch, so this
        // advances by at most MAX_BATCH.
        let expected_output_queue_cursor =
            input_queue_cursor + output.queue_storage.entries.len() as u64;
        let output_with_blocks = ProofInputsWithWindow::<S> {
            input_slot,
            expected_output_slot: output_slot,
            input_block_number,
            expected_output_block_number: output_block_number,
            proof_inputs: output,
            expected_output_store_hash,
            expected_output_queue_cursor,
        };

        Ok(output_with_blocks)
    }

    #[deprecated(
        note = "Superseded by the proof request queue, which reads queue storage directly instead of scanning TokensLocked events. No longer used in the proving path."
    )]
    #[allow(deprecated)]
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
