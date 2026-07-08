# Changelog

## 8/7/26 - Audit 1eb72: `output_slot % 32 == 0` checkpoint constraint not enforced in ZK program or on-chain

### Finding (verbatim)

Finding 1eb72:

While the check regarding the slot advancing and the next sync committee not being zero are enforced by the Mina contract, this is not the case for the `store.finalized_header.beacon().slot % 32 == 0` check. For honest provers who prepare proofs via code going through `prepare_consensus_mpt_proof_inputs` function, with `validate` set to true, the out-of-circuit off-chain checks prevent accidental usage of such finalized headers.

That check there appears to strongly suggest that it would a problem for the honest operator code if the finalized header slot to start out with on the next update would not satisfy the `% 32 == 0` condition:

```rust
// nori-bridge-head: nori/src/rpcs/consensus/mod.rs

// Block non-checkpoint slots (they prevent bootstrapping on restart)
// We need the validate_progress guard as its used as a flag to allow
// the proof anyway. And for vk building (and zk change detection) we need to be able to arbirarily bypass this 
// sort of validation.
if validate && output_slot % 32 > 0 { 
    return Err(anyhow::anyhow!(
        "Output slot {} was a non-checkpoint slot. Preventing this as it prevents bootstrapping if we go offline.",
        output_slot,
    ));

// Block non-checkpoint slots (ones where we fail to actually bootstrap by trying it explicitly)
// This re-enforces the output_slot % 32 > 0 validation check.
// I addition to checking the output_slot number % 32 lets try to bootstrap from this slot explicitly
if validate {
    Client::<S, R>::bootstrap_from_slot(&url, output_slot).await
    .map_err(|e| anyhow::anyhow!(
        "Failed to bootstrap from slot {} using {}. Preventing the use of this output_slot as it could lead to a stall of the bridge:\n{}",
        output_slot, url, e
    ))?;
}
```

What is the reason for this and what is the precise problem?

If this would prevent the honest operator code from producing the next proof, then this would amount to a temporary denial of of service possibility for an attacker who submits an update on-chain for an epoch where the finalized header slot is not a checkpoint slot.

### Response

The severity is higher than the finding suggests. The finding describes a "temporary denial of service" but the impact is a permanent denial of service that is repeatable even after recovery.

The `update()` method on the Mina `NoriTokenBridge` contract is permissionless. Anyone who can produce a valid SP1 proof can call it. The contract advances `latestHead` to the proof's `outputSlot` and updates the store hash chain (`latestHeliusStoreInputHashHighByte`, `latestHeliusStoreInputHashLowerBytes`) to the proof's `outputStoreHash`. Neither the ZK program (`consensus_mpt_program`) nor the contract enforce `outputSlot % 32 == 0`.

If any party submits a proof whose `outputSlot` is a non-checkpoint slot, the honest operator cannot produce a valid next proof from that slot. This does not require malicious intent; anyone running their own operator implementation without the off-chain `% 32` guard would trigger it. When `bootstrap_from_slot` is called, it derives a checkpoint hash from the block at that slot and passes it to the beacon chain's `getLightClientBootstrap` endpoint, which returns "LC bootstrap unavailable" because bootstrapping is only supported for checkpoint slots. The operator cannot reconstruct a Helios store rooted at that slot, so it cannot produce a proof whose `inputStoreHash` matches the now-committed store hash on-chain. The bridge is bricked.

Recovery requires the admin key to call `updateStoreHash()` (`NoriTokenBridge.ts:686`) to manually set the store hash to a valid checkpoint-rooted store. But `update()` remains permissionless and the ZK program still accepts non-checkpoint slots, so the same party can immediately brick the bridge again after recovery.

Missed epoch boundary slots (where the block proposer for a slot at position 0 in the epoch fails to produce a block) occur frequently on mainnet. When this happens, the finalized header points to the last block before the boundary, which is a non-checkpoint slot. An attacker or negligent operator does not need to compromise any proposer; they simply wait for a naturally occurring missed boundary slot and submit a proof during that window.

