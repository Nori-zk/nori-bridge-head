use tokio::sync::mpsc::Sender;

/// Event loop commands
pub enum NoriBridgeEventLoopCommand {
    Advance,
}

#[derive(Clone)]
pub struct NoriBridgeHeadHandle {
    command_tx: Sender<NoriBridgeEventLoopCommand>,
}

impl NoriBridgeHeadHandle {
    pub fn new(command_tx: Sender<NoriBridgeEventLoopCommand>) -> Self {
        Self {
            command_tx
        }
    }

    pub async fn advance(&self) {
        let _ = self.command_tx.send(NoriBridgeEventLoopCommand::Advance).await;
    }
}