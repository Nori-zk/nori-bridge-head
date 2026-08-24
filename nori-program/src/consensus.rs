use alloy_primitives::B256;
use helios_consensus_core::{
    apply_finality_update, apply_update, consensus_spec::ConsensusSpec, verify_finality_update,
    verify_update,
};
use log::debug;
use nori_hash::sha256_hash::sha256_hash_helios_store;
use nori_sp1_helios_primitives::types::{
    ConsensusProofInputs, ConsensusProofOutputs, ProofInputs, ProofOutputs,
};
use std::fmt;
use tree_hash::TreeHash;

use crate::mpt::{verify_queue, MptError};

/// Custom error type for program execution failures
#[derive(Debug)]
pub enum ProgramError {
    /// Error when the calculated hash doesn't match the provided hash
    HashChainMismatch { expected: B256, actual: B256 },
    /// Error when an update verification fails
    InvalidUpdate { index: usize, reason: String },
    /// Error when finality update verification fails
    InvalidFinalityUpdate { reason: String },
    /// Error when execution root is missing
    MissingExecutionRoot,
    /// Error when store hashing fails
    StoreHashingError(String),
    /// Error for MPT specific errors
    MptError(MptError),
    /// Error when output_slot is not a checkpoint slot (output_slot % 32 != 0)
    NonCheckpointOutputSlot { slot: u64 },
}

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProgramError::HashChainMismatch { expected, actual } => {
                write!(
                    f,
                    "Hash chain mismatch: expected {:?}, got {:?}",
                    expected, actual
                )
            }
            ProgramError::InvalidUpdate { index, reason } => {
                write!(f, "Invalid update at index {}: {}", index, reason)
            }
            ProgramError::InvalidFinalityUpdate { reason } => {
                write!(f, "Invalid finality update: {}", reason)
            }
            ProgramError::MissingExecutionRoot => {
                write!(f, "Missing execution root in proof inputs")
            }
            ProgramError::StoreHashingError(reason) => {
                write!(f, "Failed to hash store: {}", reason)
            }
            ProgramError::MptError(e) => {
                write!(f, "MPT error: {}", e)
            }
            ProgramError::NonCheckpointOutputSlot { slot } => {
                write!(
                    f,
                    "Output slot {} is not a checkpoint slot (% 32 == {})",
                    slot,
                    slot % 32
                )
            }
        }
    }
}

// Conversion from MptError to ProgramError
impl From<MptError> for ProgramError {
    fn from(error: MptError) -> Self {
        ProgramError::MptError(error)
    }
}

impl std::error::Error for ProgramError {}

