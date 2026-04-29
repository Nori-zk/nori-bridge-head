# Changelog

23/4/26 — Bridge SDK ref update + genesis_root commitment

### Added

- Commit genesis_root as ZK public output. Without genesis_root a governance action could silently swap the underlying Ethereum chain, allowing store hashes from a different derivative chain to pass verification. Adding genesis_root as a committed proof output ensures all transitions are bound to the same chain lineage. Surface all proof commitments for transparency, align consensus.rs docs with execution steps.
- nori-primitives/src/types.rs: Add genesis_root to ProofOutputs ([196..228]) and ConsensusProofOutputs ([144..176]) with serialization/deserialization, bump SIZE from 196->228 and 144->176
- nori-program/src/consensus.rs: Commit genesis_root in both consensus_program and consensus_mpt_program outputs, rename State Commitment to State Capture, add Output Commitment as distinct final step, correct SHA-256(serde_serialize(store)) description, align all inline comments with docstring step names, update debug/println messages (old->last, packing->committing)
- nori/src/bridge_head/api.rs: Add verified_contract_storage_slots_root, next_sync_committee_hash, contract_address, genesis_root to ProofMessage struct and both construction sites (BridgeHeadJobSucceeded notice and proof emit)
- nori/src/bridge_head/notice_messages.rs: Add verified_contract_storage_slots_root, next_sync_committee_hash, contract_address, genesis_root to TransitionNoticeExtensionBridgeHeadJobSucceeded

## 19/4/26 — Bridge SDK ref update

### Changed

- **`nori/src/contract_bindings/bridge-sdk.ref`** bumped to [`ce5bde7b09ef45284e840aef96fbd4fe64d80e48`](https://github.com/Nori-zk/nori-bridge-sdk/tree/ce5bde7b09ef45284e840aef96fbd4fe64d80e48) (branch `CHORE/single-mina-contracts`) — pulls in the latest `NoriTokenBridge` contract with the aligned features required for nori burn

## 26/3/26 — Mesa Contracts

### Changed

- **New contract binding method**: switched to github-style fetching for contract bindings ([f9b3fec](../../commit/f9b3fec))
- **Deprecate address field**: swapped to the new hash function ([6af3166](../../commit/6af3166))

### Added

- **Contract address as public output** (CRITICAL): exposed as a public output so downchain consumers can use it ([c5758ff](../../commit/c5758ff))
- **ELF rebuild**: regenerated ZK artifact to include `contract_address` in public outputs ([eae41ad](../../commit/eae41ad))

### Fixed

- **`SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX`** bumped by 1: `ReentrancyGuard` inheritance shifted the `NoriTokenBridge` storage slot ([6e676f3](../../commit/6e676f3))

## 24/2/26 — SP1 v5/v6 Migration

### Changed

- **sp1-sdk upgraded from `5.2.4` to `6.0.1`** across all crates (`nori`, `nori-program`, `program`)
- **Async migration**: `ProverClient::builder()` calls, `client.setup()`, and all proof generation paths are now fully `async`/`.await` — previously blocking/sync; `spawn_blocking` removed from `finality_update_job` and `benchmark_sha256_serde` as both are now natively async end-to-end
- **`nori/src/sp1_prover.rs`**: `PROVING_KEY` cache switched from `std::sync::OnceLock` to `tokio::sync::OnceCell`; `get_proving_key()` now returns `Result`; `generate_proof` no longer takes `pk` as a parameter — proving key is now fetched internally
- **`LocalProver`**: `Mock` variant updated from `sp1_sdk::CpuProver` to the new `sp1_sdk::MockProver` type introduced in v6; `prove_with_type()` is now `async`; `Mock` and `Cpu` variants split to fetch their respective proving keys internally; `.run()` calls replaced with `.await?`
- **`nori/tests/benchmark_sha256_serde.rs`**: updated to async API — removed `spawn_blocking`, switched to `ProverClient::builder().cpu().build().await`, uses `Elf::Static(ELF)`
- **`nori-hash/src/sha256_hash.rs`**: import updated from `sha2_v0_10_9` to `sha2_v0_10_8` to match renamed patch crate
- **Patch tags updated** for sp1-patched crates: `sha2`, `sha3`, `tiny-keccak`, `bls12_381` all bumped to `sp1-6.0.0`/`sp1-6.0.0-v2` tags
- **`nori/rebuild-zk.sh`**: updated to `cd nori-build-zk` instead of `cd script`

### Added

- **`nori-build-zk` crate** (`nori-build-zk/bin/make.rs`): new crate replacing the old `script` crate; provides a standalone binary that builds the ZK program via Docker (tag `v6.0.1`) and derives and writes `nori-sp1-helios-program.vk.json` using the mock client
- **`get_cuda_proving_key()`** added to `sp1_prover.rs` with its own `CUDA_PROVING_KEY` cache using `CudaProvingKey` from `sp1-cuda`
- **`sp1-cuda = "6.0.1"`** added as a workspace and `nori` crate dependency

### Fixed

- **`nori/bin/extract_zeroth_public_input.rs`**: simplified pi0 derivation — now uses `vk.bytes32()` directly (strips `0x`, converts hex → `U256` → decimal string), eliminating the need for a consensus RPC or running a mock proof
- Updated SHA2 patch alias from `sha2-v0-10-9` → `sha2-v0-10-8` to match the correct sp1-patches tag
