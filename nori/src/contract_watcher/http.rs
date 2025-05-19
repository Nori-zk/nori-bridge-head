use super::bindings::NoriTokenBridge::{self, TokensLocked};
use alloy_primitives::{Address, Log};
use anyhow::Result;
use tokio::time::{sleep, Duration};
use reqwest::Client;
use alloy::{
    providers::{Provider, ProviderBuilder, RootProvider}, rpc::types::Filter, sol_types::SolEvent,
    transports::http::Http
};

const CHUNK_SIZE: u64 = 100;
const MAX_RETRIES: usize = 3;
const RETRY_BASE_DELAY: Duration = Duration::from_secs(1);

async fn try_get_chunk(
    provider: &RootProvider<Http<Client>>,
    address: &Address,
    start: u64,
    end: u64,
) -> Result<Vec<Log<TokensLocked>>> {

    let event_signature = NoriTokenBridge::TokensLocked::SIGNATURE;

    let filter = Filter::new()
        .address(*address)
        .event(event_signature)
        .from_block(start)
        .to_block(end);

    let logs = provider.get_logs(&filter).await?;
    
    let events =
        logs.into_iter()
        .filter_map(|log| {
            NoriTokenBridge::TokensLocked::decode_log(&log.inner, true).ok()            
        })
        .collect();

    Ok(events)
}

pub async fn get_source_contract_events_between_blocks(
    eth_http_rpc: &str,
    token_bridge_address: &Address,
    start_block: u64,
    end_block: u64,
) -> Result<Vec<Log<TokensLocked>>> {
    // Vec<TokensLockedFilter>

    let rpc_url = eth_http_rpc.parse()?;
    let provider = ProviderBuilder::new().on_http(rpc_url);

    let mut all_events = Vec::new();
    let mut current_block = start_block;

    while current_block <= end_block {
        let chunk_end = (current_block + CHUNK_SIZE).min(end_block);

        println!("Processing blocks {}-{}", current_block, chunk_end);

        let mut retries = 0;
        let events = loop {
            match try_get_chunk(
                &provider,
                token_bridge_address,
                current_block,
                chunk_end
            ).await {
                Ok(events) => break events,
                Err(e) if retries < MAX_RETRIES => {
                    let delay = RETRY_BASE_DELAY * 2u32.pow(retries as u32);
                    eprintln!("Error fetching chunk (retry {} in {:?}): {:?}",
                        retries + 1, delay, e);
                    sleep(delay).await;
                    retries += 1;
                }
                Err(e) => return Err(e),
            }
        };

        all_events.extend(events);
        current_block = chunk_end + 1;  // Move to next chunk immediately
    }

    Ok(all_events)
}

/*
    BE AWARE OF API LIMITS
    Developer plan — 100 blocks
    Growth plan — 10,000 blocks
    Pro plan — 10,000 blocks
    Business plan — 10,000 blocks
    Enterprise — 10,000 blocks.
*/

/*

pub async fn get_source_contract_events_between_blocks(
    eth_http_rpc: &String,
    token_bridge_address: &Address,
    start_block: u64,
    end_block: u64,
) -> Result<()> {
    let provider = Provider::<Http>::try_from(eth_http_rpc)?;
    let provider = Arc::new(provider);
    let tokens_locked_event_abi_signature = <TokensLockedFilter as EthEvent>::abi_signature();

    println!(
        "Querying TokensLocked events between start block '{}' and end block '{}' blocks for contract {:?}",
        start_block, end_block, token_bridge_address
    );

    let filter = Filter::new()
        .address(*token_bridge_address)
        .event(&tokens_locked_event_abi_signature)
        .from_block(BlockNumber::Number(start_block.into()))
        .to_block(BlockNumber::Number(end_block.into()));

    let logs = provider.get_logs(&filter).await?;

    println!("Got events {}", logs.len());

    let events: Vec<TokensLockedFilter> = logs
        .into_iter()
        .filter_map(|log| {
            let raw_log = RawLog {
                topics: log.topics,
                data: log.data.to_vec(),
            };
            <TokensLockedFilter as EthEvent>::decode_log(&raw_log).ok()
        })
        .collect();

    println!("Logging events {}", events.len());
    for event in events {
        println!(
            "🔒 Tokens Locked Event:
        Sender: {:?}
        Amount: {}
        When: {}",
            event.user, event.amount, event.when
        );
        println!("----------------------------------------");
    }

    Ok(())
}


*/
