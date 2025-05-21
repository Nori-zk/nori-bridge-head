use alloy_primitives::FixedBytes;
use helios_consensus_core::{consensus_spec::MainnetConsensusSpec, types::FinalityUpdate};
use nori_sp1_helios_primitives::types::ProofInputs;
use tokio::sync::mpsc::Sender;

/// Event loop commands

pub struct AdvanceMessage {
    pub slot: u64,
    pub store_hash: FixedBytes<32>,
}

pub struct StageTransitionProofMessage {
    pub slot: u64,
    pub proof_inputs: ProofInputs<MainnetConsensusSpec>,
}

pub enum Command {
    StageTransitionProof(StageTransitionProofMessage),
    Advance(AdvanceMessage),
}

#[derive(Clone)]
pub struct CommandHandle {
    command_tx: Sender<Command>,
}

impl CommandHandle {
    pub fn new(command_tx: Sender<Command>) -> Self {
        Self { command_tx }
    }

    pub async fn stage_transition_proof(&self, slot: u64, proof_inputs: ProofInputs<MainnetConsensusSpec>) {
        let _ = self.command_tx.send(Command::StageTransitionProof(StageTransitionProofMessage { slot, proof_inputs })).await;
    }

    pub async fn advance(&self, slot: u64, store_hash: FixedBytes<32>) {
        let _ = self
            .command_tx
            .send(Command::Advance(AdvanceMessage {
                slot,
                store_hash
            }))
            .await;
    }
}