### Commit 1 - Test exposing non-checkpoint slot acceptance

- **`nori-test-fixtures`** (`nori-test-fixtures/`): new workspace crate with a `generate_non_checkpoint_fixture` binary that captures a real `ProofInputs<MainnetConsensusSpec>` for a non-checkpoint finalized slot. Cold starts once via `get_latest_finality_slot_and_store_hash`, then chains `prepare_consensus_mpt_proof_inputs` with `validate=false`, feeding each output slot and store hash back as the next input until a non-checkpoint output slot is observed.
- **`non_checkpoint_proof_inputs.10650047.cbor`** (`nori/tests/data/`): captured fixture, slot 10650047 (% 32 == 31), 113807 bytes.
- **`1eb72_non_checkpoint_regression`** (`nori/tests/1eb72_non_checkpoint_regression.rs`): regression test that deserializes the fixture and passes it to `consensus_mpt_program`. Asserts the program rejects non-checkpoint slots.

Results:

- `1eb72_non_checkpoint_regression`: FAILED. `consensus_mpt_program` accepts slot 10650047 (% 32 == 31) without error, confirming the vulnerability. The program produces a valid `ProofOutputs` with a non-checkpoint `output_slot`, which could be submitted to `NoriTokenBridge.update()` to brick the bridge.

## 15/6/26 - Audit e4e27: `prepare_consensus_mpt_proof_inputs` reconstructs `ExecutionHttpProxy` from env on every window

### Finding (verbatim)

Finding e4e27: `prepare_consensus_mpt_proof_inputs` reconstructs `ExecutionHttpProxy` from env on every window

`ConsensusHttpProxy::prepare_consensus_mpt_proof_inputs` constructs a fresh `ExecutionHttpProxy` via `try_from_env()` on every invocation:

```rust
// nori-bridge-head/nori/src/rpcs/consensus/mod.rs
// Get Execution Proxy (Note this is a bit messy to do this here now FIXME)
let validated_consensus_mpt_proof_input_with_window = ExecutionHttpProxy::<S>::try_from_env()
    .prepare_consensus_mpt_proof_inputs(
        input_slot,
        output_slot,
        finalized_input_block_number,
        finalized_output_block_number,
        validated_consensus_proof_inputs,
        expected_output_store_hash
    )
    .await?;
```

This happens once per proving window (the function is the per-window orchestrator invoked from the finality change detector). As an optimization, I think you could construct the `ExecutionHttpProxy` once (e.g. own it as a field on `ConsensusHttpProxy`, or pass it in) and reuse it across windows?

### Response

Agreed. The FIXME comment on the line above the call site acknowledged this was untidy. The `from_env()` call is cheap (env var reads, URL parsing, HTTP provider construction, no network calls) and the cost per window is negligible, but reconstructing identical config on every invocation is unnecessary.

The suggested approach of owning `ExecutionHttpProxy` as a field on `ConsensusHttpProxy` was adopted. `ConsensusHttpProxy::from_env()` now also constructs the `ExecutionHttpProxy` and stores it as a field, so `prepare_consensus_mpt_proof_inputs` uses `self.execution_proxy` instead of calling `try_from_env()`.

While auditing all `from_env` call sites, the same pattern was found in `validate_and_prepare_proof_inputs_actor` (`finality_change_detector.rs`), where `ConsensusHttpProxy::try_from_env()` was called inside the job loop on every proving window. This was hoisted above the loop. The `ConsensusHttpProxy` is now constructed once in `api.rs` at startup alongside the `ProverConfig`, passed into `start_validated_consensus_finality_change_detector`, which passes it into the validation actor. No `from_env` calls remain in any loop or per-window path.

### Commit

