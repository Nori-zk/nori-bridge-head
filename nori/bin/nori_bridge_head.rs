use std::sync::Arc;

use anyhow::{Ok, Result};
use async_trait::async_trait;
use log::info;
use nori::{
    bridge_head::{
        NoriBridgeHead, NoriBridgeHeadConfig, NoriBridgeHeadMode, NoriBridgeHeadProofMessage,
    },
    event_dispatcher::EventListener,
    utils::enable_logging_from_cargo_run,
};
use tokio::sync::Mutex;

pub struct ProofListener {
    bridge_head: Arc<Mutex<NoriBridgeHead>>, // Use Arc<Mutex<NoriBridgeHead>> for shared ownership
}

impl ProofListener {
    pub fn new(bridge_head: Arc<Mutex<NoriBridgeHead>>) -> Self {
        Self { bridge_head }
    }
}

#[async_trait]
impl EventListener<NoriBridgeHeadProofMessage> for ProofListener {
    async fn on_event(&mut self, data: NoriBridgeHeadProofMessage) -> Result<()> {
        println!("Got proof message: {}", data.slot);

        // Acquire lock and call advance()
        let mut bridge = self.bridge_head.lock().await; // Correctly await the lock
        bridge.advance().await; // what about using rx and tx to advance the head?

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Enable info logging when using cargo --run
    enable_logging_from_cargo_run();

    // Create NoriBridgeHead configuration
    let config = NoriBridgeHeadConfig::new(NoriBridgeHeadMode::Finality);
    let bridge_head = Arc::new(Mutex::new(NoriBridgeHead::new(config).await));

    // Create the ProofListener
    let proof_listener = ProofListener::new(bridge_head.clone());

    // Add the ProofListener as a boxed listener
    bridge_head.lock().await.add_proof_listener(Box::new(proof_listener));

    // Start the bridge head
    bridge_head.lock().await.run().await; // think this will stop any consumer running wont it...! how could we tell the bridge head to advance??

    /*
        Need to think about this the run routine will block the main thread whatever we do unless we do something like the job manager aka we allow it to

        what really needs to happen here

        we have the message system which need to run actively it can consume messages for which we need to do things with our nori_bridge like tell it to advance!

        we need to run our run loop in a spawned thread in order to not block the main thread. in which case all the state it touches needs to be threadsafe

        main thread for message consumption to call advance!
        polling worker -> with threadsafe state
        job system for worker tasks.... which are managed by the polling worker

        what state needs mutex's on it? 

        run:
        next_head (read)
        current_head (read)
        auto_advance_index
        job_idx (read)
        auto_advance_index (read)

        advance:
        job_idx (read write)
        next_head (read)
        current_head (read)
        auto_advance_index (read)

        and both call...
        attempt_finality_update
        ....which has
        working_head (read / write)
        next_sync_committee (read / write)
        current_head (read / write)
        auto_advance_index (read / write)
        current_sync_commitee (read/write but we can remove this!)

        will also need to send the proof message to the main thread in order to invoke the dispatcher.... 
        
        so we will need need job engines for the proof worker, proof dispatcher and notice dispatcher....

        main loop thread will need job engine,
        main thread will need proof dispatcher engine/notice dispatcher engine??



     */

    Ok(())
}

// 9b6fcc43-5166-4349-8b5c-49f96993b882