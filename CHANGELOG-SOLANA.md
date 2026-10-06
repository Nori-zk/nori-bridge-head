# Changelog

## 6/10/26 - Follow Ethereum through the Glamsterdam fork

Keep the bridge head proving Ethereum's finalized state after the Glamsterdam (Gloas) fork activated on Sepolia at epoch 353024. THIS IS A POST AUDIT GUEST PROGRAM CHANGE. Glamsterdam's enshrined proposer-builder separation (EIP-7732) moved the execution payload out of the beacon block, so Gloas light client headers no longer carry the execution payload header, and with it the execution state root and block number that the guest commits and verifies the proof request queue against. Only the execution block hash remains, proven against the beacon block by helios during update verification. The guest now takes the RLP encoded execution block header as an input, accepts it only if its keccak256 equals that execution block hash, and reads the state root and block number from it, so the trust chain from the sync committee signature to the state root is unbroken: signature, beacon block, execution block hash, execution header, state root. Pre-Gloas headers keep the audited path unchanged. Helios 0.12.0 is required to verify Gloas light client updates at all, and brings alloy 2.x, which in turn moves SP1 to 6.8.1; the SP1 Groth16 circuit is unchanged (v6.1.0), so the Solana verifier's Groth16 key and vk root still hold and only the program vkey changes.

- `nori-program/src/consensus.rs`: POST AUDIT CHANGE. `finalized_execution_state_root_and_block_number` added: pre-Gloas headers read the state root and block number from the execution payload header as before; Gloas headers read them from `execution_header_rlp` once its keccak256 equals the finalized header's execution block hash and it decodes as exactly one block header. `consensus_program` and `consensus_mpt_program` call it in place of the audited extraction, which is kept commented out in place. `ExecutionHeaderHashMismatch` and `InvalidExecutionHeader` added. Input tables, operation steps and error conditions updated, with the audited text struck through.
- `nori-primitives/src/types.rs`: POST AUDIT CHANGE. `execution_header_rlp` added to `ProofInputs` and `ConsensusProofInputs`, empty for pre-Gloas finalized headers.
- `nori-elf/nori-sp1-helios-program`, `nori-elf/nori-sp1-helios-program.vk.json`, `nori-elf/nori-sp1-helios-program.pi0.json`: rebuilt for the guest change, helios 0.12.0 and SP1 6.8.1. Program vkey `0x00c3654b70a7d1fe4a98c660009c96b95a1d0d3c22025f0b610b252aa5981125`.
- `nori-program/Cargo.toml`: `sp1-zkvm` 6.1.0 to 6.8.1; `alloy-consensus` added for the execution block header type, whose Amsterdam fields 1.x lacks.
- `nori/src/rpcs/execution/http.rs`: `get_execution_header` fetches the execution block header by hash over the execution RPCs and checks its hash, so a provider returning the wrong header fails over. `ExecutionHttpProxy` implements `Clone` for the consensus multiplex closures. `ProofInputs` carries `execution_header_rlp`.
- `nori/src/rpcs/consensus/mod.rs`: `get_finalized_execution_header_rlp` derives the finalized header the program will reach and fills `execution_header_rlp` on each provider attempt. The input block number is read from the execution block header for Gloas headers. helios 0.12.0 removed `BeaconBlock` and `ConsensusRpc::get_block`: `bootstrap_from_slot` reads the block root from `/eth/v1/beacon/headers/{slot}` and `get_current_checkpoint` hashes the finalized beacon header, whose root is the block root.
- `nori/Cargo.toml`: `alloy-consensus` added with `serde`.
- `Cargo.toml`: `helios`, `helios-consensus-core` and `helios-ethereum` 0.11.0 to 0.12.0. `sp1-sdk`, `sp1-build` and `sp1-cuda` 6.1.0 to 6.8.1. `sha2` and `tiny-keccak` patches to their `sp1-6.2.0` tags. `sha3` patch removed, as nothing depends on `sha3` 0.10 any more. `bls12_381` kept on `patch-0.8.0-sp1-6.0.0-v2`, as the `sp1-6.2.0` patch moved to digest 0.10, which helios's `hash_to_curve` does not compile against. `alloy-consensus` 2.5.0 added. Unused workspace dependencies removed: `helios`, `eyre`, `tracing`, `thiserror`, `zduny-wasm-timer`, `hex`, `alloy-contract`, `serde_with`, `once_cell`, `sp1-helios-program`, `sp1-helios-primitives`, and the `sp1-4.0.0` `sha2` comment. `homepage` and `repository` point to `nori-bridge-head`. The upstream helios link is pinned to tag 0.12.0.
- `Cargo.lock`: re-resolved for the above.

`cargo test --workspace` fails only `consensus_mpt_program_accepts_non_checkpoint_slot` in `nori/tests/1eb72_non_checkpoint_regression.rs`, whose fixture predates `queue_storage` and `execution_header_rlp`. `benchmark_sha256_serde` passes against the rebuilt ELF. nbhead produced a mock proof past the fork.

## 28/9/26 - Ethereum state settlement on Solana

Adapt the bridge head to settle Ethereum's finalized state on Solana.

- `nori-hash/src/merkle_poseidon_fixed.rs`: renamed to `nori-hash/src/merkle_sha256_fixed.rs`. The verified requests tree and request leaf hash with SHA-256, which Solana can recompute, in place of Kimchi Poseidon. `pack_request_leaf_fields` and `hash_request_leaf` are infallible.
- `nori-hash/src/merkle-zeros.dat`: regenerated with SHA-256 zero hashes.
- `nori-hash/src/lib.rs`: exports `merkle_sha256_fixed`.
- `nori-hash/src/bin/hash_vectors.rs`: emits SHA-256 vectors.
- `nori-hash/scripts/cross_check.mjs`: checks the vectors against Node `crypto` SHA-256.
- `nori-hash/scripts/package.json`: `o1js` dependency removed.
- `nori-hash/tests/request_leaf_byte_routing.rs`: asserts packing on 32-byte fields and writes leaves as hex.
- `test-vectors/proof-request-queue/request-leaf-vectors.json`: regenerated.
- `test-vectors/proof-request-queue/README.md`: documents SHA-256 leaf packing and names `nori-solana-sdk` and the Solana program as consumers.
- `nori-program/src/mpt.rs`: builds the requests root with `merkle_sha256_fixed`. `MptError::LeafHashError` removed, as leaf hashing is infallible.
- `nori-program/src/consensus.rs`: `LeafHashError` removed from the error documentation; the verifier contract comment names the destination chain.
- `nori/bin/nori_bridge_head.rs`: the queue cursor comment names the destination chain.
- `nori/src/rpcs/execution/http.rs`: imports `MAX_BATCH` from `merkle_sha256_fixed`.
- `nori/tests/benchmark_sha256_serde.rs`: sends `ProofInputs`, the type the guest decodes, and asserts exit code 0.
- `.env.example`: `SP1_PROOF_TYPE=groth16`, the proof type verified on Solana.
- `README.md`: network proving examples use `groth16`; the contract deployment link points to `nori-solana-sdk`.
- `Cargo.toml`, `nori-hash/Cargo.toml`, `nori-program/Cargo.toml`, `Cargo.lock`: `mina-poseidon`, `mina-curves` and `o1-utils` removed.