- **`ConsensusHttpProxy`** (`nori/src/rpcs/consensus/mod.rs`): added `execution_proxy: ExecutionHttpProxy<S>` field to the struct, constructed in `from_env()`. `prepare_consensus_mpt_proof_inputs` now uses `self.execution_proxy` instead of `ExecutionHttpProxy::try_from_env()`, removing the FIXME.
- **`validate_and_prepare_proof_inputs_actor`** (`nori/src/bridge_head/finality_change_detector.rs`): changed signature to accept a `ConsensusHttpProxy` parameter instead of constructing one internally. Removed per-job `try_from_env()` calls from both the dual-window and solo-window branches.
- **`start_validated_consensus_finality_change_detector`** (`nori/src/bridge_head/finality_change_detector.rs`): changed signature to accept a `ConsensusHttpProxy` parameter. Uses it for the initial `get_latest_finality_slot()` call and passes it into the validation actor. Removed `MainnetConsensusSpec` and `HttpRpc` imports that are no longer needed.
- **`BridgeHead::run`** (`nori/src/bridge_head/api.rs`): constructs `ConsensusHttpProxy` once at startup alongside `ProverConfig` and passes it into the finality change detector.

## 15/6/26 - Audit 8ff57: `get_first_update` panics on empty updates

### Finding (verbatim)

Finding 8ff57: `get_first_update` panics on empty updates

`Client::get_first_update` fetches a single light client update for the current sync period and unconditionally indexes element 0 of the returned vector:

```rust
// nori/src/rpcs/consensus/mod.rs
pub async fn get_first_update(&self) -> Result<Update<S>> {
    let period = calc_sync_period::<S>(self.get_current_finalizer_header_beacon_slot());

    // Handling the result and converting errors to anyhow::Error
    let updates_result = self
        .inner
        .rpc
        .get_updates(period, 1)
        .await
        .map_err(|e| Error::msg(e.to_string())); // Convert error to anyhow::Error

    match updates_result {
        Ok(mut updates) => Ok(updates.get_mut(0).unwrap().clone()), // Clone the updates if the result is Ok
        Err(e) => Err(e), // Propagate error if it's an Err
    }
}
```

The `.map_err(...)` only captures RPC errors. A successful HTTP response carrying an empty updates array (`[]`) is `Ok(vec![])`, which flows straight into `updates.get_mut(0).unwrap()` and panics.

In contrast, the function `prepare_consensus_proof_inputs` handles the same call with proper error handling instead of panicking:

```rust
// nori/src/rpcs/consensus/mod.rs
let mut updates = client.get_updates().await?;

// Panic if our updates were empty (not sure how to deal with this yet)
if updates.is_empty() {
    return Err(anyhow::anyhow!("Error updates were missing 0th update."));
}
```

### Response

The period queried by `get_first_update` is the period of a slot that has already been finalized and bootstrapped from. The call chain is:

1. `bootstrap_from_slot` / `bootstrap_from_checkpoint` gives a `finalized_header` at some slot
2. `calc_sync_period` computes which sync committee period that slot falls in
3. `get_updates(period, 1)` asks the beacon node for light client updates from that period

A sync committee period on mainnet spans 8192 slots (~27 hours). The beacon API endpoint `/eth/v1/beacon/light_client/updates?start_period=P&count=1` returns the best `LightClientUpdate` the node has stored for period P. For a beacon node to have allowed a bootstrap from a finalized checkpoint in period P, that period must be completed or in progress with finality. And a period with finality is expected to produce light client updates, as they are derived from the finality attestations made by that period's sync committee. A beacon node that serves a bootstrap for a period but has zero light client updates for that same period would be unexpected in practice. The consensus spec and major client implementations (including helios, which is our dependency) expect at least one update per period:

