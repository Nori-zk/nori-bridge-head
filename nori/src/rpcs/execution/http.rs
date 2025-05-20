use alloy::{
    eips::BlockId,
    providers::{Provider, ProviderBuilder, RootProvider},
    rpc::types::{EIP1186AccountProofResponse, Filter},
    sol_types::SolEvent,
    transports::http::Http,
};
use alloy_primitives::{Address, Log, B256};
use anyhow::{anyhow, Context, Result};
use futures::{
    future::{select_ok, BoxFuture},
    FutureExt,
};
use log::{error, info, warn};
use reqwest::{Client, Url};
use std::env;
use tokio::time::{sleep, timeout, Duration};

use crate::contract::bindings::get_source_contract_address;

const CHUNK_SIZE: u64 = 100;
const MAX_RETRIES: usize = 3;
const RETRY_BASE_DELAY: Duration = Duration::from_secs(1);
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ExecutionHttpProxy {
    principal_provider: RootProvider<Http<Client>>,
    backup_providers: Vec<RootProvider<Http<Client>>>,
    source_state_bridge_contract_address: Address,
}

pub async fn query_with_fallback<F, C, R>(
    principal_provider: &C,
    backup_providers: &[C],
    f: F,
) -> Result<R>
where
    F: Fn(C) -> BoxFuture<'static, Result<R>>,
    C: Clone,
    R: 'static,
{
    match timeout(PROVIDER_TIMEOUT, f(principal_provider.clone())).await {
        Ok(Ok(result)) => return Ok(result),
        Ok(Err(err)) => {
            warn!("Principal provider failed: {}.", err);
        }
        Err(_) => {
            warn!("Principal provider timed out after {:?}", PROVIDER_TIMEOUT);
        }
    }

    if backup_providers.is_empty() {
        return Err(anyhow!(
            "Principal provider failed and no backups available."
        ));
    }

    warn!("Trying backup providers...");

    let backup_futures: Vec<BoxFuture<'static, Result<R>>> = backup_providers
        .iter()
        .map(|provider| {
            let fut = f(provider.clone());
            Box::pin(async move {
                timeout(PROVIDER_TIMEOUT, fut)
                    .await
                    .map_err(|_| anyhow!("Provider timed out after {:?}", PROVIDER_TIMEOUT))?
            }) as BoxFuture<'static, Result<R>>
        })
        .collect();

    match select_ok(backup_futures).await {
        Ok((result, _)) => Ok(result),
        Err(e) => Err(anyhow!("All backups failed: {}", e)),
    }
}

impl ExecutionHttpProxy {
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok();

        let source_execution_http_urls = env::var("SOURCE_EXECUTION_HTTP_RPCS")
            .context("Missing SOURCE_EXECUTION_HTTP_RPCS in environment")?;

        let mut providers: Vec<RootProvider<Http<Client>>> = source_execution_http_urls
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
            .map(|rpc_url| ProviderBuilder::new().on_http(rpc_url))
            .collect();

        if providers.is_empty() {
            return Err(anyhow!(
                "No valid execution RPC URLs found in SOURCE_EXECUTION_HTTP_RPCS."
            ));
        }

        let principal_provider = providers.remove(0);

        let source_state_bridge_contract_address = get_source_contract_address()?;
            /*env::var("NORI_SOURCE_STATE_BRIDGE_CONTACT_ADDRESS")
                .context("Missing NORI_SOURCE_STATE_BRIDGE_CONTACT_ADDRESS in environment")?
                .parse::<Address>()
                .context("Invalid Ethereum address format")?;*/

        Ok(ExecutionHttpProxy {
            source_state_bridge_contract_address,
            principal_provider,
            backup_providers: providers,
        })
    }

    pub fn try_from_env() -> Self {
        ExecutionHttpProxy::from_env().unwrap()
    }

    async fn _get_source_contract_event_chunk<T>(
        provider: RootProvider<Http<Client>>,
        source_state_bridge_contract_address: Address,
        start: u64,
        end: u64,
    ) -> Result<Vec<Log<T>>>
    where
        T: SolEvent + 'static,
    {
        let event_signature = T::SIGNATURE;

        let filter = Filter::new()
            .address(source_state_bridge_contract_address)
            .event(event_signature)
            .from_block(start)
            .to_block(end);

        let logs = provider.get_logs(&filter).await?;

        let events: Vec<Log<T>> = logs
            .into_iter()
            .filter_map(|log| T::decode_log(&log.inner, true).ok())
            .collect();

        Ok(events)
    }

    pub async fn get_source_contract_events<T>(
        &self,
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

            info!(
                "Loading source contract event '{}' from blocks '{}'->'{}'.",
                T::SIGNATURE,
                current_block,
                chunk_end
            );

            let mut retries = 0;
            let events = loop {
                match query_with_fallback(
                    &self.principal_provider,
                    &self.backup_providers,
                    |provider| {
                        let contract_address = self.source_state_bridge_contract_address;
                        async move {
                            Self::_get_source_contract_event_chunk(
                                provider,
                                contract_address,
                                current_block,
                                chunk_end,
                            )
                            .await
                        }
                        .boxed()
                    },
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
        client: RootProvider<Http<Client>>,
        source_state_bridge_contract_address: Address,
        storage_keys: Vec<B256>,
        block_id: BlockId,
    ) -> Result<EIP1186AccountProofResponse> {
        let proof = client
            .get_proof(source_state_bridge_contract_address, storage_keys)
            .block_id(block_id)
            .await;

        match proof {
            Ok(proof) => Ok(proof),
            Err(e) => Err(anyhow!("ExecutionHttp RPC error: {e}")),
        }
    }

    pub async fn get_proof(
        &self,
        address: Address,
        keys: Vec<B256>,
        block_id: BlockId,
    ) -> Result<EIP1186AccountProofResponse> {
        let proof = query_with_fallback(
            &self.principal_provider,
            &self.backup_providers,
            |provider| {
                let keys = keys.clone();
                // use provider as the client here
                async move { Self::_get_proof(provider, address, keys, block_id).await }
                    .boxed()
            },
        )
        .await?;

        Ok(proof)
    }

    
}
