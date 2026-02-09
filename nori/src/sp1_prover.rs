use crate::sp1_prover_config::try_get_fallback_whitelist;

use super::sp1_prover_config::{
    get_fallback_whitelist, get_fulfillment_strategy, FulfillmentConfig, LocalProverMode,
    ProofType, ProverConfig, ProverMode,
};
use alloy_primitives::Address;
use anyhow::Result;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use log::info;
use nori_sp1_helios_primitives::types::ProofInputs;
use sp1_sdk::{Prover, ProverClient, SP1ProofWithPublicValues, SP1ProvingKey, SP1Stdin};
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

/// Import nori sp1 helios program
pub const ELF: &[u8] = include_bytes!("../../nori-elf/nori-sp1-helios-program");

/// Cache the proving key globally (initialized once)
static PROVING_KEY: OnceLock<SP1ProvingKey> = OnceLock::new();

/// Method to get proving key init or return it if it has been called already
pub async fn get_proving_key() -> &'static SP1ProvingKey {
    PROVING_KEY.get_or_init(|| {
        // Initialize prover client for setup (uses mock mode for fast setup)
        let client = ProverClient::builder().mock().build();
        let (pk, _) = client.setup(ELF);
        pk
    })
}

// ================================================================================================
// Local prover
// ================================================================================================

/// Enum to hold different prover types
enum LocalProver {
    Mock(sp1_sdk::CpuProver),
    Cpu(sp1_sdk::CpuProver),
    Cuda(sp1_sdk::CudaProver),
}

impl LocalProver {
    /// Generate proof with the given proof type
    fn prove_with_type(
        self,
        pk: &SP1ProvingKey,
        stdin: &SP1Stdin,
        proof_type: &ProofType,
    ) -> Result<SP1ProofWithPublicValues> {
        match self {
            LocalProver::Mock(p) | LocalProver::Cpu(p) => {
                let cpu_prove_builder = p.prove(pk, stdin);
                match proof_type {
                    ProofType::Plonk => cpu_prove_builder.plonk().run(),
                    ProofType::Groth16 => cpu_prove_builder.groth16().run(),
                }
            }
            LocalProver::Cuda(p) => {
                let cuda_prove_builder = p.prove(pk, stdin);
                match proof_type {
                    ProofType::Plonk => cuda_prove_builder.plonk().run(),
                    ProofType::Groth16 => cuda_prove_builder.groth16().run(),
                }
            }
        }
    }
}

// ================================================================================================
// SP1 proof generator
// ================================================================================================

/// Method to generate an SP1 proof given a prover config, key and stdin
fn generate_proof(
    config: &ProverConfig,
    pk: &SP1ProvingKey,
    stdin: &SP1Stdin,
    extended_whitelist: Option<Vec<Address>>,
) -> Result<SP1ProofWithPublicValues> {
    match &config.mode {
        ProverMode::Network(net) => {
            info!("Setting up prover client");
            let prover = ProverClient::builder()
                .network_for(net.network_mode)
                .rpc_url(&net.rpc_url)
                .private_key(&net.private_key)
                .build();

            // Get the SP1 strategy native type
            let strategy = get_fulfillment_strategy(&net.fulfillment);

            // Build the proof request
            let mut proof_request = prover.prove(pk, stdin);

            // Pick the proof type
            proof_request = match config.proof_type {
                ProofType::Plonk => proof_request.plonk(),
                ProofType::Groth16 => proof_request.groth16(),
            };

            // Apply the strategy
            proof_request = proof_request.strategy(strategy);

            // Apply the auction timeout if and only if we are in Auction mode
            if let FulfillmentConfig::Auction { timeout } = net.fulfillment {
                proof_request = proof_request.auction_timeout(timeout);
            }

            // Use the extended whitelist if provided, otherwise use the config's whitelist
            let whitelist = extended_whitelist.or_else(|| net.whitelist.clone());

            // Chain the remaining defaults
            proof_request = proof_request
                // The user can either provide a value (error if invalid) for this via ENV_SP1_MAX_PRICE_PER_PGU
                // OR we will default to (if not set):
                // SDK_DEFAULT_PRICE_PER_PGU: u64 = 1_000_000_000u64 (note this is 1e9 scaling compared to $PROVE)
                // Max price per bPGU: 1000000000000000000 (1.0000 $PROVE)
                .max_price_per_pgu(net.max_price_per_pgu)
                // The user can provide a value (error if invalid) for this via ENV_SP1_SKIP_SIMULATION
                // OR we will default to false (if not set)
                .skip_simulation(net.skip_simulation)
                // The user can provide a value (error if invalid) for this via ENV_SP1_TIMEOUT_SECS
                // OR we will default to (if not set):
                // SDK_DEFAULT_TIMEOUT_SECS: u64 = 600
                .timeout(net.timeout)
                .whitelist(whitelist);

            // cycle_limit and gas_limit are only required when skip_simulation = true.

            // When simulation runs (skip_simulation = false), SP1 calculates these for us:
            //   ├─ Cycle limit: 63_528_590 cycles (example from simulation)
            //   └─ Gas limit: 136_583_071 PGUs (example from simulation)
            // The user can choose to provide values (error if invalid) to constrain them via ENV_SP1_GAS_LIMIT and ENV_SP1_CYCLE_LIMIT
            // or the simulation calculated values will be used (effectively use whatever is nessesary).

            // When skip_simulation = true, we must provide them
            // Either the user provides values (error if invalid) via ENV_SP1_GAS_LIMIT and ENV_SP1_CYCLE_LIMIT
            // OR we default to certain values (if not set):
            // For gas_limit we always default to SDK_DEFAULT_GAS_LIMIT:
            //   └─ Gas limit: SDK_DEFAULT_GAS_LIMIT: u64 = 1_000_000_000 PGUs
            // For cycle_limit what we default to depends on the choice of ENV_SP1_NETWORK_MODE:
            // If ENV_SP1_NETWORK_MODE is 'mainnet':
            //   └─ Cycle limit: SDK_MAINNET_DEFAULT_CYCLE_LIMIT: u64 = 1_000_000_000_000 cycles
            // Else if ENV_SP1_NETWORK_MODE is 'reserved':
            //   └─ Cycle limit: SDK_RESERVED_DEFAULT_CYCLE_LIMIT: u64 = 100_000_000 cycles


            if let Some(cycle_limit) = net.cycle_limit {
                proof_request = proof_request.cycle_limit(cycle_limit);
            }
            if let Some(gas_limit) = net.gas_limit {
                proof_request = proof_request.gas_limit(gas_limit);
            }

            info!("Prover client setup complete.");

            info!("Running sp1 proof.");
            let proof = proof_request.run();
            info!("Finished sp1 proof.");

            proof
        }
        ProverMode::Local(local_mode) => {
            info!("Setting up prover client");
            let prover = match local_mode {
                LocalProverMode::Mock => LocalProver::Mock(ProverClient::builder().mock().build()),
                LocalProverMode::Cpu => LocalProver::Cpu(ProverClient::builder().cpu().build()),
                LocalProverMode::Cuda => LocalProver::Cuda(ProverClient::builder().cuda().build()),
            };
            info!("Prover client setup complete.");

            // Generate proof with the configured proof type
            info!("Running sp1 proof.");
            let proof = prover.prove_with_type(pk, stdin, &config.proof_type);
            info!("Finished sp1 proof.");

            proof
        }
    }
}

