use alloy_primitives::FixedBytes;
use helios_consensus_core::{consensus_spec::MainnetConsensusSpec};
use nori_sp1_helios_primitives::types::ProofInputsWithWindow;
use tokio::sync::mpsc::Sender;

/// Event loop commands

pub struct AdvanceMessage {
    pub slot: u64,
    pub store_hash: FixedBytes<32>,
}

/*pub struct StageTransitionProofMessage {
    pub slot: u64,
    pub proof_inputs_with_window: Box<ProofInputsWithWindow<MainnetConsensusSpec>>,
}*/

pub enum Command {
    StageTransitionProof(Box<ProofInputsWithWindow<MainnetConsensusSpec>>),
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

    pub async fn stage_transition_proof(&self, proof_inputs_with_window: ProofInputsWithWindow<MainnetConsensusSpec>) {
        let _ = self.command_tx.send(Command::StageTransitionProof(Box::new(proof_inputs_with_window))).await;
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
