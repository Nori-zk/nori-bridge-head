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
    async fn on_transition_proof_succeeded(
        &mut self,
        proof_job_data: ProofMessage,
    ) -> anyhow::Result<()>;
    /// Called when a bridge head transition notice is generated
    async fn on_bridge_head_transition_notice(
        &mut self,
        notice_data: TransitionNoticeBridgeHeadMessage,
    ) -> anyhow::Result<()>;

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
    /// Tracks the current slot for beacon finality.
    latest_beacon_finality_slot: u64,
    /// Indicates whether the bridge head has fired its started event.
    started: bool,
    /// Current bridge slot head
    current_slot: u64,
    /// FixedBytes representing the store hash
    store_hash: FixedBytes<32>,
    /// Boolean for indicating a job should be issued on the next beacon slot change event
    stage_transition_proof: bool,
}

impl ExampleBridgeHeadEventObserver {
    pub fn new(bridge_head_handle: CommandHandle) -> Self {
        Self {
            bridge_head_handle,
            latest_beacon_finality_slot: 0,
            started: false,
            current_slot: 0,
            store_hash: FixedBytes::default(),
            stage_transition_proof: false,
        }
    }

    // Advance nori head
    async fn advance(&mut self, slot: u64, store_hash: FixedBytes<32>) {
        // Update our state and the bridge heads state
        self.current_slot = slot;
        self.store_hash = store_hash;
        // Advance the bridge head
        let _ = self.bridge_head_handle.advance(slot, store_hash).await;
    }
}

#[async_trait]
impl EventObserver for ExampleBridgeHeadEventObserver {
    async fn on_transition_proof_succeeded(&mut self, proof_data: ProofMessage) -> Result<()> {
        println!("PROOF| {}", proof_data.input_slot);

        // Double check our proof actually advanced the head
        if proof_data.output_slot > self.current_slot {
            info!("Proof advanced the bridge head.");

            info!("Saving Nori sp1 proof.");
            let _ = handle_nori_proof(&proof_data.proof, proof_data.input_slot).await;

            // Advance the head
            info!("Advancing the bridge head.");
            let _ = self
                .advance(proof_data.output_slot, proof_data.output_store_hash)
                .await;

            // We should wait until finality has advanced to trigger our next proof.
            if self.latest_beacon_finality_slot > self.current_slot {
                info!("Latest beacon finality slot is advanced of our proofs output slot, starting a new job.");
                let _ = self
                    .bridge_head_handle
                    .stage_transition_proof(self.current_slot, self.store_hash)
                    .await;
            } else {
                info!("Latest beacon finality slot not is advanced of our proofs output slot, queuing a job for the next finality transition.");
                self.stage_transition_proof = true;
            }
        } else {
            info!("Proof failed to advance the bridge head. Queuing another job for the next finality transition.");
            self.stage_transition_proof = true;
        }

        Ok(())
    }

    async fn on_bridge_head_transition_notice(
        &mut self,
        notice_data: TransitionNoticeBridgeHeadMessage,
    ) -> Result<()> {
        // Do something specific
        match &notice_data {
            TransitionNoticeBridgeHeadMessage::Started(data) => {
                info!(
                    "NOTICE_TYPE| Started. Current head: {:?} Finality slot: {:?}",
                    data.extension.current_slot, data.extension.latest_beacon_slot
                );

                self.started = true;
                self.current_slot = data.extension.current_slot;
                self.store_hash = data.extension.store_hash;

                if data.extension.latest_beacon_slot > self.latest_beacon_finality_slot {
                    self.latest_beacon_finality_slot = data.extension.latest_beacon_slot;
                }
                // During initial startup we need to immediately check if genesis finality head has moved in order to apply any updates
                // that happened while this process was offline

                info!("Staging a transition proof with input slot {}", self.current_slot);
                let _ = self
                    .bridge_head_handle
                    .stage_transition_proof(self.current_slot, self.store_hash)
                    .await;
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
                    let _ = self
                        .bridge_head_handle
                        .stage_transition_proof(self.current_slot, self.store_hash)
                        .await;
                }
            }
            TransitionNoticeBridgeHeadMessage::FinalityTransitionDetected(data) => {
                info!(
                    "NOTICE_TYPE| Finality Transition Detected: {:?}",
                    data.extension.slot
                );
                self.latest_beacon_finality_slot = data.extension.slot;

                if self.stage_transition_proof {
                    let _ = self
                        .bridge_head_handle
                        .stage_transition_proof(self.current_slot, self.store_hash)
                        .await;
                    self.stage_transition_proof = false;
                }
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
