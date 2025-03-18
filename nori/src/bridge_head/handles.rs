use crate::beacon_finality_change_detector::api::FinalityChangeMessage;
use alloy_primitives::FixedBytes;
use tokio::sync::mpsc::Sender;

/// Event loop commands

pub struct AdvanceMessage {
    pub head: u64,
    pub next_sync_committee: FixedBytes<32>,
}

pub enum Command {
    PrepareTransitionProof,
    Advance(AdvanceMessage),
    BeaconFinalityChange(FinalityChangeMessage),
}

#[derive(Clone)]
pub struct CommandHandle {
    command_tx: Sender<Command>,
}

impl CommandHandle {
    pub fn new(command_tx: Sender<Command>) -> Self {
        Self { command_tx }
    }

    pub async fn prepare_transition_proof(&self) {
        let _ = self.command_tx.send(Command::PrepareTransitionProof).await;
    }

    pub async fn advance(&self, head: u64, next_sync_committee: FixedBytes<32>) {
        let _ = self.command_tx.send(Command::Advance(AdvanceMessage {head, next_sync_committee})).await;
    }
}

#[derive(Clone)]
pub struct BeaconFinalityChangeHandle {
    command_tx: Sender<Command>,
}

impl BeaconFinalityChangeHandle {
    pub fn new(command_tx: Sender<Command>) -> Self {
        Self { command_tx }
    }

    pub async fn on_beacon_change(&self, slot: u64) {
        let _ = self
            .command_tx
            .send(Command::BeaconFinalityChange(FinalityChangeMessage {
                slot,
            }))
            .await;
    }
}
