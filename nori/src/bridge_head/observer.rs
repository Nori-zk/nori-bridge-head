use super::{
    api::{BridgeHeadEvent, ProofMessage},
    handles::CommandHandle,
    notice_messages::TransitionNoticeBridgeHeadMessage,
};
use crate::{
    bridge_head::checkpoint::save_nb_checkpoint,
    utils::{handle_nori_proof, handle_nori_proof_message, panic_more},
};
use alloy_primitives::FixedBytes;
use anyhow::Context;
use async_trait::async_trait;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use log::{info, warn};
use nori_sp1_helios_primitives::types::ProofInputsWithWindow;

/// Event observer trait for handling bridge head events
#[async_trait]
pub trait EventObserver: Send + Sync {
    /// Called when a new proof is generated
    async fn on_transition_proof_succeeded(&mut self, proof_job_data: ProofMessage);

    /// Called when a bridge head transition notice is generated
    async fn on_bridge_head_transition_notice(
        &mut self,
        notice_data: TransitionNoticeBridgeHeadMessage,
    );

    /// Run method default for handling event messages
    async fn run(
        &mut self,
        mut bridge_head_event_receiver: tokio::sync::mpsc::Receiver<BridgeHeadEvent>,
    ) {
        loop {
            tokio::select! {
                msg = bridge_head_event_receiver.recv() => {
                    match msg {
                        Some(event) => {
                            // Process the event by matching its variant.
                            match event {
                                BridgeHeadEvent::ProofMessage(proof_msg) => {
                                    // Call on_proof for a Proof event.
                                    self.on_transition_proof_succeeded(proof_msg).await;
                                },
                                BridgeHeadEvent::NoticeMessage(notice_msg) => {
                                    // Call on_notice for a Notice event.
                                    self.on_bridge_head_transition_notice(notice_msg).await;
                                },
                            }
                        },
                        None => {
                            panic_more("Event receiver for BridgeHead dropped")
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
    /// Indicates whether the bridge head has fired its started event.
    started: bool,
    /// Current bridge slot head
    current_slot: u64,
    /// FixedBytes representing the store hash
    store_hash: FixedBytes<32>,
    /// Boolean for indicating a job should be issued on the next beacon slot change event
    stage_transition_proof: bool,
    /// Latest validated proof input for the current window
    latest_current_window_validated_proof_input:
        Option<ProofInputsWithWindow<MainnetConsensusSpec>>,
    /// Latest validated proof input for the next window
    latest_next_window_validated_proof_input: Option<ProofInputsWithWindow<MainnetConsensusSpec>>,
}

impl ExampleBridgeHeadEventObserver {
    pub fn new(bridge_head_handle: CommandHandle) -> Self {
        Self {
            bridge_head_handle,
            started: false,
            current_slot: 0,
            store_hash: FixedBytes::default(),
            stage_transition_proof: false,
            latest_current_window_validated_proof_input: None,
            latest_next_window_validated_proof_input: None,
        }
    }

    // Advance nori head
    async fn advance(&mut self, slot: u64, store_hash: FixedBytes<32>) {
        // Update our state and the bridge heads state
        self.current_slot = slot;
        self.store_hash = store_hash;
        // Save the checkpoint
        save_nb_checkpoint(self.current_slot, self.store_hash);

        // Advance the bridge head
        if self
            .bridge_head_handle
            .advance(slot, store_hash)
            .await
            .is_err()
        {
            panic_more("Bridge head command channel closed");
        }
    }
}

#[async_trait]
impl EventObserver for ExampleBridgeHeadEventObserver {
    async fn on_transition_proof_succeeded(&mut self, proof_data: ProofMessage) {
        println!("PROOF| {}", proof_data.input_slot);

        info!("Saving Nori sp1 proof.");
        if handle_nori_proof(&proof_data.proof, proof_data.input_slot)
            .await
            .is_err()
        {
            panic_more("Could not save new Sp1 proof to disk!");
        };
        if handle_nori_proof_message(&proof_data).await.is_err() {
            panic_more("Could not save new Sp1 proof message to disk!");
        };

        if proof_data.output_slot > self.current_slot {
            info!("Proof advanced the bridge head.");
        } else {
            warn!("PROOF DID NOT ADVANCE BRIDGE HEAD");
            warn!("PROOF DID NOT ADVANCE BRIDGE HEAD");
            warn!("PROOF DID NOT ADVANCE BRIDGE HEAD");
        }
        // Advance the head
        info!("Advancing the bridge head.");
        self.advance(proof_data.output_slot, proof_data.output_store_hash)
            .await;

        // Here we should check if we have prepared finality proof inputs for the next window. If we do we don't need
        // to stage for the next transition proof we can use what is in cache directly.
        if let Some(ref next_window) = self.latest_next_window_validated_proof_input {
            info!("We have proof inputs for the next window, determining VIABILITY.");
            // If we have a valid next window we should use it immediately
            if next_window.input_slot == proof_data.output_slot {
                info!("VIABLE: Next window proof inputs ARE contiguous, proof data output slot '{}', next window input slot: '{}', staging the next proof immediately.", proof_data.output_slot, next_window.input_slot);
                // The windows are contiguous so we can immediately start on the next proof
                if self
                    .bridge_head_handle
                    .stage_transition_proof(next_window.clone())
                    .await
                    .is_err()
                {
                    panic_more("Bridge head command channel closed");
                };
                return;
            } else {
                info!("NOT VIABLE: Next window proof inputs are NOT contiguous, proof data output slot '{}', next window input slot: '{}'", proof_data.output_slot, next_window.input_slot);
            }
        }

        // We should wait until finality has advanced to trigger our next proof unless the beacon slot has advanced...
        // However this proofinputs we have here is proved from our old slot to current finality and thus
        // is invalid (we would emit a proof from the same input slot again....)
        // ... so we should just mark the stage_transition_proof which will emit when there is a finality detected
        // which is beyond our roof_data.output_slot, note this might not need to wait for the
        // next beacon finality transition this could have already happened and so might emit almost straight away.
        info!(
            "We only have proof inputs for the current window. Queuing a job for the next detected finality advancement beyond slot '{}'.",
            proof_data.output_slot
        );
        self.stage_transition_proof = true;
    }

    async fn on_bridge_head_transition_notice(
        &mut self,
        notice_data: TransitionNoticeBridgeHeadMessage,
    ) {
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


                self.stage_transition_proof = true;
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
                    data.extension.job_id, data.extension.error
                );

                // If there are no other jobs in the queue retry the failure
                // FIXME TODO perhaps we should not retry the exact same job and just wait for the next finality...?
                // We are running behind for no reason here.... Also would simplify the logic here and allow us to remove
                // latest_validated_proof_input_with_window because we don't use it anywhere else!
                // But this change might end up in needless waiting. Say there is very short term network downtime. What about
                // number of retries?
                if data.extension.n_job_in_buffer == 0 {
                    if let Some(proof_input_with_window) =
                        self.latest_current_window_validated_proof_input.clone()
                    {
                        if self
                            .bridge_head_handle
                            .stage_transition_proof(proof_input_with_window)
                            .await
                            .is_err()
                        {
                            panic_more("Bridge head command channel closed");
                        };
                    } else {
                        panic_more("Tried to redo a job but latest_validated_proof_input_with_window was not defined");
                    }
                }
            }
            TransitionNoticeBridgeHeadMessage::FinalityTransitionDetected(data) => {
                info!(
                    "NOTICE_TYPE| Finality Transition Detected: Slot: {:?}, Block Number: {:?}",
                    data.extension.slot, data.extension.block_number
                );

                self.latest_current_window_validated_proof_input = Some(
                    *data
                        .extension
                        .current_window_proof_inputs_with_window
                        .clone(),
                );
                self.latest_next_window_validated_proof_input = data
                    .extension
                    .next_window_proof_inputs_with_window
                    .as_deref()
                    .cloned();

                if self.stage_transition_proof {
                    if self
                        .bridge_head_handle
                        .stage_transition_proof(
                            *data
                                .extension
                                .current_window_proof_inputs_with_window
                                .clone(),
                        )
                        .await
                        .is_err()
                    {
                        panic_more("Bridge head command channel closed");
                    };
                    self.stage_transition_proof = false;
                }
            }
            TransitionNoticeBridgeHeadMessage::HeadAdvanced(data) => {
                info!("NOTICE_TYPE| Head Advanced: {:?}", data.extension.slot);
            }
        }

        let json_result =
            serde_json::to_string(&notice_data).context("Failed to serialize notice data");

        match json_result {
            Ok(json) => info!("NOTICE_DATA| {}", json),
            Err(e) => warn!("Failed to serialise notice data: {}", e),
        };
    }
}
