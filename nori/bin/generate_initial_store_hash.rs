use alloy_primitives::FixedBytes;
use anyhow::anyhow;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use helios_ethereum::rpc::http_rpc::HttpRpc;
use log::{info, warn};
// use noi_sp1_helios_primitives::types::{ConsensusProofInputs, ProofInputsWithWindow};
use nori::rpcs::consensus::Client;
use nori_hash::sha256_hash::sha256_hash_helios_store;
use reqwest::Url;

use std::env;

const CONSENSUS_RPCS_ENV_VAR: &str = "NORI_SOURCE_CONSENSUS_HTTP_RPCS";

pub async fn generate_inital_store_hash_from_input_slot(input_slot: u64) -> FixedBytes<32> {
    dotenv::dotenv().ok();

    let urls: Vec<Url> = env::var(CONSENSUS_RPCS_ENV_VAR)
        .expect("NORI_SOURCE_CONSENSUS_HTTP_RPCS not set")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| match s.parse::<Url>() {
            Ok(u) => Some(u),
            Err(e) => {
                warn!("Skipping invalid URL '{}': {}", s, e);
                None
            }
        })
        .collect();

    let principal_provider_url = urls
        .first()
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "No valid consensus RPC URLs found in {}",
                CONSENSUS_RPCS_ENV_VAR
            )
        })
        .unwrap();

    let client = Client::<MainnetConsensusSpec, HttpRpc>::bootstrap_from_slot(
        &principal_provider_url,
        input_slot,
    )
    .await
    .unwrap();

    let synced_store = &client.get_inner_client().store;

    // Get the store hash
    info!("Calculating cold start store hash");
    let store_hash = sha256_hash_helios_store(synced_store).expect("Failed to hash the store");
    info!("Calculated cold start store hash: {}", store_hash);
    store_hash

    // let mut client: Client<S, R> = Client::bootstrap_from_slot( input_slot).await?;

    //  let mut client: Client<S, R> =
    //         Client::bootstrap_from_slot(consensus_client., input_slot).await?;

    // consensus_client.prepare_consensus_mpt_proof_inputs(input_slot, store_hash, validate)
}

// read param from cmd and pass as input slot

#[tokio::main]
async fn main() {
    let input_slot = std::env::args()
        .nth(1)
        .expect("Please provide an input slot as the first argument")
        .parse::<u64>()
        .expect("Failed to parse input slot as u64");
    let store_hash = generate_inital_store_hash_from_input_slot(input_slot).await;
    println!("Initial store hash for slot {}: {}", input_slot, store_hash);
}
