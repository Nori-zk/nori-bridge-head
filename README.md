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
NORI_EXECUTION_CHUNK_LIMIT=

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
- **NORI_EXECUTION_CHUNK_LIMIT**: Chunk size for eth_getProof execution rpc requests (default: 100)
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
- **Whitelist (auction bidder restriction)**: see [Whitelist configuration](#whitelist-configuration) below

See `.env.example`

#### Whitelist configuration

Only relevant when `SP1_FULFILLMENT_STRATEGY=auction`. The whitelist tells the prover network which provers are allowed to bid on your request.

There are three pools the SDK / our code can pull from:

| Pool | Source |
|---|---|
| **User list** | Addresses you set in `SP1_WHITELIST` |
| **Default pool** | `get_provers_by_uptime(high_availability_only: false)` — every recently reliable prover. This is the set the SDK silently injects when you provide no whitelist. |
| **HA subset** | `get_provers_by_uptime(high_availability_only: true)` — the high-availability subset of the default pool. |

**Variables:**

- **`SP1_WHITELIST`** *(optional)* — Comma-separated prover addresses. If set, the auction is restricted to *exactly* these provers (unless extended via the flags below).
- **`SP1_WHITELIST_ADD_HIGH_AVAILABILITY`** *(`true` / `false`, default `false`)* — Extension flag. Requires `SP1_WHITELIST`. When `true`, merges the HA subset into the user list.
- **`SP1_WHITELIST_ADD_DEFAULT`** *(`true` / `false`, default `false`)* — Extension flag. Requires `SP1_WHITELIST`. When `true`, merges the default pool into the user list. Can be combined with `SP1_WHITELIST_ADD_HIGH_AVAILABILITY`; merges are deduplicated.
- **`SP1_WHITELIST_OPEN`** *(`true` / `false`, default `false`)* — Open-auction flag. Mutually exclusive with `SP1_WHITELIST`. When `true`, sends an empty whitelist on the wire, which the network interprets as "any prover can bid." Also disables the SDK's auction-failure retry-with-HA-fallback (which only fires when the whitelist is left unset).

**Regimes (combinations of the four vars above):**

| `SP1_WHITELIST` | `..._ADD_HIGH_AVAILABILITY` | `..._ADD_DEFAULT` | `SP1_WHITELIST_OPEN` | What gets sent to the network |
|---|---|---|---|---|
| unset | — | — | unset / `false` | **SDK default** — SDK auto-injects the default pool client-side; on auction failure, retries with the HA subset |
| unset | — | — | `true` | **Empty list** — any prover can bid; no SDK auto-injection, no fallback retry |
| `0xA,0xB` | `false` | `false` | unset | **Exactly `0xA, 0xB`** |
| `0xA,0xB` | `true` | `false` | unset | **`0xA, 0xB` ∪ HA subset** |
| `0xA,0xB` | `false` | `true` | unset | **`0xA, 0xB` ∪ default pool** |
| `0xA,0xB` | `true` | `true` | unset | **`0xA, 0xB` ∪ HA subset ∪ default pool** (deduplicated) |

Invalid combinations (config will error on startup):
- `SP1_WHITELIST` set together with `SP1_WHITELIST_OPEN=true`

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