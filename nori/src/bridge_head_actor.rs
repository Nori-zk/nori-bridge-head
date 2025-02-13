use std::sync::Arc;
use log::info;
use tokio::sync::{mpsc::Sender, Mutex};
use tokio::sync::mpsc;
use crate::{bridge_head_event_loop::{BridgeHeadEventLoop, NoriBridgeEventLoopCommand, NoriBridgeHeadProofMessage}, event_dispatcher::EventListener};
use anyhow::Result;

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

    pub async fn advance(&mut self) -> Result<()> {
        self.event_loop_tx.send(NoriBridgeEventLoopCommand::Advance).await?;
        Ok(())
    }

    pub async fn add_proof_listener(&mut self, listener: impl EventListener<NoriBridgeHeadProofMessage> + Send + Sync + 'static,) -> Result<()> {
        let boxed_listener: Box<dyn EventListener<NoriBridgeHeadProofMessage> + Send + Sync> = Box::new(listener);
        info!("Boxed listener");
        let wrapped_listener = Arc::new(Mutex::new(boxed_listener));
        info!("Wrapped listener");
        self.event_loop_tx.send(NoriBridgeEventLoopCommand::AddProofListener { listener: wrapped_listener }).await?;
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        self.event_loop_tx.send(NoriBridgeEventLoopCommand::Shutdown).await?;
        Ok(())
    }
}

// Todo add advance and add event listener to this interface