use anyhow::{Context, Result};
use ethers::{prelude::*, providers::{Ws,Http}};
use std::{env, sync::Arc};
use tokio::sync::mpsc;
use super::bindings::nori_token_bridge::{NoriTokenBridge, TokensLockedFilter};

pub async fn get_source_contract_listener(
) -> Result<mpsc::Receiver<Result<TokensLockedFilter, anyhow::Error>>> {
    dotenv::dotenv().ok();

    // Validate everything upfront
    let eth_ws_rpc =
        env::var("NORI_ETH_WS_RPC").context("Missing NORI_ETH_WS_RPC in environment")?;

    let token_bridge_address = env::var("NORI_TOKEN_BRIDGE_ADDRESS")
        .context("Missing NORI_TOKEN_BRIDGE_ADDRESS in environment")?
        .parse::<Address>()
        .context("Invalid Ethereum address format")?;

    let provider = Provider::<Ws>::connect(&eth_ws_rpc)
        .await
        .context("Failed to connect to WebSocket provider")?;
    let provider = Arc::new(provider);

    let contract = NoriTokenBridge::new(token_bridge_address, provider.clone());

    // Contract locked events
    let contract_locked_event = contract.event::<TokensLockedFilter>();

    // Create channel after successful validation
    let (tx, rx) = mpsc::channel(32);

    // Spawn task for event handling
    tokio::spawn(async move {
        let mut stream = match contract_locked_event.subscribe().await {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.send(Err(e.into())).await;
                return;
            }
        };

        while let Some(event) = stream.next().await {
            let result = tx
                .send(event.map_err(|e| anyhow::anyhow!("Event error: {}", e)))
                .await;

            if result.is_err() {
                break; // Receiver dropped
            }
        }
    });

    Ok(rx)
}
