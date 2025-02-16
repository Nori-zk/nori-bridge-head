use log::warn;
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc;
use anyhow::Result;
use crate::beacon_finality_change_detector::event_loop::FinalityChangeDetectorEventLoop;

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

    pub async fn run(&mut self, listener: impl NoriBridgeHeadEventProducer + Send + Sync + 'static) {
        if !self.loop_running {
            let boxed_listener: Box<dyn NoriBridgeHeadEventProducer + Send + Sync> = Box::new(listener);
            let (tx, rx) = mpsc::channel(1);
            let event_loop = BridgeHeadEventLoop::new(rx, boxed_listener).await;
            self.event_loop_tx = tx;

            // this event loop needs to go into the beacon event loop as an argument.... and it can call spawn within the loop
            // and then i can put it into the event loop below??

            //let boxed_listener= Box::new(event_loop);
            //let beacon_change_event_loop = FinalityChangeDetectorEventLoop::new(boxed_listener); // this about the run methods... although we would need to enforce that the trait had a run method too

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