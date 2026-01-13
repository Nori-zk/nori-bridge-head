use alloy_primitives::Address;
use anyhow::Result;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use log::info;
use nori_sp1_helios_primitives::types::ProofInputs;
use sp1_sdk::{
    network::{proto::types::FulfillmentStrategy, NetworkMode},
    Prover, ProverClient, SP1ProofWithPublicValues, SP1ProvingKey, SP1Stdin,
};
use std::{env, str::FromStr, sync::{Arc, OnceLock}, time::Duration};

// Import nori sp1 helios program
pub const ELF: &[u8] = include_bytes!("../../nori-elf/nori-sp1-helios-program");

// Environment variable names for SP1 prover configuration
// SDK-defined environment variables:
// Reference: sp1-sdk-5.2.2/src/env/mod.rs:39-42,45 (EnvProver::new)
const ENV_SP1_PROVER: &str = "SP1_PROVER"; // Required: validated in nori/src/bridge_head/api.rs:123
// Reference: sp1-sdk-5.2.2/src/network/builder.rs:32,50,166,178 (NetworkProverBuilder)
const ENV_NETWORK_PRIVATE_KEY: &str = "NETWORK_PRIVATE_KEY"; // Required when SP1_PROVER=network (line 166)
const ENV_NETWORK_RPC_URL: &str = "NETWORK_RPC_URL"; // Optional with SDK defaults (line 178-179)

// Nori-specific environment variables (not defined in SDK, uses builder methods instead):
// These map to SDK builder methods: .network_for(), .strategy(), .gas_limit(), etc.
const ENV_SP1_NETWORK_MODE: &str = "SP1_NETWORK_MODE"; // Maps to NetworkProverBuilder.network_for()
const ENV_SP1_FULFILLMENT_STRATEGY: &str = "SP1_FULFILLMENT_STRATEGY"; // Maps to ProveRequest.strategy()
const ENV_SP1_PROOF_TYPE: &str = "SP1_PROOF_TYPE"; // Maps to .groth16() or .plonk()
const ENV_SP1_CYCLE_LIMIT: &str = "SP1_CYCLE_LIMIT"; // Maps to ProveRequest.cycle_limit()
const ENV_SP1_GAS_LIMIT: &str = "SP1_GAS_LIMIT"; // Maps to ProveRequest.gas_limit()
// Reference: sp1-sdk-5.2.2/src/network/prove.rs:655-659 (deprecated SDK env var, warns to use method)
const ENV_SKIP_SIMULATION: &str = "SKIP_SIMULATION"; // Deprecated SDK var, maps to ProveRequest.skip_simulation()
const ENV_SP1_TIMEOUT_SECS: &str = "SP1_TIMEOUT_SECS"; // Maps to ProveRequest.timeout()
const ENV_SP1_AUCTION_TIMEOUT_SECS: &str = "SP1_AUCTION_TIMEOUT_SECS"; // Maps to auction timeout
const ENV_SP1_WHITELIST: &str = "SP1_WHITELIST"; // Maps to ProveRequest.whitelist()

// Default values from sp1-sdk-5.2.2
// Reference: sp1-sdk-5.2.2/src/network/mod.rs
const SDK_MAINNET_RPC_URL: &str = "https://rpc.mainnet.succinct.xyz"; // Line 67
const SDK_RESERVED_RPC_URL: &str = "https://rpc.production.succinct.xyz"; // Line 69
const SDK_DEFAULT_AUCTION_TIMEOUT_SECS: u64 = 30; // Line 76: Duration::from_secs(30)
const SDK_MAINNET_DEFAULT_CYCLE_LIMIT: u64 = 1_000_000_000_000; // Line 77
const SDK_RESERVED_DEFAULT_CYCLE_LIMIT: u64 = 100_000_000; // Line 78
const SDK_DEFAULT_GAS_LIMIT: u64 = 1_000_000_000; // Line 79
const SDK_DEFAULT_TIMEOUT_SECS: u64 = 14400; // Line 80
// Reference: sp1-sdk-5.2.2/src/network/prover.rs:174
const SDK_DEFAULT_SKIP_SIMULATION: bool = false;

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

// Configuration for SP1 prover types

enum ProverMode {
    Local(LocalProverMode),
    Network(NetworkConfig),
}

#[derive(Debug)]
enum LocalProverMode {
    Mock,
    Cpu,
    Cuda,
}

pub enum ProofType {
    Plonk,
    Groth16,
}

pub enum FulfillmentConfig {
    Auction { timeout: Duration },
    Hosted,
    Reserved,
}

pub struct NetworkConfig {
    pub network_mode: NetworkMode,
    pub private_key: String,
    pub rpc_url: String,
    pub fulfillment: FulfillmentConfig,
    pub cycle_limit: u64,
    pub gas_limit: u64,
    pub skip_simulation: bool,
    pub timeout: Duration,
    pub whitelist: Option<Vec<Address>>,
}

