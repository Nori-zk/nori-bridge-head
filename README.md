# Nori-bridge-head

Helios light client running inside SP1 zkVM generating consensus proofs used in Nori bridge.

Note the relevant workspace is within the `/nori` folder. And nori specific library code have a prefix of `nori-` or nested within folders with such a prefix.

## Pre-requisites

Since sp1v6 `Protocol Buffers compiler` needs to be installed on your system.

`sudo apt install -y protobuf-compiler`

## Installation

Rust installation:

`cargo build`

## Configuration

Env vars (create a .env file):

```bash
# The source chain, is the chain which the light client will sync from.
NORI_SOURCE_CONSENSUS_HTTP_RPCS=https://ethereum-mainnet.core.chainstack.com/beacon/...,<another consensus rpc url>
NORI_SOURCE_CHAIN_ID=1
NORI_SOURCE_EXECUTION_HTTP_RPCS=https://ethereum-mainnet.core.chainstack.com/...,<another execution rpc url>

# Source contract address.
NORI_TOKEN_BRIDGE_ADDRESS=0x0..

# SP1 Prover configuration (REQUIRED)
SP1_PROVER=mock
SP1_PROOF_TYPE=groth16

# SP1 Network prover (if using SP1_PROVER=network)
SP1_NETWORK_PRIVATE_KEY=0x0..
SP1_NETWORK_RPC_URL=https://rpc.mainnet.succinct.xyz
SP1_NETWORK_MODE=mainnet

# Helios polling interval for new slots.
NORI_HELIOS_POLLING_INTERVAL=

# Timeouts for proof input validation (in seconds) @TODO
NORI_CONSENSUS_PROOF_INPUT_VALIDATION_TIMEOUT=
NORI_EXECUTION_PROOF_INPUT_VALIDATION_TIMEOUT=

# Rust logging level.
NORI_LOG=info
```

### Environment Variables

**Nori Bridge:**
- **NORI_SOURCE_CONSENSUS_HTTP_RPCS**: Comma-delimited consensus RPC URLs
- **NORI_SOURCE_CHAIN_ID**: Source chain identifier (e.g., 1 for Ethereum mainnet)
- **NORI_SOURCE_EXECUTION_HTTP_RPCS**: Comma-delimited execution RPC URLs
- **NORI_TOKEN_BRIDGE_ADDRESS**: Source contract address on the source chain
- **NORI_HELIOS_POLLING_INTERVAL**: Polling interval for Helios client to check for new finality beacon slots
- **NORI_CONSENSUS_PROOF_INPUT_VALIDATION_TIMEOUT**: Timeout (seconds) for consensus proof validation
- **NORI_EXECUTION_PROOF_INPUT_VALIDATION_TIMEOUT**: Timeout (seconds) for MPT consensus proof validation
- **NORI_LOG**: Logging level (e.g., info, debug, warn)

**SP1 Prover (Required):**
- **SP1_PROOF_TYPE**: Proof system - `groth16` or `plonk`
- **SP1_PROVER**: Prover mode - `mock`, `cpu`, `cuda`, or `network` (Succinct Prover Network)

**SP1 Network Mode (when SP1_PROVER=network):**
- **SP1_NETWORK_PRIVATE_KEY**: Private key for network prover authentication (required)
- **SP1_NETWORK_RPC_URL**: Network RPC endpoint (defaults: Mainnet=`https://rpc.mainnet.succinct.xyz`, Reserved=`https://rpc.production.succinct.xyz`)
- **SP1_NETWORK_MODE**: Network type - `mainnet` or `reserved` (default: `mainnet`)
- **SP1_FULFILLMENT_STRATEGY**: Fulfillment method - `auction`, `hosted`, or `reserved` (defaults: Mainnet=`auction`, Reserved=`reserved`)
- **SP1_AUCTION_TIMEOUT_SECS**: Auction timeout in seconds, only used when strategy is `auction` (default: 30)
- **SP1_MIN_AUCTION_PERIOD_SECS**: Minimum time (seconds) the auction must remain open before settling, only used when strategy is `auction`. The auction settles only after this period has elapsed AND at least one bid is received. Recommended 10-15 if you don't have strict latency requirements, to give bidders time to compete and improve pricing (default: 1)
- **SP1_MAX_PRICE_PER_PGU**: Maximum price per PGU (default: 1,000,000,000 / 1.0 $PROVE)
- **SP1_SKIP_SIMULATION**: Skip simulation step - `true` or `false`. When `true`, you must provide cycle/gas limits (default: `false`)
- **SP1_CYCLE_LIMIT**: Max cycles. Only required when `SP1_SKIP_SIMULATION=true`; otherwise SP1 calculates from simulation (defaults when required: Mainnet=1T, Reserved=100M)
- **SP1_GAS_LIMIT**: Gas limit. Only required when `SP1_SKIP_SIMULATION=true`; otherwise SP1 calculates from simulation (default when required: 1B)
- **SP1_TIMEOUT_SECS**: Overall timeout in seconds (default: 600 / 10 minutes)
- **SP1_WHITELIST**: Comma-separated list of prover addresses to whitelist. If not set, SDK uses recently reliable provers (optional)
- **SP1_WHITELIST_ADD_HIGH_AVAILABILITY**: When `true`, extends the configured whitelist with high-availability provers from the network. Only applies when `SP1_WHITELIST` is provided. Useful for ensuring backup provers are available - `true` or `false` (default: `false`, optional)

See `.env.example`

### Example Configurations

**Local testing (mock prover):**
```bash
SP1_PROVER=mock
SP1_PROOF_TYPE=groth16
```

**Local CPU proving:**
```bash
SP1_PROVER=cpu
SP1_PROOF_TYPE=groth16
```

**Network proving (Mainnet with auction):**
```bash
SP1_PROVER=network
SP1_PROOF_TYPE=plonk
SP1_NETWORK_PRIVATE_KEY=0x...
SP1_NETWORK_MODE=mainnet
```

**Network proving (Reserved capacity):**
```bash
SP1_PROVER=network
SP1_PROOF_TYPE=plonk
SP1_NETWORK_PRIVATE_KEY=0x...
SP1_NETWORK_MODE=reserved
```

## Build Nori-Sp1-Helios-ZK

Ensure you are using nightly if not:

```sh
rustup override set nightly
cargo clean
cargo build
```

1. ./nori/rebuild-zk.sh

## Execution

`cargo run --bin nbhead`

## Tests

`cargo test -- --nocapture`

## Nori Contract

For information on how to deploy the source contract see [here](https://github.com/Nori-zk/nori-bridge-sdk/tree/main/contracts/ethereum). 