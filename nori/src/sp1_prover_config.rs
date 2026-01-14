use alloy_primitives::Address;
use anyhow::Result;
use sp1_sdk::network::{proto::types::FulfillmentStrategy, NetworkMode};
use std::{env, str::FromStr, time::Duration};

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
const ENV_SP1_MAX_PRICE_PER_PGU: &str = "SP1_MAX_PRICE_PER_PGU";
const ENV_SP1_AUCTION_TIMEOUT_SECS: &str = "SP1_AUCTION_TIMEOUT_SECS"; // Maps to auction timeout
const ENV_SP1_WHITELIST: &str = "SP1_WHITELIST"; // Maps to ProveRequest.whitelist()

// Default values from sp1-sdk-5.2.2

// 1 PROVE (18 decimals). from 5.2.2/src/network/prove.rs Line 508 wrong value TODO
// const SDK_DEFAULT_PRICE_PER_PGU: u64 = 500_000_000_000_000; // Max price per bPGU: 1001882102603448320 (1.0018 $PROVE)
const SDK_DEFAULT_PRICE_PER_PGU: u64 = 1_000_000_000; //Max price per bPGU: 1000000000000000000 (1.0000 $PROVE)
                                                      // Reference: sp1-sdk-5.2.2/src/network/mod.rs
const SDK_MAINNET_RPC_URL: &str = "https://rpc.mainnet.succinct.xyz"; // Line 67
const SDK_RESERVED_RPC_URL: &str = "https://rpc.production.succinct.xyz"; // Line 69
const SDK_DEFAULT_AUCTION_TIMEOUT_SECS: u64 = 30; // Line 76: Duration::from_secs(30) / or 1sec TODO?

const SDK_MAINNET_DEFAULT_CYCLE_LIMIT: u64 = 1_000_000_000_000; // Line 77
const SDK_RESERVED_DEFAULT_CYCLE_LIMIT: u64 = 100_000_000; // Line 78
const SDK_DEFAULT_GAS_LIMIT: u64 = 1_000_000_000; // Line 79
const SDK_DEFAULT_TIMEOUT_SECS: u64 = 600; // Line 80

// Reference: sp1-sdk-5.2.2/src/network/prover.rs:174
const SDK_DEFAULT_SKIP_SIMULATION: bool = false;

// Configuration for SP1 prover types

pub enum ProverMode {
    Local(LocalProverMode),
    Network(NetworkConfig),
}

#[derive(Debug)]
pub enum LocalProverMode {
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
    pub max_price_per_pgu: u64,
    pub cycle_limit: u64,
    pub gas_limit: u64,
    pub skip_simulation: bool,
    pub timeout: Duration,
    pub whitelist: Option<Vec<Address>>,
}

pub struct ProverConfig {
    pub mode: ProverMode,
    pub proof_type: ProofType,
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

    /// Mock prover whose proof type is read from `SP1_PROOF_TYPE` env var.
    /// Errors if the variable is missing or contains an invalid value.
    pub fn mock_from_env() -> Result<Self> {
        let proof_type = match env::var(ENV_SP1_PROOF_TYPE).ok().as_deref() {
            Some("plonk") => ProofType::Plonk,
            Some("groth16") => ProofType::Groth16,
            Some(other) => {
                return Err(anyhow::anyhow!(
                    "Invalid {} value: '{}'. Expected 'plonk' or 'groth16'",
                    ENV_SP1_PROOF_TYPE,
                    other
                ))
            }
            None => {
                return Err(anyhow::anyhow!(
                    "Missing {} environment variable. Expected 'plonk' or 'groth16'",
                    ENV_SP1_PROOF_TYPE
                ))
            }
        };

        Ok(ProverConfig {
            mode: ProverMode::Local(LocalProverMode::Mock),
            proof_type,
        })
    }

