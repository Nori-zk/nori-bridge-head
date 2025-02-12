use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc;
use crate::bridge_head_event_loop::{BridgeHeadEventLoop, NoriBridgeEventLoopCommand};

pub struct BridgeHeadActor {
    event_loop_tx: Sender<NoriBridgeEventLoopCommand>
}

impl BridgeHeadActor {
    pub async fn new() -> Self {
        // This is stupid
        let event_loop_tx: Sender<NoriBridgeEventLoopCommand> = mpsc::channel(1).0;
        Self {
            event_loop_tx
        }
    }

    pub async fn run(&mut self) {
        let (tx, rx) = mpsc::channel(1);
        let mut event_loop = BridgeHeadEventLoop::new(rx).await;
        self.event_loop_tx = tx;
        tokio::spawn(event_loop.run_loop());
    }
}