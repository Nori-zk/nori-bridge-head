use anyhow::{Context, Result};
use ethers::{
    providers::{Http, Middleware, Provider},
    types::Address,
};
use nori::contract_watcher::{
    http::get_source_contract_events_between_blocks, watcher::get_source_contract_listener,
};
use std::{env, sync::Arc};

//#[tokio::main]
async fn main_old() -> Result<()> {
    let mut contract_update_rx = get_source_contract_listener().await?;

    println!("Started listening for contract events...");

    while let Some(event) = contract_update_rx.recv().await {
        match event {
            Ok(evt) => {
                println!(
                    "🔔 New Token Lock: \n\
                     User: {:?}\n\
                     Amount: {}\n\
                     Timestamp: {}",
                    evt.user, evt.amount, evt.when
                );
            }
            Err(e) => {
                eprintln!("⚠️ Error processing event: {}", e);
                // Add error recovery logic here
            }
        }
    }

    println!("🔴 Event listener stopped");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    // Validate everything upfront
    let eth_http_rpc =
        env::var("NORI_ETH_HTTP_RPC").context("Missing NORI_ETH_HTTP_RPC in environment")?;

    let token_bridge_address = env::var("NORI_TOKEN_BRIDGE_ADDRESS")
        .context("Missing NORI_TOKEN_BRIDGE_ADDRESS in environment")?
        .parse::<Address>()
        .context("Invalid Ethereum address format")?;

    let provider = Provider::<Http>::try_from(eth_http_rpc.clone())?;
    let provider = Arc::new(provider);
    let end_block = provider.get_block_number().await?.as_u64();
    let start_block = end_block - 50;

    let events = get_source_contract_events_between_blocks(
        &eth_http_rpc,
        &token_bridge_address,
        start_block,
        end_block,
    )
    .await?;

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