pub struct ProverConfig {
    mode: ProverMode,
    proof_type: ProofType,
}

/// Configuration for SP1 prover
impl ProverConfig {
    /// Mock prover with a caller-chosen proof type.
    pub fn mock(proof_type: ProofType) -> Self {
        ProverConfig {
            mode: ProverMode::Local(LocalProverMode::Mock),
            proof_type,
        }
    }

    /// Mock + Groth16
    pub fn mock_groth16() -> Self {
        Self::mock(ProofType::Groth16)
    }

    /// Mock + Plonk
    pub fn mock_plonk() -> Self {
        Self::mock(ProofType::Plonk)
    }

    /// Load and validate configuration from environment
    pub fn from_env() -> Result<Self> {
        let sp1_prover = env::var(ENV_SP1_PROVER).unwrap_or_else(|_| "cpu".to_string());

        // Get proof type from environment, defaults to groth16, error on invalid value
        let proof_type = match env::var(ENV_SP1_PROOF_TYPE).ok().as_deref() {
            Some("plonk") => ProofType::Plonk,
            Some("groth16") => ProofType::Groth16,
            Some(other) => return Err(anyhow::anyhow!("Invalid {} value: '{}'. Expected 'plonk' or 'groth16'", ENV_SP1_PROOF_TYPE, other)),
            None => return Err(anyhow::anyhow!("Invalid {} value: None. Expected 'plonk' or 'groth16'", ENV_SP1_PROOF_TYPE)),
        };

        let mode = match sp1_prover.as_str() {
            "mock" => ProverMode::Local(LocalProverMode::Mock),
            "cpu" => ProverMode::Local(LocalProverMode::Cpu),
            "cuda" => ProverMode::Local(LocalProverMode::Cuda),
            "network" => {
                // Get network private key from environment if set
                // Reference: sp1-sdk-5.2.2/src/network/builder.rs:32,166
                let network_mode = env::var(ENV_SP1_NETWORK_MODE)
                    .ok()
                    .and_then(|s| s.parse::<NetworkMode>().ok()) // Uses FromStr impl at mod.rs:54-63
                    .unwrap_or(NetworkMode::Mainnet); // Default when reserved-capacity feature not enabled

                let private_key = env::var(ENV_NETWORK_PRIVATE_KEY)
                    .map_err(|_| anyhow::anyhow!("{} required for network mode", ENV_NETWORK_PRIVATE_KEY))?;

                // Get RPC URL from environment or use default based on network mode
                // Reference: sp1-sdk-5.2.2/src/network/mod.rs:67,69
                let rpc_url = env::var(ENV_NETWORK_RPC_URL).ok().unwrap_or_else(|| {
                    match network_mode {
                        NetworkMode::Mainnet => SDK_MAINNET_RPC_URL.to_string(),
                        NetworkMode::Reserved => SDK_RESERVED_RPC_URL.to_string(),
                    }
                });

                // Get fulfillment strategy from environment
                // Defaults based on network mode: Auction for Mainnet, Hosted for Reserved
                // Reference: sp1-sdk-5.2.2/src/network/prover.rs:98-102 (default_fulfillment_strategy)
                let fulfillment = {
                    let strategy_str = env::var(ENV_SP1_FULFILLMENT_STRATEGY).ok();
                    
                    match strategy_str.as_deref() {
                        Some("auction") => {
                            let secs = env::var(ENV_SP1_AUCTION_TIMEOUT_SECS)
                                .ok()
                                .and_then(|s| s.parse::<u64>().ok())
                                .unwrap_or(SDK_DEFAULT_AUCTION_TIMEOUT_SECS);
                            FulfillmentConfig::Auction { timeout: Duration::from_secs(secs) }
                        }
                        Some("hosted") => FulfillmentConfig::Hosted,
                        Some("reserved") => FulfillmentConfig::Reserved,
                        None => {
                            // Default based on network_mode
                            match network_mode {
                                NetworkMode::Mainnet => {
                                    let secs = SDK_DEFAULT_AUCTION_TIMEOUT_SECS;
                                    FulfillmentConfig::Auction { timeout: Duration::from_secs(secs) }
                                }
                                NetworkMode::Reserved => FulfillmentConfig::Hosted,
                            }
                        }
                        Some(other) => return Err(anyhow::anyhow!("Invalid strategy: {}", other)),
                    }
                };

                // Get cycle limit from environment, defaults based on network mode
                // Reference: sp1-sdk-5.2.2/src/network/mod.rs:77-78
                let cycle_limit = env::var(ENV_SP1_CYCLE_LIMIT)
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(match network_mode {
                        NetworkMode::Mainnet => SDK_MAINNET_DEFAULT_CYCLE_LIMIT,
                        NetworkMode::Reserved => SDK_RESERVED_DEFAULT_CYCLE_LIMIT,
                    });

                // Get gas limit from environment, defaults to SDK_DEFAULT_GAS_LIMIT
                // Reference: sp1-sdk-5.2.2/src/network/prover.rs:770-803 (get_execution_limits)
                let gas_limit = env::var(ENV_SP1_GAS_LIMIT)
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(SDK_DEFAULT_GAS_LIMIT);

                // Get skip simulation flag from environment, defaults to SDK_DEFAULT_SKIP_SIMULATION (false)
                // Reference: sp1-sdk-5.2.2/src/network/prover.rs:174 (default)
                // Reference: sp1-sdk-5.2.2/src/network/prove.rs:655-659 (deprecated SKIP_SIMULATION env var)
                let skip_simulation = env::var(ENV_SKIP_SIMULATION)
                    .ok()
                    .and_then(|s| s.parse::<bool>().ok())
                    .unwrap_or(SDK_DEFAULT_SKIP_SIMULATION);

                // Get timeout from environment, defaults to SDK_DEFAULT_TIMEOUT_SECS (14400 seconds / 4 hours)
                // Reference: sp1-sdk-5.2.2/src/network/mod.rs:80
                let timeout_secs = env::var(ENV_SP1_TIMEOUT_SECS)
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(SDK_DEFAULT_TIMEOUT_SECS);
                let timeout = Duration::from_secs(timeout_secs);

                // Get whitelist from environment (comma-separated addresses)
                // If None, SDK uses recently reliable provers
                // Reference: sp1-sdk-5.2.2/src/network/prove.rs:356-383 (whitelist method docs)
                let whitelist = env::var(ENV_SP1_WHITELIST).ok().and_then(|s| {
                    let addresses: Result<Vec<Address>, _> = s
                        .split(',')
                        .map(|addr| Address::from_str(addr.trim()))
                        .collect();
                    addresses.ok()
                });

                ProverMode::Network(NetworkConfig {
                    network_mode,
                    private_key,
                    rpc_url,
                    fulfillment,
                    //fulfillment_strategy,
                    cycle_limit,
                    gas_limit,
                    skip_simulation,
                    timeout,
                    //auction_timeout,
                    whitelist,
                })
            }
            _ => return Err(anyhow::anyhow!("Invalid SP1_PROVER value: '{}'. Expected one of: mock, cpu, cuda, network", sp1_prover)),
        };

        Ok(ProverConfig { mode, proof_type })
    }
}

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
                    ProofType::Groth16 => request.groth16().run()
                }
            }
        }
    }
}

