# Nori-bridge-head

Helios light client running inside SP1 zkVM generating consensus proofs used in Nori bridge.

Note the relevant workspace is within the `/nori' folder.

## Installation

`cargo build`

## Configuration

Env vars (create a .env file):

```
SP1_PROVER=network
NETWORK_PRIVATE_KEY=0x00..
NETWORK_RPC_URL=https://rpc.succinct.xyz...
SOURCE_CONSENSUS_RPC_URL=https://ethereum-mainnet.core.chainstack.com/beacon/...
SOURCE_CHAIN_ID=1
NORI_LOG=info
NORI_HELIOS_POLLING_INTERVAL=1.0 
```

- SP1_PROVER, sets the mode for the ZK prover, options are: "mock", "cpu", "cuda" and "network" (note mock executes the program but mocks the zk proof).
- NETWORK_PRIVATE_KEY, network prover private key.
- NETWORK_RPC_URL, network prover rpc url.
- SOURCE_CONSENSUS_RPC_URL, consensus rpc.
- SOURCE_CHAIN_ID, chain identifier.
- NORI_LOG, Nori logging level.
- NORI_HELIOS_POLLING_INTERVAL, dictates the polling interval for latest Helios client beacon slot.

## Execution

`cargo run --bin nbhead`