use super::sp1_prover_config::{
    FulfillmentConfig, LocalProverMode, ProofType, ProverConfig, ProverMode, get_fulfillment_strategy
};
use anyhow::Result;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use log::info;
use nori_sp1_helios_primitives::types::ProofInputs;
use sp1_sdk::{Prover, ProverClient, SP1ProofWithPublicValues, SP1ProvingKey, SP1Stdin};
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
            //TODO why ?
            LocalProver::Mock(p) | LocalProver::Cpu(p) => {
                let req = p.prove(pk, stdin);
                match proof_type {
                    ProofType::Plonk => req.plonk().run(),
                    ProofType::Groth16 => req.groth16().run(),
                }
            }
            LocalProver::Cuda(p) => {
                let request = p.prove(pk, stdin);
                match proof_type {
                    ProofType::Plonk => request.plonk().run(),
                    ProofType::Groth16 => request.groth16().run(),
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

            // Chain the remaining defaults and run
            let proof_complete_request = proof_request
                // .max_price_per_pgu(net.max_price_per_pgu)
                // .max_price_per_pgu(2) // Max price per bPGU: 2000000000 (0.0000 $PROVE)
                //                       // Max price per bPGU: 2000000000000000000 (2.0000 $PROVE)
                // .cycle_limit(net.cycle_limit)
                // .gas_limit(net.gas_limit)
                //├─ Cycle limit: 1000000000000 cycles
                //└─ Gas limit: 1000000000 PGUs
                //^that only should be set with simulation off? TODO
                //without cycle_limit and gas_limit
                //├─ Cycle limit: 63528590 cycles
                //└─ Gas limit: 136583071 PGUs
                .skip_simulation(net.skip_simulation)
                .timeout(net.timeout) //Timeout: 14400 seconds
                //without timeout(default)//Timeout: 500 seconds
                .whitelist(net.whitelist.clone());
            info!("Prover client setup complete.");

            info!("Running sp1 proof.");
            let proof = proof_complete_request.run();
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

    // Clone the config and bump ref count
    let config = Arc::clone(&config);

    let proof: SP1ProofWithPublicValues =
        tokio::task::spawn_blocking(move || -> Result<SP1ProofWithPublicValues> {
            // Setup stdin
            let mut stdin = SP1Stdin::new();
            stdin.write_slice(&encoded_proof_inputs);

            // Generate proof with configured prover
            generate_proof(&config, pk, &stdin)
        })
        .await??; // Await the blocking task and propagate errors properly

    Ok(ProverJobOutput {
        proof,
        input_head,
        job_id,
    })
}
