use std::process;
use anyhow::Result;
use log::{info,error};
use sp1_sdk::SP1ProofWithPublicValues;
use std::{env, fs, path::Path};
use crate::bridge_head::api::ProofMessage;

/// Panic the entire process with a message.
/// Use this when an error is unrecoverable and the entire application should restart.
/// This ensures we don't enter a zombie state where actors are dead but main process continues.
pub fn panic_more(message: &str) -> ! {
    error!("FATAL: {}", message);
    error!("Terminating entire process");
    process::exit(1);
}

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
    let nori_log = env::var("NORI_LOG").unwrap_or_default();
    let log_level = if nori_log.contains("debug") || nori_log.contains("trace") {
        nori_log.clone()
    } else if nori_log.is_empty() {
        "alloy_transport_http=off".to_string()
    } else {
        format!("{},alloy_transport_http=off", nori_log)
    };
    env::set_var("RUST_LOG", log_level);
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
