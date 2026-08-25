# Changelog

## 25/8/26 - Finding a3850: The committed deposit root is not constrained to be complete

### Finding (verbatim)

#### Description

To bridge from Ethereum to Mina, a user locks funds on the Ethereum `NoriTokenBridge`, which records the deposit in the cumulative mapping `lockedTokens[codeChallenge]` and emits a `TokensLocked` event. On the Mina side, `noriMint` lets that user mint, gated on the user's deposit being a member of a rolling window of up to `maxWindow` (32) verified deposit roots. Each root is committed to Mina by an `update` call carrying an SP1 proof whose output `verified_contract_storage_slots_root` (surfaced on Mina as `verifiedContractDepositsRoot`) is dispatched into that window.

The issue discussed in this finding stems centrally from the fact that this deposit root need not include all deposits. Instead, it is a freshly built Poseidon Merkle tree over whatever storage slots the prover supplies, and the SP1 guest verifies only the inclusion of those slots; it never enforces that the tree is complete.

The root is produced by `verify_storage_slot_proofs`. After verifying the contract account against the execution state root, it builds the tree purely from the supplied `contract_storage.storage_slots`, returning the all-zero hash when the prover supplies none:

```rust
// nori-bridge-head/nori-program/src/mpt.rs
// (source excerpt elided — see verify_storage_slot_proofs)
```

Each supplied slot is checked to exist under the contract's storage root, but nothing constrains what is omitted, so a proof carrying zero deposit slots, and hence the zero-hash root, is valid.

As `update` is permissionless, any party, not only the operator, can advance the bridge head with a valid proof carrying an empty or pruned deposit root.

#### Impact

**Active preemption of the mint path.**
Each `update` must advance the Ethereum slot (`latestHead`):

```ts
// nori-bridge-sdk/contracts/mina/src/NoriTokenBridge.ts
// (source excerpt elided — see the slot progress assertion in update)
```

Thus if an attacker frontruns the legitimate operator's update that would advance `latestHead` to \( N \) with an update advancing `latestHead` to \( N \) or a larger value, the operator's transaction will be rejected.
Note that construction of a proof with an empty deposit root will require less compute than a proof with a non-empty deposit root. Furthermore, the attacker can begin working on a proof that will update `latestHead` from \( N - 1 \) to \( N \) before the `update` that advances the on-chain state of `latestHead` to \( N - 1 \) got finalized on Mina. Indeed, if they anticipate that the next `update` will advance `latestHead` to \( N - 1 \) and slot \( N \) has been finalized on Ethereum, they can already begin computing the proof.
It thus appears plausible that a determined attacker can preempt legitimate updates and persist in landing `update` calls with empty deposit root on-chain, preventing the legitimate operator or other parties from placing non-empty deposit roots.

Under such a sustained attack, the mint path is denied to all users for as long as the attack continues. The funds are not permanently burned, however.
The Ethereum contract's `lockedTokens[codeChallenge]` is cumulative and never deleted: the deposit still exists in Ethereum storage at any later output block, and the guest checks each slot against the output block's storage root rather than the block at which it was locked.
Because of this, once the attacker stops, an unopposed self-rescue or the operator can drain the backlog. Thus the impact is an attacker-controlled outage of the mint path rather than irreversible loss.

**Passive omission.** Even without an attacker there is no guarantee that any given deposit is ever committed. A selectively censoring, buggy, or offline operator can omit a deposit, so that it is never mintable via updates submitted by the operator. The user's only recourse is a self-rescue run in which they race the operator with an `update` of their own that includes their deposit, a high bar for an ordinary user, though possible when unopposed.

**Data-availability dependency.** To mint against an update, a user needs the exact leaf set and order of that update's tree. Data availability is thus a point to consider, though out of scope for this audit.

#### Recommendations

The root cause is that the committed deposit root is not required to encompass all intended deposits as reflected by the Ethereum state. The fix is to enforce such a requirement in the SP1 guest program, so that any valid `update` must carry the deposits that Ethereum state records.