/// Zero-Knowledge Consensus State Transition Proof for Ethereum Light Client Updates with Result type
///
/// Cryptographic state machine processing light client updates with hash chaining.
/// This program takes encoded inputs containing light client updates (sync committee updates and a finality update),
/// verifies their validity against the current state, applies the updates, and outputs the new state commitments.
/// Enforces strict hash-chain validation between successive states.
///
/// # Critical Path
/// ```text
/// prev_store_hash → process updates → new_store_hash
///      │               │              │
///      │               │              │
///      └               ┤              ┘
/// Initial State     Updates       Final State
///     Validation                 Commitment
/// ```
///
/// # Inputs (All Values Must Be Precomputed Hashes)
/// | Name                    | Type               | Description                       |
/// |-------------------------|--------------------|-----------------------------------|
/// | `updates`               | `Vec<Update>`      | Ordered sync committee updates    |
/// | `finality_update`       | `FinalityUpdate`   | Finalized header proof            |
/// | `expected_current_slot` | `u64`              | Current chain slot for validation |
/// | `store`                 | `LightClientStore` | Full client state                 |
/// | `genesis_root`          | `B256`             | Genesis block root                |
/// | `forks`                 | `ForkData`         | Network fork versions             |
/// | `store_hash`            | `B256`             | SHA-256(store) from last proof    |
///
/// # Operations (In Exact Execution Order)
/// 1. **Last Store Hash Validation** (Irreversible Check)
///    - Calculate last store hash: `SHA-256(serde_serialize(store))`
///    - Assert equality: `calculated_prev_store_hash == input_store_hash`
///
/// 2. **State Capture** (Pre-Update Snapshot)
///    - Record `input_slot` = `store.finalized_header.beacon().slot`
///
/// 3. **Update Processing** (Sequential, Atomic)
///    - Verify and apply each sync committee update in `updates`, if any
///
/// 4. **Finality Proof** (Header Finalization)
///    - Verify and apply `finality_update`
///
/// 5. **State Capture** (Post-Update Snapshot)
///    - Record `output_slot` = `store.finalized_header.beacon().slot`
///    - Extract `next_sync_committee_hash` = `store.next_sync_committee.tree_hash_root()`
///      (`B256::ZERO` if `next_sync_committee` is `None`)
///    - Extract `execution_state_root` = `store.finalized_header.execution()?.state_root()`
///      (fails with `MissingExecutionRoot` if execution header is absent)
///    - Extract `output_block_number` = `store.finalized_header.execution()?.block_number()`
///
/// 6. **Post-State Hashing**
///    - Compute `output_store_hash` = `SHA-256(serde_serialize(store))`, to be validated in the next round
///
/// 7. **Output Commitment**
///    - Pack `ConsensusProofOutputs` committing: `input_slot`, `input_store_hash`,
///      `output_slot`, `output_store_hash`, `execution_state_root`,
///      `next_sync_committee_hash`, `output_block_number`
///
/// # Outputs (All Values Are Hash Commitments)
/// | Field                      | Type   | Description                                     |
/// |----------------------------|--------|-------------------------------------------------|
/// | `input_slot`               | `u64`  | Slot before updates                             |
/// | `input_store_hash`         | `B256` | Input store hash                                |
/// | `output_slot`              | `u64`  | Slot after updates                              |
/// | `output_store_hash`        | `B256` | Updated store hash                              |
/// | `execution_state_root`     | `B256` | Execution layer state root                      |
/// | `next_sync_committee_hash` | `B256` | Hash of the next sync committee state (or zero) |
/// | `output_block_number`      | `u64`  | Execution block number of finalized output      |
///
/// # Error Conditions
/// 1. **Store Hashing Error**
///    `sha256_hash_helios_store` fails → `StoreHashingError` (steps 1 and 6)
/// 2. **Hash Chain Break**
///    `calculated_prev_store_hash != input_store_hash` → Invalid initial state
/// 3. **Invalid Update**
///    Any `verify_update` returns error → Malformed or fraudulent update
/// 4. **Invalid Finality**
///    `verify_finality_update` fails → Unverifiable final header
/// 5. **Missing Execution Root**
///    `store.finalized_header.execution()` is `Err` → Incomplete header data
pub fn consensus_program<S: ConsensusSpec>(
    proof_inputs: ConsensusProofInputs<S>,
) -> Result<ConsensusProofOutputs, ProgramError> {
    // Unpack inputs
    let ConsensusProofInputs {
        updates,
        finality_update,
        expected_current_slot,
        mut store,
        genesis_root,
        forks,
        store_hash: input_store_hash,
    } = proof_inputs;

    // 1. Last Store Hash Validation - Calculate SHA-256(serde_serialize(store)) and assert equality with input_store_hash
    debug!("Hashing last store state and comparing with proof inputs store hash.");
    let calculated_prev_store_hash = sha256_hash_helios_store(&store)
        .map_err(|e| ProgramError::StoreHashingError(format!("Failed to hash store: {}", e)))?;
    if calculated_prev_store_hash != input_store_hash {
        return Err(ProgramError::HashChainMismatch {
            expected: input_store_hash,
            actual: calculated_prev_store_hash,
        });
    }
    debug!(
        "Last store state hash is valid: {}",
        calculated_prev_store_hash
    );

    // 2. State Capture (Pre-Update Snapshot) - Record input_slot
    debug!("Capturing input_slot.");
    let input_slot = store.finalized_header.beacon().slot;
    debug!("input_slot captured.");

    // 3. Update Processing - Verify and apply sync committee updates, if any
    for (index, update) in updates.iter().enumerate() {
        // update.finalized_header.beacon().slot; introduce printing this so we can see if we are applying updates beyond our head
        let finalized_beacon_slot = { update.finalized_header().beacon().slot };
        debug!(
            "Processing update {} of {}. Update beacon finalized slot: {}",
            index + 1,
            updates.len(),
            finalized_beacon_slot
        );
        if let Err(err) = verify_update(update, expected_current_slot, &store, genesis_root, &forks)
        {
            return Err(ProgramError::InvalidUpdate {
                index,
                reason: format!("{:?}", err),
            });
        }
        debug!("Update {} is valid.", index + 1);
        apply_update(&mut store, update);
        debug!("Applied update {}.", index + 1);
    }

    // 4. Finality Proof - Verify and apply finality update
    debug!("Processing finality update.");
    if let Err(err) = verify_finality_update(
        &finality_update,
        expected_current_slot,
        &store,
        genesis_root,
        &forks,
    ) {
        return Err(ProgramError::InvalidFinalityUpdate {
            reason: format!("{:?}", err),
        });
    }
    debug!("Finality update is valid.");
    apply_finality_update(&mut store, &finality_update);
    debug!("Applied finality update.");

    // Should do an assertion here to ensure we have increased our head (we do check this downstream later)

    // 5. State Capture (Post-Update Snapshot)
    debug!("Capturing output_slot, next_sync_committee_hash and execution_state_root.");
    let output_slot = store.finalized_header.beacon().slot;
    let next_sync_committee_hash: B256 = match &mut store.next_sync_committee {
        Some(next_sync_committee) => next_sync_committee.tree_hash_root(),
        None => B256::ZERO,
    };
    let execution_state_root_result = store.finalized_header.execution();
    if execution_state_root_result.is_err() {
        return Err(ProgramError::MissingExecutionRoot);
    }
    let execution = execution_state_root_result.unwrap();
    let execution_state_root = *execution.state_root();
    let output_block_number = *execution.block_number();
    debug!("output_slot, next_sync_committee_hash, execution_state_root and output_block_number captured.");

    // 6. Post-State Hashing - Calculate updated store hash to be validated in the next round
    debug!("Hashing updated store.");
    let output_store_hash = sha256_hash_helios_store(&store).map_err(|e| {
        ProgramError::StoreHashingError(format!("Failed to hash updated store: {}", e))
    })?;
    debug!("Hashing updated store complete: {}", output_store_hash);

    // 7. Output Commitment
    debug!("Committing outputs.");
    let proof_outputs = ConsensusProofOutputs {
        input_slot,
        input_store_hash,
        output_slot,
        output_store_hash,
        execution_state_root,
        next_sync_committee_hash,
        output_block_number,
    };
    debug!("Packed outputs.");

    Ok(proof_outputs)
}

