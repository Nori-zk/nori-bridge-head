use alloy_primitives::FixedBytes;
use tokio::sync::mpsc::Sender;

/// Event loop commands

pub struct AdvanceMessage {
    pub head: u64,
    pub store_hash: FixedBytes<32>,
}

pub enum Command {
    StageTransitionProof,
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

    pub async fn stage_transition_proof(&self) {
        let _ = self.command_tx.send(Command::StageTransitionProof).await;
    }

    pub async fn advance(&self, head: u64, store_hash: FixedBytes<32>) {
        let _ = self
            .command_tx
            .send(Command::Advance(AdvanceMessage {
                head,
                store_hash
            }))
            .await;
    }
}
