use alloy_primitives::FixedBytes;
use anyhow::Result;
use log::info;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path};

const NB_CHECKPOINT_FILE: &str = "./checkpoint/nb_checkpoint.json";

#[derive(Serialize, Deserialize)]
pub struct NoriBridgeCheckpoint {
    pub slot_head: u64,
    pub store_hash: FixedBytes<32>
}

// Static method to check if the checkpoint file exists
pub fn nb_checkpoint_exists() -> bool {
    Path::new(NB_CHECKPOINT_FILE).exists()
}

// Static method to load the checkpoint from file
pub fn load_nb_checkpoint() -> Result<NoriBridgeCheckpoint> {
    // Open the checkpoint file
    let mut file = File::open(NB_CHECKPOINT_FILE).expect("Failed to open nori checkpoint file.");

    // Read the contents into a Vec<u8>
    let mut serialized_checkpoint = Vec::new();
    file.read_to_end(&mut serialized_checkpoint)
        .expect("Failed to read nori checkpoint file.");

    // Deserialize the checkpoint data using serde_json (not serde_cbor)
    let nb_checkpoint: NoriBridgeCheckpoint = serde_json::from_slice(&serialized_checkpoint)
        .expect("Failed to deserialize nori checkpoint.");

    Ok(nb_checkpoint)
}

pub fn save_nb_checkpoint(slot_head: u64, store_hash: FixedBytes<32>) {
    info!("Saving checkpoint.");
    
    // Create dir if nessesary
    let checkpoint_dir = Path::new(NB_CHECKPOINT_FILE).parent().unwrap();

    // Ensure the directory exists
    std::fs::create_dir_all(checkpoint_dir).expect("Failed to create checkpoint directory.");

    // Define the current checkpoint
    let checkpoint = NoriBridgeCheckpoint {
        slot_head,
        store_hash
    };

    // Serialize the checkpoint to a byte vector
    let serialized_nb_checkpoint =
        serde_json::to_string(&checkpoint).expect("Failed to serialize nori checkpoint.");

    // Write the serialized data to the file specified by `checkpoint_location`
    std::fs::write(NB_CHECKPOINT_FILE, &serialized_nb_checkpoint)
        .map_err(|e| anyhow::anyhow!("Failed to write to checkpoint file: {}", e))
        .unwrap();

    info!("Nori bridge checkpoint saved successfully.");
}
