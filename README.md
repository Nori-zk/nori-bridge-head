# Nori-bridge-head

Helios light client running inside SP1 zkVM generating consensus proofs used in Nori bridge.

Note the relevant workspace is within the `/nori` folder. And nori specific library code have a prefix of `nori-`.

## Installation

Rust installation:

`cargo build`

Smart contract installation (as `NoriTokenBridge.json` is needed from [Nori Bridge SDK](https://github.com/Nori-zk/nori-bridge-sdk) ):

`cd cd nori/src/contracts/ && npm install`

## Configuration

Env vars (create a .env file):

```
# The source chain, is the chain which the light client will sync from.
NORI_SOURCE_CONSENSUS_HTTP_RPCS=https://ethereum-mainnet.core.chainstack.com/beacon/...,<another consensus rpc url>
NORI_SOURCE_CHAIN_ID=1
NORI_SOURCE_EXECUTION_HTTP_RPCS=https://ethereum-mainnet.core.chainstack.com/...,<another execution rpc url>

# Source contract address.
NORI_TOKEN_BRIDGE_ADDRESS=0x0..

# SP1 Prover. Set to mock for testing, or use network to generate proofs on the Succinct Prover Network.
SP1_PROVER=mock

# SP1 Network prover (if using SP1_PROVER=network).
SP1_VERIFIER_ADDRESS=...
NETWORK_PRIVATE_KEY=0x0..
NETWORK_RPC_URL=https://rpc.succinct.xyz...

# Helios polling interval for new slots.
NORI_HELIOS_POLLING_INTERVAL=

# Rust logging level.
NORI_LOG=info
```

- **NORI_SOURCE_CONSENSUS_HTTP_RPCS**: Comma delimited consensus rpc urls.
- **NORI_SOURCE_CHAIN_ID**: Source chain identifier.
- **NORI_SOURCE_EXECUTION_HTTP_RPCS**: Comma delimited execution rpc urls.
- **NORI_TOKEN_BRIDGE_ADDRESS**: The source contract's address in the source chain.
- **SP1_PROVER**: sets the mode for the ZK prover, options are: "mock", "cpu", "cuda" and "network" (note mock executes the program but mocks the zk proof).
- **SP1_VERIFIER_ADDRESS**: the address of the verifier contract
- **NETWORK_PRIVATE_KEY**: network prover private key.
- **NETWORK_RPC_URL**: network prover rpc url.
- **NORI_HELIOS_POLLING_INTERVAL**: dictates the polling interval for the Helios client to find the latest finality beacon slot.
- **NORI_CONSENSUS_PROOF_INPUT_VALIDATION_TIMEOUT**: how long a consensus proof validation check is given (in seconds) before timing out.
- **NORI_EXECUTION_PROOF_INPUT_VALIDATION_TIMEOUT**: how long a mpt consensus proof validation check is given (in seconds) before timing out.
- **NORI_LOG**: Nori logging level.

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

For information on how to deploy the source contract see [here](./nori-contracts/README.md). 