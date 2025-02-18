use async_trait::async_trait;
use anyhow::{Context, Result};
use log::info;
use crate::utils::handle_nori_proof;
use super::{api::ProofMessage, handles::AdvanceHandle, notice_messages::{NoticeMessageExtension, NoticeMessage}};

// Observer trait
#[async_trait]
pub trait EventObserver: Send + Sync {
    async fn on_proof(&mut self, proof_job_data: ProofMessage) -> Result<()>;
    async fn on_notice(&mut self, notice_data: NoticeMessage) -> Result<()>;
}

// Example event observer

pub struct ExampleEventObserver {
    bridge_head: AdvanceHandle,
}

impl ExampleEventObserver {
    pub fn new(bridge_head: AdvanceHandle) -> Self {
        Self { bridge_head }
    }
}

#[async_trait]
impl EventObserver
    for ExampleEventObserver
{
    async fn on_proof(&mut self, proof_data: ProofMessage) -> Result<()> {
        println!("PROOF| {}", proof_data.slot);

        info!("Saving Nori sp1 proof");
        handle_nori_proof(&proof_data.proof, proof_data.slot).await?;

        self.bridge_head.advance().await;

        Ok(())
    }

    async fn on_notice(&mut self, notice_data: NoticeMessage) -> Result<()> {       
        // Do something specific
        match notice_data.clone().extension {
            NoticeMessageExtension::Started(data) => {
                println!("NOTICE_TYPE| Started");
            }
            NoticeMessageExtension::Warning(data) => {
                println!("NOTICE_TYPE| Warning: {:?}", data.message);
            }
            NoticeMessageExtension::JobCreated(data) => {
                println!("NOTICE_TYPE| Job Created: {:?}", data.job_idx);
            }
            NoticeMessageExtension::JobSucceeded(data) => {
                println!("NOTICE_TYPE| Job Succeeded: {:?}", data.job_idx);
            }
            NoticeMessageExtension::JobFailed(data) => {
                println!("NOTICE_TYPE| Job Failed: {:?}", data.job_idx);
            }
            NoticeMessageExtension::FinalityTransitionDetected(data) => {
                println!("NOTICE_TYPE| Finality Transition Detected: {:?}", data.slot);
            }
            NoticeMessageExtension::AdvanceRequested(data) => {
                println!("NOTICE_TYPE| Advance Requested");
            }
            NoticeMessageExtension::HeadAdvanced(data) => {
                println!("NOTICE_TYPE| Head Advanced: {:?}", data.slot);
            }
        }

        let json =
            serde_json::to_string(&notice_data).context("Failed to serialize notice data")?;

        println!("NOTICE_DATA| {}", json);

        Ok(())
    }
}