How to adjust the design of the guest program and potentially also the Ethereum contract is a significant design question. There is however one concrete footgun to be aware of. Completeness must be enforced incrementally rather than all-at-once. A rule of "include everything since the last update" is itself a denial-of-service vector: by spamming cheap locks, an attacker can either exceed the Merkle tree's \( 2^{16} \)-leaf limit for a single update, or produce deposits faster than the operator can prove them.

### Response

The finding is correct and the recommendation is accepted. The guest no longer accepts a prover-chosen set of storage slots. The deposit set is now an on-chain, append-only **proof request queue** on Ethereum (`NoriProofRequestQueue`), which the guest is required to drain in order.

`verify_storage_slot_proofs` is replaced by `verify_queue`. The queue holds a monotonic `head` counter and a mapping of numbered request records; the destination chain holds a `queueCursor` recording how many requests have been settled. From those two values the batch is fully determined:

- the start is the cursor, which the destination chain asserts against its own stored value,
- `head` is read out of the queue's storage by Merkle-Patricia proof against the execution state root the same proof commits,
- the size is `min(head - cursor, MAX_BATCH)`, and a witness whose entry count disagrees is rejected with `BatchSizeMismatch`,
- each entry's five storage words are located from its index by Solidity's mapping and struct-member rules, not read from the witness.

Nothing about the batch is supplied by the prover, so an empty or pruned root cannot be produced. The all-zero root is reachable only when `head == cursor`, and that equality is itself proven — at genesis by an exclusion proof that the head slot is absent from the storage trie.

Completeness is total rather than best-effort: every entry in the range contributes exactly one leaf. A populated slot is pinned by an inclusion proof of its true value; an empty slot, or a target account that does not exist, by an exclusion proof, contributing a leaf with value zero. In a Merkle-Patricia trie "holds zero" and "absent from the trie" are the same fact, and for a given root and key exactly one of the two proofs can verify, so the claim is pinned in either direction. A junk request therefore proves as zero and the queue advances past it — it can neither stall the queue nor be silently dropped. A proof that verifies as neither aborts the entire run: a committable failure outcome would reintroduce exactly the omission this finding describes.

