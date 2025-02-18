use tokio::sync::mpsc::Sender;
use crate::beacon_finality_change_detector::api::FinalityChangeMessage;

/// Event loop commands
pub enum NoriBridgeEventLoopCommand {
    Advance,
    BeaconFinalityChange(FinalityChangeMessage),
}

#[derive(Clone)]
pub struct NoriBridgeHeadAdvanceHandle {
    command_tx: Sender<NoriBridgeEventLoopCommand>,
}

impl NoriBridgeHeadAdvanceHandle {
    pub fn new(command_tx: Sender<NoriBridgeEventLoopCommand>) -> Self {
        Self {
            command_tx
        }
    }

    pub async fn advance(&self) {
        let _ = self.command_tx.send(NoriBridgeEventLoopCommand::Advance).await;
    }
}


#[derive(Clone)]
pub struct NoriBridgeHeadBeaconFinalityChangeHandle {
    command_tx: Sender<NoriBridgeEventLoopCommand>,
}

impl NoriBridgeHeadBeaconFinalityChangeHandle {
    pub fn new(command_tx: Sender<NoriBridgeEventLoopCommand>) -> Self {
        Self {
            command_tx
        }
    }

    pub async fn on_beacon_change(&self, slot: u64) {
        let _ = self.command_tx.send(NoriBridgeEventLoopCommand::BeaconFinalityChange(FinalityChangeMessage { slot })).await;
    }
}