/// Zero-Knowledge Consensus State Transition Proof with MPT proof-request-queue verification for Ethereum Light Client Updates with Result type
///
/// Extends the basic consensus program by verifying Merkle Patricia Trie (MPT) proofs of the proof request queue
/// against the finalized execution state root. Ensures that the queue is consistent with the execution state.
///
/// # Critical Path
/// ```text
/// prev_store_hash → process updates → verify queue proofs → new_store_hash
///        │                 │                   │                   │
///        │                 │                   │                   │
///        └                 ┤                   ┴                   ┘
///  Initial State        Updates          Queue Proofs         Final State
///   Validation          Applied            Verified           Commitment
/// ```
///
/// # Inputs (All Values Must Be Precomputed Hashes)
/// | Name                    | Type               | Description                            |
/// |-------------------------|--------------------|----------------------------------------|
/// | `updates`               | `Vec<Update>`      | Ordered sync committee updates         |
/// | `finality_update`       | `FinalityUpdate`   | Finalized header proof                 |
/// | `expected_current_slot` | `u64`              | Current chain slot for validation      |
/// | `store`                 | `LightClientStore` | Full client state                      |
/// | `genesis_root`          | `B256`             | Genesis block root                     |
/// | `forks`                 | `ForkData`         | Network fork versions                  |
/// | `store_hash`            | `B256`             | SHA-256(store) from last proof         |
/// | `queue_storage`         | `QueueStorage`     | Proof request queue account & entry proofs |
///
/// # Operations (In Exact Execution Order)
/// 1. **Last Store Hash Validation** (Irreversible Check)
///    - Calculate last store hash: `SHA-256(serde_serialize(store))`
///    - Assert equality: `calculated_prev_store_hash == input_store_hash`
///
/// 2. **State Capture** (Pre-Update Snapshot)
///    - Record `input_slot` = `store.finalized_header.beacon().slot`
///
/// 3. **Update Processing** (Sequential, Atomic)
///    - Verify and apply each sync committee update in `updates`, if any
///
/// 4. **Finality Proof** (Header Finalization)
///    - Verify and apply `finality_update`
///
/// 5. **Verify Proof Request Queue**
///    - Extract `execution_state_root` = `store.finalized_header.execution()?.state_root()`
///      (fails with `MissingExecutionRoot` if execution header is absent)
///    - Extract `proof_request_queue_address` from `queue_storage.proof_request_queue_address`
///    - Verify the queue account, its `head`, every queued entry, and each entry's target
///      account and storage word via `verify_queue`
///    - Produces `(output_queue_cursor, verified_requests_root)`
///
/// 6. **State Capture** (Post-Update Snapshot)
///    - Record `output_slot` = `store.finalized_header.beacon().slot`
///    - Extract `next_sync_committee_hash` = `store.next_sync_committee.tree_hash_root()`
///      (`B256::ZERO` if `next_sync_committee` is `None`)
///
/// 7. **Checkpoint Slot Validation** (Output Rejection)
///    - Assert `output_slot % 32 == 0`
///    - `output_slot` reflects the store's actual finalized header slot after `updates` and
///      `finality_update` have been applied (`apply_generic_update` only advances
///      `store.finalized_header` when the update is newer/quorate, so the pre-apply
///      `finality_update.finalized_header().beacon().slot` is not a reliable stand-in for it)
///    - Non-checkpoint slots cannot be bootstrapped via `getLightClientBootstrap`,
///      so committing one on-chain bricks the bridge (see finding 1eb72)
///
/// 8. **Post-State Hashing**
///    - Compute `output_store_hash` = `SHA-256(serde_serialize(store))`, to be validated in the next round
///
/// 9. **Output Commitment**
///    - Pack `ProofOutputs` committing: `input_slot`, `input_store_hash`,
///      `output_slot`, `output_store_hash`, `execution_state_root`,
///      `verified_requests_root`, `next_sync_committee_hash`,
///      `proof_request_queue_address`, `input_queue_cursor`, `output_queue_cursor`,
///      `output_block_number`
///
/// # Outputs (All Values Are Hash Commitments)
/// | Field                         | Type      | Description                                     |
/// |-------------------------------|-----------|-------------------------------------------------|
/// | `input_slot`                  | `u64`     | Slot before updates                             |
/// | `input_store_hash`            | `B256`    | Input store hash                                |
/// | `output_slot`                 | `u64`     | Slot after updates                              |
/// | `output_store_hash`           | `B256`    | Updated store hash                              |
/// | `execution_state_root`        | `B256`    | Execution layer state root                      |
/// | `verified_requests_root`      | `B256`    | Merkle root of verified requests                |
/// | `next_sync_committee_hash`    | `B256`    | Hash of the next sync committee state (or zero) |
/// | `proof_request_queue_address` | `Address` | NoriProofRequestQueue address (20 bytes, BE)    |
/// | `input_queue_cursor`          | `u64`     | Cursor this proof resumed from                  |
/// | `output_queue_cursor`         | `u64`     | Cursor after draining this batch                |
/// | `output_block_number`         | `u64`     | Execution block number of finalized output      |
///
/// # Error Conditions
/// 1. **Store Hashing Error**
///    `sha256_hash_helios_store` fails → `StoreHashingError` (steps 1 and 8)
/// 2. **Hash Chain Break**
///    `calculated_prev_store_hash != input_store_hash` → Invalid initial state
/// 3. **Invalid Update**
///    Any `verify_update` returns error → Malformed or fraudulent update
/// 4. **Invalid Finality**
///    `verify_finality_update` fails → Unverifiable final header
/// 5. **Missing Execution Root**
///    `store.finalized_header.execution()` is `Err` → Incomplete header data
/// 6. **Invalid MPT Proof**
///    `verify_queue` may fail due to:
///    - `InvalidProofRequestQueueAccountProof { address, reason }` → the queue account itself could not be proven against the execution state root
///    - `InvalidTargetAccountProof { address, reason }` → a consumer contract named by a queue entry could not be proven present or absent
///    - `InvalidStorageSlotProof { slot_key, reason }` → a storage slot proof failed
///    - `LeafHashError { target, slot_key, value, reason }` → `hash_request_leaf` failed for a queue entry
///    - `ExceedsMaxTreeDepth { slots, requested_depth, max_depth }` → the number of queue entries yields a Merkle tree that is too large
///    - `CursorAheadOfHead { cursor, head }` → the destination-chain cursor is ahead of the proven queue head
///    - `BatchSizeMismatch { expected, supplied }` → the witness supplied a different number of entries than the batch the queue state derives
///    - `MissingTargetWitness { target }` / `MissingSlotWitness { target, slot_key }` → no account or slot proof was supplied for a target/slot an entry references
///    Any of these returns an `MptError`, wrapped as `ProgramError::MptError`
/// 7. **Non-Checkpoint Output Slot**
///    `output_slot % 32 != 0` (checked after `updates`/`finality_update` are applied) → `NonCheckpointOutputSlot`
///
pub fn consensus_mpt_program<S: ConsensusSpec>(
    proof_inputs: ProofInputs<S>,
    debug_print: bool,
) -> Result<ProofOutputs, ProgramError> {
    // Unpack inputs
    let ProofInputs {
        updates,
        finality_update,
        expected_current_slot,
        mut store,
        genesis_root,
        forks,
        store_hash: input_store_hash,
        queue_storage,
    } = proof_inputs;
    // The queue is the account the storage proofs anchor on, so it is the
    // address committed to the destination chain.
    let proof_request_queue_address = queue_storage.proof_request_queue_address;
    let input_queue_cursor = queue_storage.input_cursor;
    // @AUDIT - We should consider whether we want to enforce that there are no best valid updates in the store here.
    // 0. we should not proceed if we have a best valid update in our store
    // as we have a next_sync_committe non zero assertion in the verifier contract on Mina
    // if let Some(best) = &store.best_valid_update {
    //     panic!("Best valid update in store: {:?}", best);
    // }

    // 1. Last Store Hash Validation - Calculate SHA-256(serde_serialize(store)) and assert equality with input_store_hash
    if debug_print {
        println!("Hashing last store state and comparing with proof inputs store hash.");
    }
    let calculated_prev_store_hash = sha256_hash_helios_store(&store)
        .map_err(|e| ProgramError::StoreHashingError(format!("Failed to hash store: {}", e)))?;
    if calculated_prev_store_hash != input_store_hash {
        return Err(ProgramError::HashChainMismatch {
            expected: input_store_hash,
            actual: calculated_prev_store_hash,
        });
    }
    if debug_print {
        println!(
            "Last store state hash is valid: {}",
            calculated_prev_store_hash
        );
    }

    // 2. State Capture (Pre-Update Snapshot) - Record input_slot
    if debug_print {
        println!("Capturing input_slot.");
    }
    let input_slot = store.finalized_header.beacon().slot;
    if debug_print {
        println!("input_slot captured.");
    }

    // 3. Update Processing - Verify and apply sync committee updates, if any
    for (index, update) in updates.iter().enumerate() {
        // update.finalized_header.beacon().slot; introduce printing this so we can see if we are applying updates beyond our head
        let finalized_beacon_slot = { update.finalized_header().beacon().slot };
        if debug_print {
            println!(
                "Processing update {} of {}. Update beacon finalized slot: {}",
                index + 1,
                updates.len(),
                finalized_beacon_slot
            );
        }
        if let Err(err) = verify_update(update, expected_current_slot, &store, genesis_root, &forks)
        {
            return Err(ProgramError::InvalidUpdate {
                index,
                reason: format!("{:?}", err),
            });
        }
        if debug_print {
            println!("Update {} is valid.", index + 1);
        }
        apply_update(&mut store, update);
        if debug_print {
            println!("Applied update {}.", index + 1);
        }
    }

    // 4. Finality Proof - Verify and apply finality update
    if debug_print {
        println!("Processing finality update.");
    }
    if let Err(err) = verify_finality_update(
        &finality_update,
        expected_current_slot,
        &store,
        genesis_root,
        &forks,
    ) {
        return Err(ProgramError::InvalidFinalityUpdate {
            reason: format!("{:?}", err),
        });
    }
    if debug_print {
        println!("Finality update is valid.");
    }
    apply_finality_update(&mut store, &finality_update);
    if debug_print {
        println!("Applied finality update.");
    }

    // Should do an assertion here to ensure we have increased our head (we do check this downstream later)

    // 5. Verify Proof Request Queue - execution_state_root, proof_request_queue_address, MPT proofs
    let execution_state_root_result = store.finalized_header.execution();
    if execution_state_root_result.is_err() {
        return Err(ProgramError::MissingExecutionRoot);
    }
    let execution = execution_state_root_result.unwrap();
    let execution_state_root = *execution.state_root();
    let output_block_number = *execution.block_number();
    if debug_print {
        println!("Verifying proof request queue.");
    }
    let (output_queue_cursor, verified_requests_root) =
        verify_queue(execution_state_root, queue_storage)
            .map_err(ProgramError::MptError)?;
    if debug_print {
        println!(
            "Proof request queue is valid, cursor {} -> {}.",
            input_queue_cursor, output_queue_cursor
        );
    }

    // 6. State Capture (Post-Update Snapshot) - output_slot, next_sync_committee_hash
    if debug_print {
        println!("Capturing output_slot, next_sync_committee_hash.");
    }
    let output_slot = store.finalized_header.beacon().slot;
    let next_sync_committee_hash: B256 = match &mut store.next_sync_committee {
        Some(next_sync_committee) => next_sync_committee.tree_hash_root(),
        None => B256::ZERO,
    };
    if debug_print {
        println!("output_slot, next_sync_committee_hash captured.");
    }

    // 7. Checkpoint Slot Validation - Reject non-checkpoint output slots (output_slot % 32 != 0)
    if output_slot % 32 != 0 {
        return Err(ProgramError::NonCheckpointOutputSlot { slot: output_slot });
    }

    // 8. Post-State Hashing - Calculate updated store hash to be validated in the next round
    if debug_print {
        println!("Hashing updated store.");
    }
    let output_store_hash = sha256_hash_helios_store(&store).map_err(|e| {
        ProgramError::StoreHashingError(format!("Failed to hash updated store: {}", e))
    })?;
    if debug_print {
        println!("Hashing updated store complete: {}", output_store_hash);
    }

    // 9. Output Commitment
    if debug_print {
        println!("Committing outputs.");
    }
    let proof_outputs = ProofOutputs {
        input_slot,
        input_store_hash,
        output_slot,
        output_store_hash,
        execution_state_root,
        verified_requests_root,
        next_sync_committee_hash,
        proof_request_queue_address,
        input_queue_cursor,
        output_queue_cursor,
        output_block_number,
    };
    if debug_print {
        println!("Packed outputs.");
    }

    Ok(proof_outputs)
}