fn get_fulfillment_strategy(config: &FulfillmentConfig) -> FulfillmentStrategy {
    match config {
        FulfillmentConfig::Auction { .. } => FulfillmentStrategy::Auction,
        FulfillmentConfig::Hosted => FulfillmentStrategy::Hosted,
        FulfillmentConfig::Reserved => FulfillmentStrategy::Reserved,
    }
}

fn generate_proof(
    config: &ProverConfig,
    pk: &SP1ProvingKey,
    stdin: &SP1Stdin,
) -> Result<SP1ProofWithPublicValues> {
    match &config.mode {
        ProverMode::Network(net) => {
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
                ProofType::Groth16 => proof_request.groth16()
            };

            // Apply the strategy
            proof_request  = proof_request.strategy(strategy);

            // Apply the auction timeout if and only if we are in Auction mode
            if let FulfillmentConfig::Auction { timeout } = net.fulfillment {
                proof_request = proof_request.auction_timeout(timeout);
            }

            // Chain the remaining defaults and run
            info!("Running sp1 proof.");
            let proof = proof_request.cycle_limit(net.cycle_limit)
                .gas_limit(net.gas_limit)
                .skip_simulation(net.skip_simulation)
                .timeout(net.timeout)
                .whitelist(net.whitelist.clone())
                .run();
            info!("Finished sp1 proof.");

            proof
        },
        ProverMode::Local(local_mode) => {
            let prover = match local_mode  {
                LocalProverMode::Mock => LocalProver::Mock(ProverClient::builder().mock().build()),
                LocalProverMode::Cpu => LocalProver::Cpu(ProverClient::builder().cpu().build()),
                LocalProverMode::Cuda => LocalProver::Cuda(ProverClient::builder().cuda().build()),
            };

            // Generate proof with the configured proof type
            info!("Running sp1 proof.");
            let proof = prover.prove_with_type(pk, stdin, &config.proof_type);
            info!("Finished sp1 proof.");

            proof
        }
    }
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
            generate_proof(&config ,pk, &stdin)
        })
        .await??; // Await the blocking task and propagate errors properly

    Ok(ProverJobOutput {
        proof,
        input_head,
        job_id,
    })
}
