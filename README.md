# Nori-bridge-head

Helios light client running inside SP1 zkVM generating consensus proofs used in Nori bridge.

Note the relevant workspace is within the `/nori` folder. And nori specific library code have a prefix of `nori-`.

## Installation

`cargo build`

## Configuration

Env vars (create a .env file):

```
# The source chain, is the chain which the light client will sync from.
SOURCE_CONSENSUS_RPC_URL=https://ethereum-mainnet.core.chainstack.com/beacon/...
SOURCE_CHAIN_ID=1

# SP1 Prover. Set to mock for testing, or use network to generate proofs on the Succinct Prover Network.
SP1_PROVER=mock
PRIVATE_KEY=0x0...

# SP1 Network prover (if using SP1_PROVER=network).
SP1_VERIFIER_ADDRESS=...
NETWORK_PRIVATE_KEY=0x0..
NETWORK_RPC_URL=https://rpc.succinct.xyz...

# Helios polling interval for new slots.
NORI_HELIOS_POLLING_INTERVAL=

# Rust logging level.
NORI_LOG=info
```

- **SOURCE_CONSENSUS_RPC_URL**: consensus rpc.
- **SOURCE_CHAIN_ID**: chain identifier.
- **SP1_PROVER**: sets the mode for the ZK prover, options are: "mock", "cpu", "cuda" and "network" (note mock executes the program but mocks the zk proof).
- **PRIVATE_KEY**: private key for the account that will be deploying the contract
- **SP1_VERIFIER_ADDRESS**: the address of the verifier contract
- **NETWORK_PRIVATE_KEY**: network prover private key.
- **NETWORK_RPC_URL**: network prover rpc url.
- **NORI_HELIOS_POLLING_INTERVAL**: dictates the polling interval for latest Helios client beacon slot.
- **NORI_LOG**: Nori logging level.

## Build Nori-Sp1-Helios-ZK

1. Remove target directory in root `sudo rm -rf target && cargo clean` (to ensure a clean build)
2. Build the ZK `cd scripts/ && cargo run --bin make && cd ..`

## Execution

`cargo run --bin nbhead`

## Tests

`cargo test -- --nocapture`