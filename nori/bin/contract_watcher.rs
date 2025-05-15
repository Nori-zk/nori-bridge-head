use nori::{contract_watcher::watcher::get_source_contract_listener};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
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
                // Add your custom logic here
            }
            Err(e) => {
                eprintln!("⚠️ Error processing event: {}", e);
                // Add error recovery logic here if needed
            }
        }
    }
    
    println!("🔴 Event listener stopped");
    Ok(())
}