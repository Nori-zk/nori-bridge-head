use std::{env, ops::Add};

use crate::contract::bindings::NoriStateBridge::{self, TokensLocked};
use alloy::{
    providers::{Provider, ProviderBuilder, RootProvider},
    rpc::types::Filter,
    sol_types::SolEvent,
    transports::http::Http,
};
use alloy_primitives::{Address, Log};
use anyhow::{anyhow, Context, Result};
use futures::{
    future::{select_ok, BoxFuture},
    FutureExt,
};
use log::{error, info, warn};
use reqwest::{Client, Url};
use tokio::time::{sleep, Duration};

const CHUNK_SIZE: u64 = 100;
const MAX_RETRIES: usize = 3;
const RETRY_BASE_DELAY: Duration = Duration::from_secs(1);
pub struct ExecutionHttpProxy {
    principle_provider: RootProvider<Http<Client>>,
    backup_providers: Vec<RootProvider<Http<Client>>>,
    source_state_bridge_contract_address: Address,
}

pub async fn query_with_fallback<F, C, R>(
    principle_provider: &C,
    backup_providers: &[C],
    f: F,
) -> Result<R>
where
    F: Fn(C) -> BoxFuture<'static, Result<R>>,
    C: Clone,
    R: 'static,
{
    match f(principle_provider.clone()).await {
        Ok(result) => Ok(result),
        Err(err) => {
            warn!("Principal provider failed: {}.", err);
            if backup_providers.is_empty() {
                return Err(err);
            }
            warn!("Trying backup providers...");
            let backup_futures = backup_providers.iter().map(|provider| f(provider.clone()));

            match select_ok(backup_futures).await {
                Ok((result, _)) => Ok(result),
                Err(e) => Err(anyhow!("All backups failed: {}", e)),
            }
        }
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

        let principle_provider = providers.remove(0);

        let source_state_bridge_contract_address =
            env::var("NORI_SOURCE_STATE_BRIDGE_CONTACT_ADDRESS")
                .context("Missing NORI_SOURCE_STATE_BRIDGE_CONTACT_ADDRESS in environment")?
                .parse::<Address>()
                .context("Invalid Ethereum address format")?;

        Ok(ExecutionHttpProxy {
            source_state_bridge_contract_address,
            principle_provider,
            backup_providers: providers,
        })
    }

    pub fn try_from_env() -> Self {
        ExecutionHttpProxy::from_env().unwrap()
    }

    async fn get_source_contract_event_chunk<T>(
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
                    &self.principle_provider,
                    &self.backup_providers,
                    |provider| {
                        let contract_address = self.source_state_bridge_contract_address;
                        async move {
                            Self::get_source_contract_event_chunk(
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
}