This also addresses the footgun raised in the recommendation. Completeness is enforced **incrementally**, not all-at-once: a batch is the outstanding backlog, capped at `MAX_BATCH` (2^16, the Merkle tree's leaf capacity), and anything beyond that cap drains across consecutive updates, in order, with no entry skipped.

Forcing a large batch is bounded by what enqueueing costs. The queue drains every update window, so a backlog only builds within one window, and each `requestProof` writes three to five cold storage words and pays `proofRequestQueueFee` on top, with the fees accruing to the protocol. Reaching the cap inside a window would take several times the blockspace that window has, so the cap bounds the tree rather than describing a state an attacker can force. A larger batch costs proving time and fees; it cannot leave the queue stuck.

On the preemption scenario, a frontrunner must resume from the same cursor and carry the same entries, so a competing proof can no longer advance the head while omitting deposits. On the data-availability point the audit places out of scope, the queue is append-only and never deleted, so the leaf set of any committed batch is reconstructible from Ethereum state by any party; the proof pipeline additionally publishes the verified request set in cursor order.

### Changes

- **`verify_queue`** (`nori-program/src/mpt.rs`): replaces `verify_storage_slot_proofs`. Takes the execution state root and the queue witness, and returns the output cursor and the requests root. Verifies the queue account, then the `head` slot, then bounds the cursor, then derives the batch, then each entry's five words at index-derived locations, then each distinct target account and the requested slot beneath it, and finally folds one leaf per entry into the Merkle tree.
- **Inclusion / exclusion handling** (`nori-program/src/mpt.rs`): `verify_storage_word` selects the required proof from the claimed value — a non-zero value requires an inclusion proof of its RLP encoding, a zero value requires an exclusion proof. `verify_account` takes `Option<TrieAccount>`: `Some` requires an inclusion proof and promotes the account's storage root, `None` requires an exclusion proof and yields `EMPTY_ROOT_HASH`, under which only zero-valued slot proofs can verify.
- **Error surface** (`nori-program/src/mpt.rs`): `MptError` now carries `CursorAheadOfHead`, `BatchSizeMismatch`, `MissingTargetWitness` and `MissingSlotWitness` for the queue-specific failures. Account-proof failures are branded by a `ProvenAccount` selector into `InvalidProofRequestQueueAccountProof` and `InvalidTargetAccountProof`, so a failure identifies which account it concerns without comparing addresses.
- **Queue witness types** (`nori-primitives/src/types.rs`): `QueueStorage`, `QueueEntryProof`, `TargetStorageProof`, `TargetSlotProof` and `VerifiedRequest` added; `ProofInputs.contract_storage` replaced by `queue_storage`. `StorageSlot`, `ContractStorage` and `VerifiedContractStorageSlot` are removed.
- **Storage layout module** (`nori-primitives/src/storage_layout.rs`): new home for the queue layout constants (`QUEUE_HEAD_STORAGE_INDEX`, `QUEUE_REQUESTS_STORAGE_INDEX`, `QUEUE_ENTRY_WORDS`, `MAX_COLLECTION_KEYS`) and the Solidity slot arithmetic (`storage_slot_of_index`, `mapping_entry_location`, `struct_word_slot`, `word_of_address`, `word_of_b256`). These are shared by the guest, which derives the keys it verifies, and the host, which fetches those same keys — so there is one definition per rule rather than two that can drift.
- **Public outputs** (`nori-primitives/src/types.rs`): `ProofOutputs` gains `input_queue_cursor`, `output_queue_cursor` and `output_block_number`; `contract_address` becomes `proof_request_queue_address` (the proof now anchors on the queue, not the bridge) and `verified_contract_storage_slots_root` is renamed `verified_requests_root`. With the `genesis_root` removal recorded separately under Finding d3034, the encoding moves from 228 to 220 bytes. `output_block_number` is committed so that any party can identify the block every storage read was taken at and reconstruct the batch independently.
- **Leaf format** (`nori-hash/src/merkle_poseidon_fixed.rs`): `hash_request_leaf` replaces `hash_storage_slot`, packing 117 bytes into four field elements — `target` (20 bytes) together with `collection_keys_count` and the leading byte of each collection key and of the value, then the three 31-byte tails. The count is hashed so that an unused trailing key, which is zero, cannot collide with a request that supplied a zero key. Namespacing each leaf by `target` is what allows a consumer to reject leaves enqueued by a different contract. `MAX_BATCH` lives alongside `MAX_TREE_DEPTH`, from which it derives.
- **Guest wiring** (`nori-program/src/consensus.rs`): `consensus_mpt_program` calls `verify_queue` and commits the queue address, both cursors and the requests root. `execution_state_root` and `output_block_number` are read from a single `store.finalized_header.execution()` call on the post-apply finalized header, so the committed pair cannot disagree about which block the storage reads describe.
- **Host witness assembly** (`nori/src/rpcs/execution/http.rs`): `_prepare_consensus_mpt_proof_inputs` no longer scans `TokensLocked` events to choose slots. It reads `head`, derives the batch and every entry location, then issues one `eth_getProof` against the queue covering the head slot and all entry words, and one per distinct target address. Every read is pinned to the window's output block. An empty batch costs two RPC calls and still proves `head == cursor`. Account existence is decided by verifying the inclusion proof and then the exclusion proof locally rather than inferring it from the response shape, because clients disagree on how an absent account is represented.
- **RPC resilience** (`nori/src/rpcs/execution/http.rs`): `NORI_EXECUTION_CHUNK_LIMIT` (default 100) caps how many storage keys go into a single `eth_getProof`; requests are chunked and retried with exponential backoff, replacing a hardcoded chunk size.
- **Cursor plumbing** (`nori/src/bridge_head/`): the queue cursor is carried end to end — `BridgeHead` tracks it, `advance` and `run` take it, `AdvanceMessage` and the checkpoint persist it (with a `serde` default so checkpoints written before the queue still load), and the started, job-created, job-succeeded, job-failed and advanced notices all report it. `ProofMessage` and the job-succeeded notice additionally publish `verified_requests`, the committed leaf set in cursor order.
- **Environment** (`nori/src/contract.rs`, `.env.example`, `README.md`): `NORI_TOKEN_BRIDGE_ADDRESS` becomes `NORI_PROOF_QUEUE_ADDRESS`, since the address the proof anchors on is now the queue. The event-scanning helpers and the `nori-contract-bindings` crate they were the last consumer of are deleted.
- **Guest artifact** (`nori-elf/`): the ELF, `vk.json` and `pi0.json` are regenerated for the guest changes recorded here and under Finding d3034 together. The destination chain pins pi0 in `noriHeliosProgramPi0`, so shipping these changes requires the admin-gated update d3034 documents.
- **Cross-language test vectors** (`test-vectors/proof-request-queue/`): three fixtures rendered by Rust tests and vendored into the SDK — the Poseidon leaf packing, the public-output byte encoding, and the storage locations of `head` and of each entry's five words. These three encodings must agree bit-for-bit between the SP1 guest and the Mina circuit, so they are pinned rather than reimplemented independently on each side.

### Results

- Guest: **40 passing** — `cargo test -p nori-sp1-helios-program --lib`. `nori-hash`: **20 passing** — `cargo test -p nori-hash` (14 unit, 1 integration, 5 doctests).
- Adversarial coverage for `verify_queue` (`nori-program/src/mpt.rs`) is built on fixture tries constructed in-process with `alloy-trie`, requiring no network. It covers the cases where the distinction matters: a populated slot claimed as zero, an empty slot claimed non-zero, an empty slot correctly claimed zero, zero-valued entry words, an absent target account claimed absent, a live account claimed absent, an absent account with a non-zero claim, and the genesis case of `head == 0` yielding an empty batch and the zero root. Batch derivation is covered for a cursor ahead of head and for an entry count that disagrees with the derived batch, along with duplicate-slot deduplication.
- Exclusion proofs are exercised across all three trie shapes an absence can take — an empty branch child, another key's leaf, and a diverging extension node — the last being a historically error-prone corner of Merkle-Patricia verifiers. Each test asserts the node shape its fixture actually produces, so a change to the fixture keys fails rather than silently dropping a shape from the coverage.
- The leaf packing is additionally verified byte by byte: each of the 117 payload bytes is routed through `pack_request_leaf_fields` and checked against the documented field layout by independent arithmetic, so a transposed byte cannot pass unnoticed.

## 25/8/26 - Finding d3034: Compromised admin is not prevented from minting or unlocking illegitimately

### Finding (verbatim)

#### Description

The Mina bridge is designed to hold up even against a compromised or malicious governance: a compromised admin should not be able to mint tokens that are not backed by real Ethereum deposits. The project describes the intended assumptions as follows:

> Using AdminKey (multisig), [the admin account] can update the values here: [...]. Worst case, [the admin] could maliciously set these to wrong values and "brick" the bridge. The Ethereum smart contract address is set at deployment and can't be modified, and since the Solidity contract is not upgradable, [they] can't create fake locks, so [..] shouldn't be able to mint tokens maliciously on the Mina side.
> In the code, this intent is also stated, most explicitly in the doc comment on the immutable `genesisRoot` field, which singles out one way a compromised governance could otherwise mint unbacked tokens --- redirecting the bridge to an attacker-controlled chain --- and aims to prevent it:

```ts
// nori-bridge-sdk: contracts/mina/src/NoriTokenBridge.ts
// (doc comment on the immutable genesisRoot field elided)
```

The claimed guarantee is that, although the store hash must stay admin-upgradable and is opaque, `update` checks the genesis validators root emitted by each proof against the immutable `genesisRoot`, so a rotated store hash cannot point the bridge at a different chain.

This protection does not work: the genesis root is not an anchor to the real chain. The bridge-head guest program performs an incremental store-to-store transition and never verifies that the store descends from genesis. The store, the genesis root, and the previous store hash are all witness inputs; the only check binding the store is its hash against the chained value:

```rust
// nori-bridge-head: nori-program/src/consensus.rs (consensus_mpt_program)
// (source excerpt elided — witness destructuring, store hash chain check,
//  verify_update / verify_finality_update calls, and the ProofOutputs packing)
```

Both `verify_update` and `verify_finality_update` only pass `genesis_root` onwards to `verify_generic_update`, where the genesis root is used purely to derive the BLS signing domain.

The real root of trust is therefore the store-hash chain, seeded once at deploy. The genesis root only fixes the signing domain; it does not establish that the store's sync committees or headers belong to the real Ethereum chain. That trust would normally come from bootstrapping the store from a trusted checkpoint, which this program does not do --- it accepts the store as an unauthenticated input.

**Store-hash rotation.**

A compromised admin can weaponize this directly, using only the power the comment considers safe. `updateStoreHash` sets the on-chain store hash to any value on an admin signature:

```ts
// nori-bridge-sdk: contracts/mina/src/NoriTokenBridge.ts
// (source excerpt elided — see updateStoreHash)
```

A compromised admin generates their own BLS sync committee, builds a `LightClientStore` containing it together with a fabricated finalized header carrying an attacker-chosen execution state root, and sets the on-chain store hash to that store's hash. They then submit an `update` whose finality update (and any sync-committee updates) are signed by the fabricated committee, using the genuine `genesis_root` for the domain. As the attacker controls the fabricated committee's keys, they can produce the required signatures, and can thus produce a proof for the guest program with arbitrary deposit root contents. Using such a proof in an `update` transaction, the on-chain `genesisRoot` check will be passed, and `latestVerifiedContractDepositsRoot` updated to contain the fake deposits of their choosing. This is precisely the redirect to a fake chain that `genesisRoot` immutability was meant to prevent.

**Program and recursion verification key swap.**

The genesis-root failure is not even required for a takeover. Separately from it, the admin holds powers that were presumably not meant to allow unbacked minting but do. One is that the identity of the accepted guest program is itself admin-mutable, giving a more direct route:

```ts
// nori-bridge-sdk: contracts/mina/src/NoriTokenBridge.ts
// (source excerpt elided — see updateNoriHeliosProgramPi0 and updateProofConversionPO2)
```

In `ethVerify` the converted proof is verified against a fixed verification key, which is intended to be for the `node` compressor circuit in `src/compressor/compressor.ts` in `proof-conversion`. However, this circuit bakes in neither the verification keys it recursively verifies proofs against, nor which guest program the SP1 proof is ultimately verified against. Only commitments are exposed, which need to be pinned on-chain. This is done against `noriHeliosProgramPi0` for pinning the guest program and against `proofConversionPO2` for pinning the recursion verification keys.

An admin can change both commitments. Changing either lets the on-chain checks accept a constraint-less recursion circuit or guest program, allowing a compromised admin to produce accepted `update`s carrying deposit roots with arbitrary contents.

By any of these routes a compromised admin can produce an `update` carrying a deposit root with contents of their choosing, then use `noriMint` to mint an arbitrary amount, for the invented deposits, to an address they control. Those tokens could then be withdrawn on the Ethereum side using the usual mechanism, draining the bridge.

This list is not exhaustive: a compromised admin may have further avenues to mint themselves illegitimate tokens, for instance by changing the `FungibleToken` verification key.

**Ethereum side.**

The same structural pattern exists on the Ethereum bridge. The function `unlockTokens` trusts two contracts for verification, `stateSettlement` and `accountValidation`, and `setAlignedContracts` lets the operator repoint the bridge at replacements, which could include contracts that approve any input, which would then allow the compromised admin to drain the locked pool.

#### Impact

The intention that a compromised admin should not be able to drain the bridge on the Ethereum side, or mint illegitimately on the Mina side, but at worst can only brick the bridge, is not achieved by the current design. The admin account/contract has several independent avenues to drain the bridge.

#### Recommendations

Document for the project and for users what trust assumptions are made regarding the admin. To achieve the intended threat model, in which the admin account/contract should not be able to mint/unlock illegitimately, design changes are necessary.

### Response

The finding is correct, including that the list is not exhaustive. We are taking the first of the two recommended paths: documenting the trust assumptions that govern the admin role.

Three on-chain commitments have to remain updatable for the bridge to stay operable. `noriHeliosProgramPi0` and `proofConversionPO2` pin the accepted guest program and the recursion verification keys; the guest program changes as the Helios light client evolves, and the conversion keys rotate on major SP1 upgrades. `latestHeliusStoreInputHash*` pins the Helios store, which must be replaceable because the store serialization can change and an extended finality failure on Ethereum would require a forced store reconstruction. Freezing any of the three would turn a routine upgrade into a redeployment and state migration of the whole bridge.

Each of those commitments is also, as the finding sets out, a route by which a compromised admin could have an `update` accepted that carries a deposit root of their choosing, and mint against it. `setAlignedContracts` on the Ethereum side follows the same pattern for the unlock path. The bridge consequently does not guarantee that a compromised admin can only halt it; it can mint unbacked tokens and drain the locked pool.

What constrains the role is procedural rather than cryptographic. The admin is a multisig fronting a `TimelockController`, so a change to any of these values is published on-chain and subject to the timelock delay before it takes effect. That delay is the security parameter, and it is the window in which users can act on a change they consider hostile. This is the assumption we are documenting rather than a guarantee we are claiming.

Two admin surfaces named in the finding are not live routes to minting. `updateVerificationKey` cannot succeed: the bridge account is deployed with `setVerificationKey` set to `impossibleDuringCurrentVersion()` and `setPermissions` set to `impossible()`, so the account's own verification key is fixed for the current protocol version and the permission cannot be relaxed. `canChangeAdmin` returns `Bool(false)`, and `canMint` is gated on the single-use `mintLock` flag that `noriMint` clears immediately before calling `token.mint`, not on an admin signature. The `FungibleToken` verification key is governed by that token's own deploy-time `allowUpdates` setting.

`genesis_root` is a guest input. It is passed to `verify_update` and `verify_finality_update`, which use it to derive the BLS signing domain; it is not compared against the store, and the store is bound instead by the store-hash chain. It is not committed as a public output, and the corresponding on-chain field and assertion are removed.

### Changes

- **`ProofOutputs`** (`nori-primitives/src/types.rs`): `genesis_root` removed from the committed outputs and from `to_bytes`/`from_bytes`. Together with the queue changes recorded under Finding a3850, the encoding moves from 228 to 220 bytes.
- **`ConsensusProofOutputs`** (`nori-primitives/src/types.rs`): `genesis_root` removed and `output_block_number` added, mirroring `ProofOutputs`; the encoding moves from 176 to 152 bytes.
- **`consensus_program` and `consensus_mpt_program`** (`nori-program/src/consensus.rs`): neither commits `genesis_root`. Both continue to accept it as an input and pass it to `verify_update` and `verify_finality_update` for the BLS signing domain. The doc blocks and output tables are updated to match.
- **Downstream messages** (`nori/src/bridge_head/api.rs`, `notice_messages.rs`): `genesis_root` dropped from `ProofMessage` and from the job-succeeded transition notice, since it is no longer a proof output.
- **Documentation**: the guest doc blocks in `nori-program/src/consensus.rs` describing the committed outputs no longer list `genesis_root`. The admin trust assumptions set out in the response above are recorded in this entry, which is the reference for them until they are carried into the deployment documentation.

### Results

- `nori-primitives`: **6 passing** — `cargo test -p nori-sp1-helios-primitives`. The public-output encoding is covered by a round-trip test and by an offset test asserting the byte positions the destination chain's verifier reads, both of which move with the layout. The shared byte-encoding vectors are rendered by a Rust test and asserted by the Mina-side circuit, so a divergence between the two implementations fails on one side rather than producing proofs that verify inconsistently.
- No behavioural test accompanies the trust statement itself; it records an accepted assumption rather than a code constraint.

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

### Commit 2 - Fix applied

The constraint is enforced against `output_slot`, i.e. `store.finalized_header.beacon().slot` as it stands after `consensus_mpt_program` has finished applying `updates` and `finality_update`, since `output_slot` is the value committed into `ProofOutputs` and ultimately written on-chain by `NoriTokenBridge.update()`. `store.finalized_header` is not set from a single, fixed source: both `apply_update` (called once per entry in `updates`) and `apply_finality_update` (called for `finality_update`) route through the same helios function, `apply_generic_update` (`helios ethereum/consensus-core/src/consensus_core.rs`), which first decides whether the update qualifies at all:

```rust
// helios: ethereum/consensus-core/src/consensus_core.rs, apply_generic_update
let should_apply_update = {
    let has_majority = committee_bits * 3 >= S::sync_committee_size() * 2;
    let update_is_newer = update_finalized_slot > store.finalized_header.beacon().slot;
    let good_update = update_is_newer || update_has_finalized_next_committee;
    has_majority && good_update
};
if should_apply_update {
    apply_update_no_quorum_check(store, update);
}
```

Only if `should_apply_update` is true does it call `apply_update_no_quorum_check`, which re-checks slot progression before actually writing the field:

```rust
// helios: ethereum/consensus-core/src/consensus_core.rs, apply_update_no_quorum_check
if update_finalized_slot > store.finalized_header.beacon().slot {
    store.finalized_header = update.finalized_header.clone().unwrap();
}
```

This is the only line in helios that assigns `store.finalized_header`. `verify_update`/`verify_finality_update` establish that an update is cryptographically valid; neither establishes that `should_apply_update` or this second slot check will hold for it. So whether processing a given entry in `updates`, or `finality_update`, changes `store.finalized_header` at all, and to what, depends on the store's finalized slot as left by whatever was processed immediately before it. After both processing steps, `store.finalized_header.beacon().slot` can therefore end up matching any one of `updates`, `finality_update`, or `input_slot` unchanged, none of which `consensus_mpt_program` can determine ahead of time. That is why the check reads `output_slot` and asserts `output_slot % 32 == 0` only in step 7, after step 6 has captured `store.finalized_header.beacon().slot` post-apply.

- **`ProgramError`** (`nori-program/src/consensus.rs`): added `NonCheckpointOutputSlot { slot: u64 }` variant with display format showing the slot and its `% 32` remainder.
- **`consensus_mpt_program`** (`nori-program/src/consensus.rs`): added step 7 Checkpoint Slot Validation, checking `output_slot % 32 != 0` and returning `NonCheckpointOutputSlot`, immediately after `output_slot` is captured in step 6 (post-apply). Docstring operations and error-condition lists renumbered and updated accordingly.
- The check is not added to `consensus_program` because it is only used as an off-chain dry run in `prepare_consensus_mpt_proof_inputs`, gated by the `validate` flag. The existing off-chain `% 32` check in `mod.rs` already handles the `validate=true` case. `consensus_mpt_program` is the ZK circuit entrypoint (`nori-program/src/main.rs`), so enforcing the constraint there makes it impossible to produce a valid proof for a non-checkpoint slot.

Results:

`cargo test -p nori --test 1eb72_non_checkpoint_regression`

- `1eb72_non_checkpoint_regression`: PASSED. `consensus_mpt_program` rejects slot 10650047 (% 32 == 31) with `NonCheckpointOutputSlot` error.

This test is not sufficient proof that the check is anchored to the right value. Its fixture has an empty `updates` array, so `finality_update`'s own (pre-apply) slot and the true post-apply `output_slot` are numerically identical (10650047 in both), and the test cannot tell a check on one from a check on the other. A fixture that could tell them apart would need an `updates[i]` entry that advances `store.finalized_header` to a non-checkpoint slot, followed by a `finality_update` that fails to override it.

Sepolia's live missed-slot rate is about 3.3% per epoch boundary (measured via `light-sepolia.beaconcha.in`, 50 missed slots over 1505 slots), which is what let the existing fixture be captured in about 3.2 hours of polling. But `updates` is only non-empty when a proof crosses a sync-committee period boundary (8192 slots, about 27.3 hours), so on average about 13.65 hours pass before there is even an `updates` entry to look at, and getting that entry's own finalized slot to land on non-checkpoint needs roughly 30 such boundaries at the same 3.3% rate, around 34 days of continuous polling. On top of that the update still has to go unoverridden by the corresponding `finality_update`, which depends on operational conditions (which of `all_providers_urls` a call to `multiplex` lands on, node lag, timing skew between the `get_updates` and `finality_update` calls) rather than slot-miss statistics, and we have no measured rate for it. Given all of that, we are documenting this as a known gap rather than waiting for it.

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
