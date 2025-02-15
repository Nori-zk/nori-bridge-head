use anyhow::{Context, Result};
use async_trait::async_trait;
use log::{info, warn};
use nori::{
    bridge_head::BridgeHead,
    bridge_head_event_loop::NoriBridgeHeadProofMessage,
    event_dispatcher::NoriBridgeEventListener,
    notice_messages::{NoriBridgeHeadMessageExtension, NoriBridgeHeadNoticeMessage},
    utils::{enable_logging_from_cargo_run, handle_nori_proof},
};
use std::process;
use std::sync::Arc;
use tokio::{signal::ctrl_c, sync::Mutex};

pub struct ProofListener {
    bridge_head: Arc<Mutex<BridgeHead>>, // Use Arc<Mutex<NoriBridgeHead>> for shared ownership
}

impl ProofListener {
    pub fn new(bridge_head: Arc<Mutex<BridgeHead>>) -> Self {
        Self { bridge_head }
    }
}

#[async_trait]
impl NoriBridgeEventListener<NoriBridgeHeadProofMessage, NoriBridgeHeadNoticeMessage>
    for ProofListener
{
    async fn on_proof(&mut self, proof_data: NoriBridgeHeadProofMessage) -> Result<()> {
        println!("Got proof message: {}", proof_data.slot);

        handle_nori_proof(&proof_data.proof, proof_data.slot).await?;

        // Acquire lock and call advance()
        let mut bridge = self.bridge_head.lock().await;
        bridge.advance().await?;

        Ok(())
    }
    async fn on_notice(&mut self, notice_data: NoriBridgeHeadNoticeMessage) -> Result<()> {
        warn!("IN ON NOTICE");
        let json =
            serde_json::to_string(&notice_data).context("Failed to serialize notice data")?;

        println!("Got notice message: {}", json);

        // Do something specific
        match notice_data.extension {
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeStarted(data) => {
                println!("Started");
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeWarning(data) => {
                println!("Warning: {:?}", data.message);
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeJobCreated(data) => {
                println!("Job Created: {:?}", data.job_idx);
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeJobSucceeded(data) => {
                println!("Job Succeeded: {:?}", data.job_idx);
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeJobFailed(data) => {
                println!("Job Failed: {:?}", data.job_idx);
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeFinalityTransitionDetected(data) => {
                println!("Finality Transition Detected: {:?}", data.slot);
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeAdvanceRequested(data) => {
                println!("Advance Requested");
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeHeadAdvanced(data) => {
                println!("Head Advanced: {:?}", data.slot);
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Enable info logging when using cargo --run
    enable_logging_from_cargo_run();

    info!("Starting");

    let bridge_head = Arc::new(Mutex::new(BridgeHead::new().await));

    info!("Inited bridge head");

    // Create the ProofListener
    let proof_listener = ProofListener::new(bridge_head.clone());

    info!("Inited proof listener");

    // Add the ProofListener as a boxed listener
    let mut bridge_head_guard = bridge_head.lock().await;

    info!("Locked bridge head");

    info!("Starting nori event loop.");

    // Start the event loop
    bridge_head_guard.run().await;

    info!("Adding proof listener");

    // Add proof listener
    bridge_head_guard.add_listener(proof_listener).await?;

    // Drop the guard
    drop(bridge_head_guard);

    info!("Waiting for exit.");

    // Wait for ctrl-c
    ctrl_c()
        .await
        .expect("Failed to listen for shutdown signal");

    process::exit(1);
}
