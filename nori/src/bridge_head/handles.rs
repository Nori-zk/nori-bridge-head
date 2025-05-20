use alloy_primitives::FixedBytes;
use helios_consensus_core::{consensus_spec::MainnetConsensusSpec, types::FinalityUpdate};
use tokio::sync::mpsc::Sender;

use crate::helios::FinalityUpdateAndSlot;

/// Event loop commands

pub struct AdvanceMessage {
    pub slot: u64,
    pub store_hash: FixedBytes<32>,
}

pub struct StageTransitionProofMessage {
    pub slot: u64,
    pub finality_update: FinalityUpdate<MainnetConsensusSpec>,
    pub store_hash: FixedBytes<32>,
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

    pub async fn stage_transition_proof(&self, slot: u64, store_hash: FixedBytes<32>, finality_update: FinalityUpdate<MainnetConsensusSpec>) {
        let _ = self.command_tx.send(Command::StageTransitionProof(StageTransitionProofMessage { slot, store_hash, finality_update })).await;
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