- **Consensus spec** (v1.6.1, `specs/altair/light-client/full-node.md` line 171): "Full nodes SHOULD provide the best derivable `LightClientUpdate` ... for each sync committee period" ([link](https://github.com/ethereum/consensus-specs/blob/v1.6.1/specs/altair/light-client/full-node.md#L171-L172))
- **Lighthouse** (v8.1.3, `light_client_server_cache.rs` line 195-209): quotes the spec requirement verbatim in a comment and implements it by storing the best update per `sync_period` during block processing ([link](https://github.com/sigp/lighthouse/blob/v8.1.3/beacon_node/beacon_chain/src/light_client_server_cache.rs#L195-L209))
- **Helios** (0.11.1, `consensus.rs` line 460-463): `advance()` calls `get_updates(current_period, 1)` then `updates.get_mut(0).unwrap()`, expecting the 0th update to exist. Empty is treated as "nothing to do" rather than an error state ([link](https://github.com/a16z/helios/blob/0.11.1/ethereum/src/consensus.rs#L460-L463))

The only theoretical edge case would be querying a period that is so new that finality has not yet occurred within it, but that cannot happen here because the period is derived from an already-finalized header, not from wall clock time.

In practice, if one has bootstrapped from a finalized slot in period P, the beacon node will have at least one update for period P. The unwrap is technically unclean but not practically reachable.

`get_first_update` is not in the production path and was never planned to be. It is only called from `get_latest_finality_slot_and_store_hash`, which is the test cold start procedure. In production, the initial store hash is generated by a separate script (`nori/bin/generate_initial_store_hash.rs`, see branch `FEAT/inital-store-hash-script`) which mirrors `prepare_consensus_proof_inputs` directly and has its own empty updates guard. The production path (`prepare_consensus_proof_inputs`) already had the correct error handling before this finding was reported.

The bridge head runs containerised. This function was never planned to be in the production path, but if it were, a panic would result in a container restart with no impact on the wider system.

The fix is accepted regardless. Replacing the unwrap with a proper error return is the right thing to do for code quality and auditability.

### Commit

- **`get_first_update`** (`nori/src/rpcs/consensus/mod.rs`): replaced the `match` block containing `updates.get_mut(0).unwrap()` with a `map_err` providing a meaningful error message including the period number, an `is_empty()` guard returning `Err(anyhow!("Error updates were missing 0th update."))`, and `updates.get(0).unwrap().clone()` after the guard. The unwrap is now unreachable due to the empty check above it.
- **`prepare_consensus_proof_inputs`** (`nori/src/rpcs/consensus/mod.rs`): updated the comment above the existing `is_empty()` guard from "not sure how to deal with this yet" to "this shouldn't happen but this is defense in depth", and added a rationale comment explaining why empty updates are not expected.

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

### Commit 2 - Fix applied

- **`zeros[level]` corrected to `zeros[depth + 1 - level]`** (`nori-hash/src/merkle_poseidon_fixed.rs`): three sites patched in `fold_merkle_left` (line 137), `build_merkle_tree` (line 230), and `get_merkle_path_from_leaves` (line 327). When the tree-building loop is at a given `level` counting down from `depth`, the parent node of two dummy children represents an all-zero subtree of height `depth + 1 - level`. The corrected index selects the matching precomputed zero hash from `get_merkle_zeros`.

Results:

- Regression tests: 2 pass, 0 fail (`regression_a2090_bruteforce_reference`, `regression_a2090_recursive_reference`). All leaf counts [1, 3, 5, 6, 9, 17] now match both the brute-force and recursive references for both `build_merkle_tree` and `fold_merkle_left` (24 checks, 24 pass).
- Self-consistency (Rust): passes 0-50 leaves.
- Self-consistency (TypeScript non-provable): passes 0-50 leaves.
- Self-consistency (TypeScript provable): passes 0-10 leaves.
- Cross-reference (patched Rust vs patched TypeScript non-provable): 51 leaf counts, 0 leaf mismatches, 0 root mismatches.
- Cross-reference (patched Rust vs patched TypeScript provable): 51 leaf counts checked, 0 root mismatches, 40 leaf mismatches (all MISSING, provable suite only runs 0-10, no data exists for 11-50), 11 overlapping leaf counts all leaves and roots match.
- Cross-reference (patched TypeScript non-provable vs patched TypeScript provable): 51 leaf counts checked, 0 root mismatches, 40 leaf mismatches (all MISSING, provable suite only runs 0-10, no data exists for 11-50), 11 overlapping leaf counts all leaves and roots match.

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
