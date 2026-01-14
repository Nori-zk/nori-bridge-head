use alloy_primitives::FixedBytes;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use nori_sp1_helios_primitives::types::ProofInputsWithWindow;
use tokio::sync::mpsc::{error::SendError, Sender};

/// Event loop commands

pub struct AdvanceMessage {
    pub slot: u64,
    pub store_hash: FixedBytes<32>,
}

pub enum Command {
    StageTransitionProof(Box<ProofInputsWithWindow<MainnetConsensusSpec>>),
    Advance(AdvanceMessage),
}

#[derive(Clone)]
pub struct CommandHandle {
    command_tx: Sender<Command>,
}

/// Bridge head command handle
impl CommandHandle {
    pub fn new(command_tx: Sender<Command>) -> Self {
        Self { command_tx }
    }

    /// Send a message to the bridge head api to stage an SP1 job
    pub async fn stage_transition_proof(
        &self,
        proof_inputs_with_window: ProofInputsWithWindow<MainnetConsensusSpec>,
    ) -> Result<(), SendError<Command>> {
        return self
            .command_tx
            .send(Command::StageTransitionProof(Box::new(
                proof_inputs_with_window,
            )))
            .await;
    }

    /// Send a message to the bridge head to inform it of a finality advancement after it has been settled on Mina
    pub async fn advance(
        &self,
        slot: u64,
        store_hash: FixedBytes<32>,
    ) -> Result<(), SendError<Command>> {
        return self
            .command_tx
            .send(Command::Advance(AdvanceMessage { slot, store_hash }))
            .await;
    }
}
