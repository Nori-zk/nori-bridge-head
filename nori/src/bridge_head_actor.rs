use std::process;
use std::sync::Arc;
use log::info;
use tokio::sync::{mpsc::Sender, Mutex};
use tokio::sync::mpsc;
use crate::{bridge_head_event_loop::{BridgeHeadEventLoop, NoriBridgeEventLoopCommand, NoriBridgeHeadProofMessage}, event_dispatcher::EventListener};
use anyhow::Result;

pub struct BridgeHeadActor {
    event_loop_tx: Sender<NoriBridgeEventLoopCommand>,
    loop_running: bool,
    proof_listeners_buffer: Vec<Arc<Mutex<Box<dyn EventListener<NoriBridgeHeadProofMessage> + Send + Sync>>>>
}

impl BridgeHeadActor {
    pub async fn new() -> Self {
        // This is stupid
        let event_loop_tx: Sender<NoriBridgeEventLoopCommand> = mpsc::channel(1).0;
        Self {
            event_loop_tx,
            loop_running: false,
            proof_listeners_buffer: Vec::new()
        }
    }

    pub async fn run(&mut self) {
        let (tx, rx) = mpsc::channel(1);
        let event_loop = BridgeHeadEventLoop::new(rx).await;
        self.event_loop_tx = tx;
        tokio::spawn(event_loop.run_loop());
        self.loop_running = true;
        for listener in self.proof_listeners_buffer.iter() {
            self.event_loop_tx.send(NoriBridgeEventLoopCommand::AddProofListener { listener: listener.clone() }).await.unwrap();
        }
        self.proof_listeners_buffer.clear();
    }

    pub async fn advance(&mut self) -> Result<()> {
        self.event_loop_tx.send(NoriBridgeEventLoopCommand::Advance).await?;
        Ok(())
    }

    pub async fn add_proof_listener(&mut self, listener: impl EventListener<NoriBridgeHeadProofMessage> + Send + Sync + 'static,) -> Result<()> {
        let boxed_listener: Box<dyn EventListener<NoriBridgeHeadProofMessage> + Send + Sync> = Box::new(listener);
        let wrapped_listener = Arc::new(Mutex::new(boxed_listener));
        if !self.loop_running {
            self.proof_listeners_buffer.push(wrapped_listener);
        }
        else {
            self.event_loop_tx.send(NoriBridgeEventLoopCommand::AddProofListener { listener: wrapped_listener }).await?;
        }
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        self.event_loop_tx.send(NoriBridgeEventLoopCommand::Shutdown).await?;
        process::exit(1);
        Ok(())
    }
}

// Todo add advance and add event listener to this interface