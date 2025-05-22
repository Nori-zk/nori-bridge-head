use alloy::providers::{Provider, ProviderBuilder};
use alloy_primitives::Address;
use anyhow::{Context, Result};
use nori::{contract::bindings::NoriStateBridge, rpcs::execution::{http::ExecutionHttpProxy, ws::get_source_contract_listener}, utils::enable_logging_from_cargo_run};
use std::env;

async fn main_ws() -> Result<()> {
    dotenv::dotenv().ok();
    // Validate everything upfront

    let eth_ws_rpc =
        env::var("NORI_ETH_WS_RPC").context("Missing NORI_ETH_WS_RPC in environment")?;

    let token_bridge_address = env::var("NORI_TOKEN_BRIDGE_ADDRESS")
        .context("Missing NORI_TOKEN_BRIDGE_ADDRESS in environment")?
        .parse::<Address>()
        .context("Invalid Ethereum address format")?;

    let mut contract_update_rx =
        get_source_contract_listener(&eth_ws_rpc, &token_bridge_address).await?;

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

async fn main_http() -> Result<()> {
    dotenv::dotenv().ok();

    // Validate everything upfront
    let eth_http_rpc =
        env::var("NORI_ETH_HTTP_RPC").context("Missing NORI_ETH_HTTP_RPC in environment")?;

    let rpc_url = eth_http_rpc.parse()?;
    let provider = ProviderBuilder::new().on_http(rpc_url);

    let end_block = provider.get_block_number().await?;
    let start_block = end_block - 50;

    let proxy = ExecutionHttpProxy::try_from_env();

    let events = proxy.get_source_contract_events::<NoriStateBridge::TokensLocked>(start_block, end_block).await?;

    println!("Logging events {}", events.len());
    for event in events {
        println!(
            "🔒 Tokens Locked Event:
        Address: {:?}
        Sender: {:?}
        Amount: {}
        When: {}",
            event.address, event.user, event.amount, event.when
        );
        println!("----------------------------------------");
    }
    Ok(())
}


#[tokio::main]
async fn main() {
    // Enable info logging when using cargo --run
    enable_logging_from_cargo_run();
    let args: Vec<String> = env::args().collect();
    let last_arg = &args[args.len() - 1];
    if last_arg == "ws" {
        main_ws().await.unwrap();
    } else {
        main_http().await.unwrap();
    }
}
