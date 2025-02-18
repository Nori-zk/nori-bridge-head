use tokio::sync::mpsc::Sender;
use crate::beacon_finality_change_detector::api::FinalityChangeMessage;

/// Event loop commands
pub enum EventLoopCommand {
    Advance,
    BeaconFinalityChange(FinalityChangeMessage),
}

#[derive(Clone)]
pub struct AdvanceHandle {
    command_tx: Sender<EventLoopCommand>,
}

impl AdvanceHandle {
    pub fn new(command_tx: Sender<EventLoopCommand>) -> Self {
        Self {
            command_tx
        }
    }

    pub async fn advance(&self) {
        let _ = self.command_tx.send(EventLoopCommand::Advance).await;
    }
}


#[derive(Clone)]
pub struct BeaconFinalityChangeHandle {
    command_tx: Sender<EventLoopCommand>,
}

impl BeaconFinalityChangeHandle {
    pub fn new(command_tx: Sender<EventLoopCommand>) -> Self {
        Self {
            command_tx
        }
    }

    pub async fn on_beacon_change(&self, slot: u64) {
        let _ = self.command_tx.send(EventLoopCommand::BeaconFinalityChange(FinalityChangeMessage { slot })).await;
    }
}