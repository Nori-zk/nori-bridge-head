use anyhow::Result;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use helios_ethereum::rpc::http_rpc::HttpRpc;
use nori::rpcs::consensus::ConsensusHttpProxy;
use std::path::PathBuf;

const SLOT_DURATION_SECS: u64 = 12;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let consensus_client = ConsensusHttpProxy::<MainnetConsensusSpec, HttpRpc>::try_from_env();

    // Cold start once
    println!("Cold starting...");
    let (mut input_slot, mut store_hash) = loop {
        match consensus_client.get_latest_finality_slot_and_store_hash().await {
            Ok(v) => break v,
            Err(e) => {
                println!("Cold start failed: {}. Retrying in {} seconds...", e, SLOT_DURATION_SECS);
                tokio::time::sleep(std::time::Duration::from_secs(SLOT_DURATION_SECS)).await;
            }
        }
    };
    println!("Cold start slot: {}, store hash: {}", input_slot, store_hash);

    // Chain updates until we get a non checkpoint output slot
    loop {
        println!("Preparing proof inputs from slot {}...", input_slot);
        let proof_inputs_with_window = match consensus_client
            .prepare_consensus_mpt_proof_inputs(input_slot, store_hash, false)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                println!("Proof input preparation failed: {}. Retrying in {} seconds...", e, SLOT_DURATION_SECS);
                tokio::time::sleep(std::time::Duration::from_secs(SLOT_DURATION_SECS)).await;
                continue;
            }
        };

        let output_slot = proof_inputs_with_window.expected_output_slot;
        let output_store_hash = proof_inputs_with_window.expected_output_store_hash;
        println!("Output slot: {} (% 32 == {})", output_slot, output_slot % 32);

        if output_slot % 32 != 0 {
            println!("Non checkpoint slot found. Generating fixture.");

            let encoded = serde_cbor::to_vec(&proof_inputs_with_window.proof_inputs)?;

            let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("nori")
                .join("tests")
                .join("data");
            std::fs::create_dir_all(&fixture_dir)?;

            let fixture_path = fixture_dir.join(format!(
                "non_checkpoint_proof_inputs.{}.cbor",
                output_slot
            ));
            std::fs::write(&fixture_path, &encoded)?;
            println!("Fixture written to {}", fixture_path.display());
            println!("Size: {} bytes", encoded.len());

            return Ok(());
        }

        // Chain: use this output as the next input
        input_slot = output_slot;
        store_hash = output_store_hash;

        // Wait for finality to advance
        println!("Checkpoint slot. Waiting {} seconds for finality to advance...", SLOT_DURATION_SECS);
        tokio::time::sleep(std::time::Duration::from_secs(SLOT_DURATION_SECS)).await;
    }
}
