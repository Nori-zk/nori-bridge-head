use super::{
    api::{BridgeHeadEvent, ProofMessage},
    handles::CommandHandle,
    notice_messages::TransitionNoticeBridgeHeadMessage,
};
use crate::utils::handle_nori_proof;
use alloy_primitives::FixedBytes;
use anyhow::{Context, Result};
use async_trait::async_trait;
use log::{info, warn};

/// Event observer trait for handling bridge head events
#[async_trait]
pub trait EventObserver: Send + Sync {
    /// Called when a new proof is generated
    async fn on_transition_proof_succeeded(&mut self, proof_job_data: ProofMessage) -> anyhow::Result<()>;
    /// Called when a bridge head transition notice is generated
    async fn on_bridge_head_transition_notice(&mut self, notice_data: TransitionNoticeBridgeHeadMessage) -> anyhow::Result<()>;

    /// Run method default for handling event messages
    async fn run(
        &mut self,
        mut bridge_head_event_receiver: tokio::sync::broadcast::Receiver<BridgeHeadEvent>,
    ) {
        loop {
            tokio::select! {
                result = bridge_head_event_receiver.recv() => {
                    match result {
                        Ok(event) => {
                            // Process the event by matching its variant.
                            match event {
                                BridgeHeadEvent::ProofMessage(proof_msg) => {
                                    // Call on_proof for a Proof event.
                                    if let Err(e) = self.on_transition_proof_succeeded(proof_msg).await {
                                        warn!("Error handling proof event: {}", e);
                                    }
                                },
                                BridgeHeadEvent::NoticeMessage(notice_msg) => {
                                    // Call on_notice for a Notice event.
                                    if let Err(e) = self.on_bridge_head_transition_notice(notice_msg).await {
                                        warn!("Error handling notice event: {}", e);
                                    }
                                },
                            }
                        },
                        Err(err) => {
                            warn!("Error receiving event: {}", err);
                            break;
                        },
                    }
                },
            }
        }
    }
}

/// Reference implementation of EventObserver
pub struct ExampleBridgeHeadEventObserver {
    /// Handle to trigger bridge head advancement
    bridge_head_handle: CommandHandle,
}

impl ExampleBridgeHeadEventObserver {
    pub fn new(bridge_head_handle: CommandHandle) -> Self {
        Self { bridge_head_handle }
    }

    // Advance nori head
    async fn advance(
        &mut self,
        slot: u64,
        store_hash: FixedBytes<32>,
    ) {
        let _ = self
            .bridge_head_handle
            .advance(slot, store_hash)
            .await;
    }
}

#[async_trait]
impl EventObserver for ExampleBridgeHeadEventObserver {
    async fn on_transition_proof_succeeded(&mut self, proof_data: ProofMessage) -> Result<()> {
        println!("PROOF| {}", proof_data.input_slot);

        info!("Saving Nori sp1 proof");
        let _ = handle_nori_proof(&proof_data.proof, proof_data.input_slot).await;

        // Advance the head
        let _ = self
            .advance(
                proof_data.output_slot,
                proof_data.output_store_hash,
            )
            .await;

        // Stage the next proof
        let _ = self.bridge_head_handle.stage_transition_proof().await;

        Ok(())
    }

    async fn on_bridge_head_transition_notice(&mut self, notice_data: TransitionNoticeBridgeHeadMessage) -> Result<()> {
        // Do something specific
        match &notice_data {
            TransitionNoticeBridgeHeadMessage::Started(data) => {
                info!(
                    "NOTICE_TYPE| Started. Current head: {:?} Finality slot: {:?}",
                    data.extension.current_slot, data.extension.latest_beacon_slot
                );
                // During initial startup we need to immediately check if genesis finality head has moved in order to apply any updates
                // that happened while this process was offline
                let _ = self.bridge_head_handle.stage_transition_proof().await;
            }
            TransitionNoticeBridgeHeadMessage::Warning(data) => {
                info!("NOTICE_TYPE| Warning: {:?}", data.extension.message);
            }
            TransitionNoticeBridgeHeadMessage::JobCreated(data) => {
                info!("NOTICE_TYPE| Job Created: {:?}", data.extension.job_id);
            }
            TransitionNoticeBridgeHeadMessage::JobSucceeded(data) => {
                info!("NOTICE_TYPE| Job Succeeded: {:?}", data.extension.job_id);
            }
            TransitionNoticeBridgeHeadMessage::JobFailed(data) => {
                info!(
                    "NOTICE_TYPE| Job Failed: {:?}: {}",
                    data.extension.job_id, data.extension.message
                );

                // If there are no other jobs in the queue retry the failure
                if data.extension.n_job_in_buffer == 0 {
                    let _ = self.bridge_head_handle.stage_transition_proof().await;
                }
            }
            TransitionNoticeBridgeHeadMessage::FinalityTransitionDetected(data) => {
                info!(
                    "NOTICE_TYPE| Finality Transition Detected: {:?}",
                    data.extension.slot
                );
            }
            TransitionNoticeBridgeHeadMessage::HeadAdvanced(data) => {
                info!("NOTICE_TYPE| Head Advanced: {:?}", data.extension.slot);
            }
        }

        let json =
            serde_json::to_string(&notice_data).context("Failed to serialize notice data")?;

        info!("NOTICE_DATA| {}", json);

        Ok(())
    }
}
