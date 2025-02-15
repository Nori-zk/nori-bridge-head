# Nori-bridge-head v1.0.0

Relevant workspace is within the `/nori' folder.

## Installation

`cargo build`

## Configuration

Env vars (create a .env file):

```
NORI_HELIOS_POLLING_INTERVAL=1.0 
SP1_PROVER=network
SP1_PRIVATE_KEY=0x00...
NETWORK_PRIVATE_KEY=0x00..
NETWORK_RPC_URL=https://rpc.succinct.xyz...
SOURCE_CONSENSUS_RPC_URL=https://ethereum-mainnet.core.chainstack.com/beacon/aaefa098dbed72294c371e8c36800986
SOURCE_CHAIN_ID=1
```

- NORI_HELIOS_POLLING_INTERVAL, dictates the polling interval for new slot heads.
- SP1_PROVER, sets the mode for the ZK prover, options are: "mock", "cpu", "cuda" and "network" (note mock executes the program but mocks the zk proof).
- SP1_PRIVATE_KEY, private key.
- NETWORK_PRIVATE_KEY, network prover private key.
- NETWORK_RPC_URL, network prover rpc url.
- SOURCE_CONSENSUS_RPC_URL, consensus rpc.
- SOURCE_CHAIN_ID, chain identifier.

## Execution

`cargo run --bin nbhead`

## How to publish to private registry

1. Get an account at crate.nori.it.com from your administrator
2. Create an auth token
3. Create a .cargo/config.toml

```
# Example .cargo/config.toml
# The index needs to point to the host where your Kellnr instance runs
# The token is the authentication token for the user configured in Kellnr
[registries.kellnr]
index = "sparse+https://crate.nori.it.com/api/v1/crates/"
credential-provider = ["cargo:token"]
```

4. Cargo login - paste your token

```
cargo login --registry kellnr
```

4. `cd nori`
5. `cargo publish --registry kellnr`