use anyhow::Result;
use log::info;
use sp1_sdk::SP1ProofWithPublicValues;
use std::{env, fs, path::Path};

use crate::bridge_head::api::ProofMessage;

pub async fn handle_nori_proof(proof: &SP1ProofWithPublicValues, latest_block: u64) -> Result<()> {
    // Create directory to save the proofs
    let proof_path = format!("./sp1-helios-proofs");
    let proof_dir = Path::new(&proof_path);
    fs::create_dir_all(proof_dir)?;
    let if_mock = if env::var("SP1_PROVER").unwrap_or_default() == "mock" {
        "mock-"
    } else {
        ""
    };
    let filename = format!("{}{}-{}.json", if_mock, latest_block, proof.sp1_version);
    let file_path = proof_dir.join(filename);
    // Save the proof
    std::fs::write(&file_path, serde_json::to_string(&proof).unwrap()).unwrap();
    info!(
        "Proof saved successfully to {}.",
        file_path.to_str().unwrap()
    );
    Ok(())
}

pub async fn handle_nori_proof_message(proof_message: &ProofMessage) -> Result<()> {
    // Create directory to save the proofs
    let proof_path = format!("./sp1-helios-proof-messages");
    let proof_dir = Path::new(&proof_path);
    fs::create_dir_all(proof_dir)?;
    let if_mock = if env::var("SP1_PROVER").unwrap_or_default() == "mock" {
        "mock-"
    } else {
        ""
    };
    let filename = format!("{}{}-{}.json", if_mock, proof_message.input_slot, proof_message.proof.sp1_version);
    let file_path = proof_dir.join(filename);
    // Save the proof
    std::fs::write(&file_path, serde_json::to_string(&proof_message).unwrap()).unwrap();
    info!(
        "Proof saved successfully to {}.",
        file_path.to_str().unwrap()
    );
    Ok(())
}

pub fn enable_logging_from_cargo_run() {
    dotenv::dotenv().ok();
    env::set_var("RUST_LOG", env::var("NORI_LOG").unwrap_or("".to_string()));
    env_logger::init();
}

pub fn enable_logging_from_cargo_run_with_helios_suppression() {
    dotenv::dotenv().ok();
    let nori_log = env::var("NORI_LOG").unwrap_or_default();
    let log_level = if nori_log.is_empty() {
        "helios::consensus=error".to_string()
    } else {
        format!("{},helios::consensus=error", nori_log)
    };
    env::set_var("RUST_LOG", log_level);
    env_logger::init();
}
