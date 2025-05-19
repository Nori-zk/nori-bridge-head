use alloy::eips::BlockNumberOrTag;
use alloy::providers::Provider;
use alloy::sol_types::SolEvent;
use alloy::transports::ws::WsConnect;
use alloy::{providers::ProviderBuilder, rpc::types::Filter};
use alloy_primitives::{Address, Log};
use anyhow::{anyhow, Result};
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::contract::bindings::NoriTokenBridge::{self, TokensLocked};


pub async fn get_source_contract_listener(
    eth_ws_rpc: &str,
    address: &Address,
) -> Result<mpsc::Receiver<Result<Log<TokensLocked>, anyhow::Error>>> {
    dotenv::dotenv().ok();

    let provider = ProviderBuilder::new()
        .on_ws(WsConnect::new(eth_ws_rpc))
        .await?;

    let event_signature = NoriTokenBridge::TokensLocked::SIGNATURE;

    let filter = Filter::new().address(*address).event(event_signature);

    // Create channel after successful validation
    let (tx, rx) = mpsc::channel(32);

    tokio::spawn(async move {
        // Create the subscription inside the spawn
        let sub_result = provider.subscribe_logs(&filter).await;
        
        match sub_result {
            Ok(sub) => {
                let mut stream = sub.into_stream();

                while let Some(log) = stream.next().await {
                    println!("Got a log {}", log.address());
                    let result: Result<Log<TokensLocked>> =
                        NoriTokenBridge::TokensLocked::decode_log(&log.inner, true)
                            .map_err(|e| anyhow!("Failed to decode TokensLocked: {:?}", e));

                    let send_result = tx.send(result).await;

                    if send_result.is_err() {
                        println!("Receiver dropped, stopping listener");
                        break; // Receiver dropped
                    }
                }

                println!("Stream concluded");
            }
            Err(e) => {
                println!("Failed to create subscription: {}", e);
                // Optionally send the error to the receiver
                let _ = tx.send(Err(anyhow!("Subscription failed: {}", e))).await;
            }
        }
    });

    Ok(rx)
}