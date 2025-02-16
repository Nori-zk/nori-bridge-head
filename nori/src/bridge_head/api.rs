use log::warn;
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc;
use anyhow::Result;
use super::event_handler::NoriBridgeHeadEventProducer;
use super::event_loop::{BridgeHeadEventLoop, NoriBridgeEventLoopCommand, NoriBridgeHeadProofMessage};
use super::notice_messages::NoriBridgeHeadNoticeMessage;

pub struct BridgeHead {
    event_loop_tx: Sender<NoriBridgeEventLoopCommand>,
    loop_running: bool
}

impl BridgeHead {
    pub async fn new() -> Self {
        // This is stupid
        let event_loop_tx: Sender<NoriBridgeEventLoopCommand> = mpsc::channel(1).0;
        Self {
            event_loop_tx,
            loop_running: false,
        }
    }

    pub async fn run(&mut self, listener: impl NoriBridgeHeadEventProducer<NoriBridgeHeadProofMessage, NoriBridgeHeadNoticeMessage> + Send + Sync + 'static) {
        if !self.loop_running {
            let boxed_listener: Box<dyn NoriBridgeHeadEventProducer<NoriBridgeHeadProofMessage, NoriBridgeHeadNoticeMessage> + Send + Sync> = Box::new(listener);
            let (tx, rx) = mpsc::channel(1);
            let event_loop = BridgeHeadEventLoop::new(rx, boxed_listener).await;
            self.event_loop_tx = tx;
            tokio::spawn(event_loop.run_loop());
            self.loop_running = true;
        }
        else {
            warn!("Run has already been invoked.");
        }
    }

    pub async fn advance(&mut self) -> Result<()> {
        self.event_loop_tx.send(NoriBridgeEventLoopCommand::Advance).await?;
        Ok(())
    }

}

// Todo add advance and add event listener to this interface