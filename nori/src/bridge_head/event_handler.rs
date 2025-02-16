use std::sync::Arc;
use async_trait::async_trait;
use anyhow::{Context, Result};
use log::info;
use tokio::sync::Mutex;
use crate::utils::handle_nori_proof;
use super::{api::BridgeHead, event_loop::NoriBridgeHeadProofMessage, notice_messages::{NoriBridgeHeadNoticeMessage, NoriBridgeHeadMessageExtension}};

/// Trait
#[async_trait]
pub trait NoriBridgeHeadEventProducer<T: Clone, Q: Clone>: Send + Sync {
    async fn on_proof(&mut self, proof_job_data: T) -> Result<()>;
    async fn on_notice(&mut self, notice_data: Q) -> Result<()>;
}

/// Example event handler

pub struct ExampleEventHandler {
    bridge_head: Arc<Mutex<BridgeHead>>, // Use Arc<Mutex<NoriBridgeHead>> for shared ownership
}

impl ExampleEventHandler {
    pub fn new(bridge_head: Arc<Mutex<BridgeHead>>) -> Self {
        Self { bridge_head }
    }
}

#[async_trait]
impl NoriBridgeHeadEventProducer<NoriBridgeHeadProofMessage, NoriBridgeHeadNoticeMessage>
    for ExampleEventHandler
{
    async fn on_proof(&mut self, proof_data: NoriBridgeHeadProofMessage) -> Result<()> {
        println!("PROOF| {}", proof_data.slot);

        info!("Saving Nori sp1 proof");
        handle_nori_proof(&proof_data.proof, proof_data.slot).await?;

        // Acquire lock and call advance()
        let mut bridge = self.bridge_head.lock().await;
        bridge.advance().await?;

        Ok(())
    }

    async fn on_notice(&mut self, notice_data: NoriBridgeHeadNoticeMessage) -> Result<()> {       
        // Do something specific
        match notice_data.clone().extension {
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeStarted(data) => {
                println!("NOTICE_TYPE| Started");
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeWarning(data) => {
                println!("NOTICE_TYPE| Warning: {:?}", data.message);
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeJobCreated(data) => {
                println!("NOTICE_TYPE| Job Created: {:?}", data.job_idx);
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeJobSucceeded(data) => {
                println!("NOTICE_TYPE| Job Succeeded: {:?}", data.job_idx);
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeJobFailed(data) => {
                println!("NOTICE_TYPE| Job Failed: {:?}", data.job_idx);
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeFinalityTransitionDetected(data) => {
                println!("NOTICE_TYPE| Finality Transition Detected: {:?}", data.slot);
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeAdvanceRequested(data) => {
                println!("NOTICE_TYPE| Advance Requested");
            }
            NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeHeadAdvanced(data) => {
                println!("NOTICE_TYPE| Head Advanced: {:?}", data.slot);
            }
        }

        let json =
            serde_json::to_string(&notice_data).context("Failed to serialize notice data")?;

        println!("NOTICE_DATA| {}", json);

        Ok(())
    }
}