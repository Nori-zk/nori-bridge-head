use async_trait::async_trait;
use anyhow::{Context, Result};
use log::info;
use crate::utils::handle_nori_proof;
use super::{api::NoriBridgeHeadProofMessage, handles::NoriBridgeHeadAdvanceHandle, notice_messages::{NoriBridgeHeadMessageExtension, NoriBridgeHeadNoticeMessage}};

// Observer trait
#[async_trait]
pub trait NoriBridgeHeadEventObserver: Send + Sync {
    async fn on_proof(&mut self, proof_job_data: NoriBridgeHeadProofMessage) -> Result<()>;
    async fn on_notice(&mut self, notice_data: NoriBridgeHeadNoticeMessage) -> Result<()>;
}

// Example event observer

pub struct ExampleEventObserver {
    bridge_head: NoriBridgeHeadAdvanceHandle,
}

impl ExampleEventObserver {
    pub fn new(bridge_head: NoriBridgeHeadAdvanceHandle) -> Self {
        Self { bridge_head }
    }
}

#[async_trait]
impl NoriBridgeHeadEventObserver
    for ExampleEventObserver
{
    async fn on_proof(&mut self, proof_data: NoriBridgeHeadProofMessage) -> Result<()> {
        println!("PROOF| {}", proof_data.slot);

        info!("Saving Nori sp1 proof");
        handle_nori_proof(&proof_data.proof, proof_data.slot).await?;

        self.bridge_head.advance().await;

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