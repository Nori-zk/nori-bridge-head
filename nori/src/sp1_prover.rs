use crate::{
    contract::bindings::{
        addresses_to_storage_slots, get_source_contract_address, NoriStateBridge,
    },
    rpcs::{consensus::{get_checkpoint, get_client, get_store_with_next_sync_committee, get_updates}, execution::http::ExecutionHttpProxy},
};
use alloy::eips::BlockId;
use alloy_primitives::FixedBytes;
use anyhow::Result;
use helios_consensus_core::{
    apply_update,
    consensus_spec::MainnetConsensusSpec,
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

/// Encodes a prepared helio store ready for a sp1 helio slot transition proof
/// # Arguments
/// * `input_head` - Target slot number to prove from up until current finality head
/// * `last_next_sync_committee` -  The previous hash of next_sync_committee
/// * `store_hash` - The previous hash of the helio client store state at the `input_head` slot
pub async fn prepare_zk_program_input(
    input_slot: u64,
    store_hash: FixedBytes<32>,
    finality_update: FinalityUpdate<MainnetConsensusSpec>,
) -> Result<Vec<u8>> {
    // Get latest beacon checkpoint
    let helios_checkpoint = get_checkpoint(input_slot).await?;

    // Re init helios client and extract references
    let mut helios_update_client = get_client(helios_checkpoint).await?;
    let genesis_root = &helios_update_client.config.chain.genesis_root;
    let forks = &helios_update_client.config.forks;

    // Get finality update
    info!("Getting finality update from input slot {}", input_slot);
    /*let finality_update = helios_update_client
    .rpc
    .get_finality_update()
    .await
    .map_err(|e| anyhow::anyhow!("Failed to fetch finality update via RPC: {}", e))?;*/

    // Try and throw early if our finality_update beacon slot is not ahead of our input slot. As doing the proof would not yield any progress.
    // In operator/strategy make sure to prevent this.
    // It would be nice to stage a proof for a little bit of time later but not sure how to handle this quite yet.
    /*let latest_slot = finality_update.finalized_header().beacon().slot;
    if latest_slot <= input_slot {
        return Err(anyhow::anyhow!(
            "Finality update beacon slot {} is not ahead of our input slot {}",
            latest_slot,
            input_slot
        ));
    }*/ // Disabled so we can get vk's

    // Get sync commitee updates
    info!("Getting sync commitee updates.");
    let mut updates = get_updates(&helios_update_client).await?;

    // Panic if our updates were empty (not sure how to deal with this yet)
    if updates.is_empty() {
        return Err(anyhow::anyhow!("Error updates were missing 0th update."));
    }

    // Get the expected current slot from the client
    let expected_current_slot = helios_update_client.expected_current_slot();

    // Get a synced bootstrapped store.
    let store: LightClientStore<MainnetConsensusSpec> = {
        // Get a reference to the first update
        let first_update = &updates[0];
        // Get the finalized_header beacon slot of the first update
        let first_update_slot = first_update.finalized_header().beacon().slot;

        // If the 0th updates finalized_header beacon slot is lower than our input_head then we can apply
        // this update to our bootstrapped client to bring it in sync with the input_head. This puts the
        // next_sync_committee onto our store as well as other state and bring the store up to date
        // with its state restored back to the same condition as the terminal ("updated") store state in
        // the last zk programs invocation
        if first_update_slot < input_slot {
            // FIXME should this be <= ?
            info!("First update finalized header beacon slot is less than our input head.");
            // Remove the zeroth update from the updates.
            let first_update = updates.remove(0);
            // Validate the update.
            verify_update(
                &first_update,
                expected_current_slot,
                &helios_update_client.store,
                *genesis_root,
                forks,
            )
            .map_err(|e| anyhow::anyhow!("Verify update failed: {}", e))?;
            // Apply the update to the client... mutating its store
            apply_update(&mut helios_update_client.store, &first_update);
            helios_update_client.store
        } else {
            info!("First update finalized header beacon slot is greater than or equal to our input head.");
            // The 0th update is ahead of the input_head, the next_sync_committee and other store state
            // need to be restored without advancing the store in terms of slot.
            // This can happen if either we are exactly on a period transition (every 32*256 slots) or
            // if our client is very behind due to being offline while a period transition (or more than one)
            // has occured.
            get_store_with_next_sync_committee(
                expected_current_slot,
                helios_update_client.store,
                genesis_root,
                forks,
                first_update,
            )?
        }
    };

    let finalized_input_block_number = *store
        .finalized_header
        .execution()
        .map_err(|_| {
            anyhow::Error::msg("Failed to get input finalized execution header".to_string())
        })?
        .block_number();

    // Need to validate that we will advance!

    let finalized_output_block_number = *finality_update
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
    let mpt_account_proof = proxy.get_proof(
        get_source_contract_address()?,
        storage_address_slots_map.values().cloned().collect(),
        BlockId::number(finalized_output_block_number),
    ).await?;

    info!("mpt_account_proof {:?}", serde_json::to_string(&mpt_account_proof));

    // Create program inputs
    info!("Building sp1 proof inputs.");
    let inputs = ProofInputs {
        updates,
        finality_update,
        expected_current_slot,
        store,
        genesis_root: *genesis_root,
        forks: forks.clone(),
        store_hash,
    };
    info!("Built sp1 proof inputs.");

    // Encode proof inputs
    info!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs = serde_cbor::to_vec(&inputs)?;
    info!("Encoded sp1 proof inputs.");
    Ok(encoded_proof_inputs)
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
    store_hash: FixedBytes<32>,
    finality_update: FinalityUpdate<MainnetConsensusSpec>,
) -> Result<ProverJobOutput> {
    // Encode proof inputs
    info!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs =
        prepare_zk_program_input(input_head, store_hash, finality_update).await?;

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
