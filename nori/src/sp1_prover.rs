use crate::{
    contract::bindings::{
        addresses_to_storage_slots, get_source_contract_address, NoriStateBridge,
    },
    rpcs::{
        //consensus::{get_checkpoint, get_client, get_store_with_next_sync_committee, get_updates},
        execution::http::ExecutionHttpProxy,
    },
};
use alloy::eips::BlockId;
use alloy_primitives::FixedBytes;
use anyhow::Result;
use helios_consensus_core::{
    apply_update,
    consensus_spec::{ConsensusSpec, MainnetConsensusSpec},
    types::{FinalityUpdate, LightClientStore},
    verify_update,
};
use helios_ethereum::rpc::ConsensusRpc;
use log::info;
use nori_sp1_helios_primitives::types::ProofInputs;
use sp1_sdk::{ProverClient, SP1ProofWithPublicValues, SP1ProvingKey, SP1Stdin};
use std::sync::OnceLock;

// Import nori sp1 helios program
pub const ELF: &[u8] = include_bytes!("../../nori-elf/nori-sp1-helios-program");

// Cache the proving key globally (initialized once)
static PROVING_KEY: OnceLock<SP1ProvingKey> = OnceLock::new();

pub async fn get_proving_key() -> &'static SP1ProvingKey {
    PROVING_KEY.get_or_init(|| {
        // Initialize fresh client just for setup
        let client = ProverClient::from_env();
        let (pk, _) = client.setup(ELF);
        pk
    })
}

// Struct for ProverJobOutput
pub struct ProverJobOutput {
    job_id: u64,
    input_head: u64,
    proof: SP1ProofWithPublicValues,
}

impl ProverJobOutput {
    pub fn input_head(&self) -> u64 {
        self.input_head
    }

    pub fn proof(&self) -> SP1ProofWithPublicValues {
        self.proof.clone()
    }

    pub fn job_id(&self) -> u64 {
        self.job_id
    }
}

/// Generates a ZK proof for a finality update at the given slot
///
/// # Arguments
/// * `job_id` - The identifier for this job
/// * `input_head` - Target slot number to prove from up until current finality head
/// * `store_hash` - The previous hash of the helio client store state at the `input_head` slot
pub async fn finality_update_job(
    job_id: u64,
    input_head: u64,
    inputs: ProofInputs<MainnetConsensusSpec>,
    //store_hash: FixedBytes<32>,
    //finality_update: FinalityUpdate<MainnetConsensusSpec>,
) -> Result<ProverJobOutput> {
    // Prepare mpt proof inputs
    let finalized_input_block_number = *inputs.store
        .finalized_header
        .execution()
        .map_err(|_| {
            anyhow::Error::msg("Failed to get input finalized execution header".to_string())
        })?
        .block_number();

    // Need to validate that we will advance!

    let finalized_output_block_number = *inputs.finality_update
        .finalized_header()
        .execution()
        .map_err(|_| {
            anyhow::Error::msg("Failed to get output finalized execution header".to_string())
        })?
        .block_number();

    // Now get contract events...

    let proxy = ExecutionHttpProxy::try_from_env();

    let contract_events = proxy
        .get_source_contract_events::<NoriStateBridge::TokensLocked>(
            finalized_input_block_number,
            finalized_output_block_number,
        )
        .await?;

    let storage_address_slots_map = addresses_to_storage_slots(contract_events)?;

    for (address, storage_slot) in storage_address_slots_map.iter() {
        println!(
            "Storage slots obtained address '{:?}' storage_slot '{:?}'",
            address, storage_slot
        );
    }

    // Get mpt proof
    let mpt_account_proof = proxy
        .get_proof(
            get_source_contract_address()?,
            storage_address_slots_map.values().cloned().collect(),
            BlockId::number(finalized_output_block_number),
        )
        .await?;

    info!(
        "mpt_account_proof {:?}",
        serde_json::to_string(&mpt_account_proof)
    );

    // Encode proof inputs
    info!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs = serde_cbor::to_vec(&inputs)?;
    info!("Encoded sp1 proof inputs.");

    /*let encoded_proof_inputs =
    prepare_zk_program_input(input_head, store_hash, finality_update).await?;*/

    // Get proving key
    let pk = get_proving_key().await;

    let proof: SP1ProofWithPublicValues =
        tokio::task::spawn_blocking(move || -> Result<SP1ProofWithPublicValues> {
            // Setup prover client
            info!("Setting up prover client");
            let mut stdin = SP1Stdin::new();
            stdin.write_slice(&encoded_proof_inputs);
            let prover_client = ProverClient::from_env();
            info!("Prover client setup complete.");

            // Generate proof.
            info!("Running sp1 proof.");
            let proof = prover_client.prove(pk, &stdin).plonk().run();
            info!("Finished sp1 proof.");

            proof
        })
        .await??; // Await the blocking task and propagate errors properly

    Ok(ProverJobOutput {
        proof,
        input_head,
        job_id,
    })
}