// ================================================================================================
// Bridge head SP1 proof worker
// ================================================================================================

/// Struct for ProverJobOutput
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
/// * `inputs` - Input to the zk program for the finality transition job
pub async fn finality_update_job(
    config: Arc<ProverConfig>,
    job_id: u64,
    input_head: u64,
    inputs: ProofInputs<MainnetConsensusSpec>,
) -> Result<ProverJobOutput> {
    info!(
        "Contract storage {:?}",
        serde_json::to_string(&inputs.contract_storage)
    );

    // Encode proof inputs
    info!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs = serde_cbor::to_vec(&inputs)?;
    info!("Encoded sp1 proof inputs.");

    // Get proving key
    let pk = get_proving_key().await;

    // Fetch and extend whitelist if needed (before spawn_blocking since it's async)
    let extended_whitelist = match &config.mode {
        ProverMode::Network(net) if net.whitelist_add_high_availability => {
            info!("Fetching high-availability provers to extend whitelist...");
            match try_get_fallback_whitelist(net).await {
                Ok(fallback) => {
                    info!("Successfully fetched {} high-availability provers.", fallback.len());

                    // Extend the existing whitelist with fallback provers (deduplicated, order preserved)
                    let mut extended = net.whitelist.clone().unwrap_or_default();
                    let mut seen: HashSet<Address> = extended.iter().copied().collect();

                    let mut added_count = 0;
                    for addr in fallback {
                        if seen.insert(addr) {
                            extended.push(addr);
                            added_count += 1;
                        }
                    }

                    info!(
                        "Whitelist extended with {} new provers (total: {}):",
                        added_count,
                        extended.len()
                    );
                    for addr in &extended {
                        info!("  - {}", addr);
                    }
                    Some(extended)
                }
                Err(e) => {
                    info!("Failed to fetch high-availability provers: {}. Using configured whitelist only.", e);
                    net.whitelist.clone()
                }
            }
        }
        _ => None,
    };

    // Clone the config and bump ref count
    let config = Arc::clone(&config);

    let proof: SP1ProofWithPublicValues =
        tokio::task::spawn_blocking(move || -> Result<SP1ProofWithPublicValues> {
            // Setup stdin
            let mut stdin = SP1Stdin::new();
            stdin.write_slice(&encoded_proof_inputs);

            // Generate proof with configured prover
            generate_proof(&config, pk, &stdin, extended_whitelist)
        })
        .await??; // Await the blocking task and propagate errors properly

    Ok(ProverJobOutput {
        proof,
        input_head,
        job_id,
    })
}
