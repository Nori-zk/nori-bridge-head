# Changelog

## 15/5/26 - Audit A2090: Non-standard Merkle zero indexing

### Finding (verbatim)

Finding a2090: `buildMerkleTree` uses the `zeros` array backwards

Hi, we noticed an issue in nori-bridge-sdk\o1js-zk-utils\src\merkle-attestor\merkleTree.ts. When using zero hash while building the Merkle tree, the incorrect level/index is used.

In buildMerkleTree (and also foldMerkleLeft and getMerklePathFromLeaves), zeros is set to be getMerkleZeros(depth) by the caller, which generates an array of Hashes that correspond to all-zero subtrees.

```typescript
/**
 * Generate zero hashes array of length depth + 1
 */
export function getMerkleZeros(depth: number): Field[] {
    const zeros: Field[] = [];

    // Start with zeros[0] = Field(0)
    zeros.push(Field(0));

    for (let i = 1; i < depth + 1; i++) {
        // Each next zero is hash of the previous zero with itself
        zeros.push(Poseidon.hash([zeros[i - 1], zeros[i - 1]]));
    }

    return zeros;
}
```

Notice that the array is ordered from smallest subtree (tree with a single 0 node, depth 0) to largest (tree of depth depth, i.e. depth+1 levels).

However, in buildMerkleTree, when utilizing the zeros array, the following snippet is used:

```typescript
for (let level = depth; level > 0; level--) {
    // Omitted...

    for (let i = 0; i < parentWidth; i++) {
        const leftIdx = 2 * i;

        if (leftIdx >= nNonDummyNodes) {
            // Both left and right dummy nodes, use zeros cache
            parentLevel[i] = zeros[level];
        } else {
            // Omitted...
        }
    }
    // Omitted...
}
```

The zeros array is used backwards. e.g., when level=depth, the child level is the bottom layer of the tree, and the parent level is the layer above and hence should use zeros[1], hash that corresponds to a subtree of depth 1. Instead, the current code uses zeros[level], which is the hash for a subtree of depth depth.

This leads to a completeness issue. The Mina bridge's off-chain witness builder uses this helper to derive deposit proofs, and noriMint() later recomputes the root on-chain and requires it to match the verified Ethereum deposit root. Whenever the number of leaves in the tree is not a power of 2 (i.e., there are dummy nodes in the tree), due to this incorrect calculation of the Merkle root, valid deposits would become unmintable even though the Ethereum proof and deposit data are correct.

The fix is relatively straightforward: either reverse the order of the result of getMerkleZeros, or replace parentLevel[i] = zeros[level]; with parentLevel[i] = zeros[depth + 1 - level];.

### Response

The non-standard indexing is acknowledged. The same reversed indexing exists symmetrically in both the TypeScript (`merkleTree.ts`) and Rust (`merkle_poseidon_fixed.rs`) implementations across all three affected functions: `buildMerkleTree`/`build_merkle_tree`, `foldMerkleLeft`/`fold_merkle_left`, and `getMerklePathFromLeaves`/`get_merkle_path_from_leaves`. Because both producers in this closed system use the same non-standard convention, the computed roots agree across languages for all leaf counts. We do not believe there is a soundness or completeness failure in the deployed system. If the bug were asymmetric, failures would appear at any non-power-of-two count leaving adjacent dummies, the smallest being n=5, then 6, 9, 10, 11, 12, 13. Applying the proposed fix to only one side would introduce the completeness failure described in the report. The mistake cancels out leaving it safe as written but highly non-standard. Worth fixing but needs to be done carefully to avoid regression of the mint function.

### Discussion

After discussion it was noted that the two cited tests are not sufficient to rule out cross-language divergence when run in isolation, as each only checks self-consistency within its own language. This is agreed. The tests were not designed to be used in isolation. They were designed to be used in concert: the raw output from any two of the three test suites (Rust, TypeScript non-provable, TypeScript provable) was compared using an uncommitted comparison script that normalised and diffed leaves and roots line-by-line across languages. An improved version of this script (`nori-bridge-sdk/o1js-zk-utils/test/cross-reference-roots.sh`) is now committed to nori-bridge-sdk for transparency.

Three test suites cover this code:

1. Rust - `cargo test -p nori-hash test_all_leaf_counts_and_indices_with_build_and_fold` (n_leaves 0-50)
2. TypeScript (non-provable) - `npm run test -- -t "test_all_leaf_counts_and_indices_with_build_and_fold"` (n_leaves 0-50)
3. TypeScript (provable) - `npm run test -- -t "test_all_leaf_counts_and_indices_with_pipeline"` (n_leaves 0-10, truncated for speed; previously run to 50)

This cross-referencing is a sample-based confidence check, not a claim of completeness proof.

The finding correctly identifies a deviation from the standard Merkle zero-hash convention. While harmless in the current closed two-implementation system, non-standard indexing would be a problem for any future third-party verifier or public auditability tooling that assumes the standard convention. The fix is accepted and will be applied to both sides simultaneously. Testing will be bolstered first (commit 1) to expose the non-standard indexing against independent reference implementations, then the fix applied (commit 2), so that the before and after results can be documented in this summary.

### Commit 1 - Test exposure of the non-standard indexing

- **Regression tests** (`nori-hash/src/merkle_poseidon_fixed.rs`): added `regression_a2090_bruteforce_reference` and `regression_a2090_recursive_reference` tests over leaf counts [1, 3, 5, 6, 9, 17] verifying `build_merkle_tree` and `fold_merkle_left` against two independent reference implementations. The brute-force reference pads with zeros and hashes every pair with no zeros cache. The recursive reference builds the tree top-down, returning `Fp(0)` for empty subtrees. Neither references the zeros array. Leaf counts 5, 6, 9, 17 exercise the bug (adjacent dummy nodes at various depths); 1 and 3 are sanity cases where only a lone dummy pairs with a real leaf.

Results:

- Regression tests: n_leaves 1 and 3 pass (no adjacent dummies, 8 pass). n_leaves 5, 6, 9, 17 fail against both the brute-force and recursive references for both `build_merkle_tree` and `fold_merkle_left` (8 failures per reference, 16 fail, 24 total checks), confirming the non-standard indexing is detectable and diverges from the standard convention.
- Self-consistency (Rust): passes 0-50 leaves.
- Self-consistency (TypeScript non-provable): passes 0-50 leaves.
- Self-consistency (TypeScript provable): passes 0-10 leaves.
- Cross-reference (unpatched Rust vs unpatched TypeScript non-provable): 51 leaf counts, all leaves and roots match. Zero differences.
- Cross-reference (unpatched Rust vs unpatched TypeScript provable): 11 leaf counts (0-10), all leaves and roots match. Zero differences.

## 23/4/26 — Bridge SDK ref update + genesis_root commitment

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