    /// Load and validate configuration from environment
    pub fn from_env() -> Result<Self> {
        let sp1_prover = env::var(ENV_SP1_PROVER).unwrap_or_else(|_| "cpu".to_string());

        // Get proof type from environment, defaults to groth16, error on invalid value
        let proof_type = match env::var(ENV_SP1_PROOF_TYPE).ok().as_deref() {
            Some("plonk") => ProofType::Plonk,
            Some("groth16") => ProofType::Groth16,
            Some(other) => {
                return Err(anyhow::anyhow!(
                    "Invalid {} value: '{}'. Expected 'plonk' or 'groth16'",
                    ENV_SP1_PROOF_TYPE,
                    other
                ))
            }
            None => {
                return Err(anyhow::anyhow!(
                    "Invalid {} value: None. Expected 'plonk' or 'groth16'",
                    ENV_SP1_PROOF_TYPE
                ))
            }
        };

        let mode = match sp1_prover.as_str() {
            "mock" => ProverMode::Local(LocalProverMode::Mock),
            "cpu" => ProverMode::Local(LocalProverMode::Cpu),
            "cuda" => ProverMode::Local(LocalProverMode::Cuda),
            "network" => {
                // Get network private key from environment if set
                // Reference: sp1-sdk-5.2.2/src/network/builder.rs:32,166
                let network_mode = match env::var(ENV_SP1_NETWORK_MODE).ok().as_deref() {
                    Some("mainnet") => NetworkMode::Mainnet,
                    Some("reserved") => NetworkMode::Reserved,
                    Some(other) => {
                        return Err(anyhow::anyhow!(
                            "Invalid {} value: '{}'. Expected 'mainnet' or 'reserved'",
                            ENV_SP1_NETWORK_MODE,
                            other
                        ))
                    }
                    None => NetworkMode::Mainnet
                };

                let private_key = env::var(ENV_NETWORK_PRIVATE_KEY).map_err(|_| {
                    anyhow::anyhow!("{} required for network mode", ENV_NETWORK_PRIVATE_KEY)
                })?;

                // Get RPC URL from environment or use default based on network mode
                // Reference: sp1-sdk-5.2.2/src/network/mod.rs:67,69
                let rpc_url =
                    env::var(ENV_NETWORK_RPC_URL)
                        .ok()
                        .unwrap_or_else(|| match network_mode {
                            NetworkMode::Mainnet => SDK_MAINNET_RPC_URL.to_string(),
                            NetworkMode::Reserved => SDK_RESERVED_RPC_URL.to_string(),
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
                            FulfillmentConfig::Auction {
                                timeout: Duration::from_secs(secs),
                            }
                        }
                        Some("hosted") => FulfillmentConfig::Hosted,
                        Some("reserved") => FulfillmentConfig::Reserved,
                        None => {
                            // Default based on network_mode
                            match network_mode {
                                NetworkMode::Mainnet => {
                                    let secs = SDK_DEFAULT_AUCTION_TIMEOUT_SECS;
                                    FulfillmentConfig::Auction {
                                        timeout: Duration::from_secs(secs),
                                    }
                                }
                                NetworkMode::Reserved => FulfillmentConfig::Reserved,
                            }
                        }
                        Some(other) => return Err(anyhow::anyhow!("Invalid strategy: {}", other)),
                    }
                };

                // 5.2.2/src/network/prove.rs Line 508
                let max_price_per_pgu = env::var(ENV_SP1_MAX_PRICE_PER_PGU)
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(SDK_DEFAULT_PRICE_PER_PGU);

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
                    max_price_per_pgu,
                    cycle_limit,
                    gas_limit,
                    skip_simulation,
                    timeout,
                    whitelist,
                })
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Invalid SP1_PROVER value: '{}'. Expected one of: mock, cpu, cuda, network",
                    sp1_prover
                ))
            }
        };

        Ok(ProverConfig { mode, proof_type })
    }
}

/// Helper to get SP1 FulfillmentStrategy from our own FulfillmentConfig enum which includes the timeout
pub fn get_fulfillment_strategy(config: &FulfillmentConfig) -> FulfillmentStrategy {
    match config {
        FulfillmentConfig::Auction { .. } => FulfillmentStrategy::Auction,
        FulfillmentConfig::Hosted => FulfillmentStrategy::Hosted,
        FulfillmentConfig::Reserved => FulfillmentStrategy::Reserved,
    }
}
