use alloy_primitives::{B256, U256};
use helios_consensus_core::{
    apply_finality_update, apply_update, consensus_spec::ConsensusSpec, verify_finality_update,
    verify_update,
};
use log::debug;
use nori_hash::sha256_hash::sha256_hash_helios_store;
use nori_sp1_helios_primitives::types::{ConsensusProofInputs, ConsensusProofOutputs, ProofInputs, ProofOutputs};
use std::fmt;
use tree_hash::TreeHash;

use crate::mpt::{verify_storage_slot_proofs, zk_verify_storage_slot_proofs, MptError};

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
///      ▼               ▼              ▼
/// Initial State     Updates       Final State
///     Validation                 Commitment
/// ```
///
/// # Inputs (All Values Must Be Precomputed Hashes)
/// | Name                  | Type              | Description                      |
/// |-----------------------|-------------------|----------------------------------|
/// | `updates`             | `Vec<Update>`     | Ordered sync committee updates    |
/// | `finality_update`     | `FinalityUpdate`  | Finalized header proof           |
/// | `expected_current_slot` | `Slot`          | Current chain slot for validation |
/// | `store`               | `LightClientStore`| Full client state                |
/// | `genesis_root`        | `B256`            | Genesis block root               |
/// | `forks`               | `ForkData`        | Network fork versions            |
/// | `store_hash`          | `B256`            | SHA-256(store) from last proof   |
///
/// # Operations (In Exact Execution Order)
/// 1. **Initial Hash Validation** (Irreversible Check)
///    - Compute `SHA-256(store)`
///    - Assert: `calculated_prev_store_hash == prev_store_hash`
///
/// 2. **State Capture** (Pre-Update Snapshot)
///    - Record `prev_header` = `store.finalized_header.beacon.tree_hash_root()`
///    - Record `prev_head` = `store.finalized_header.beacon.slot`
///    - Record `start_sync_committee_hash` = `store.current_sync_committee.tree_hash_root()`
///
/// 3. **Update Processing** (Sequential, Atomic)
///    For each update in `sync_committee_updates`:
///    - Verify update signatures against current sync committee
///    - Check slot consistency with `expected_current_slot`
///    - Validate fork versions against `forks`
///    - Apply update to `store` state (mutates sync committees)
///
/// 4. **Finality Proof** (Header Finalization)
///    - Verify finality signatures against updated sync committee
///    - Validate finality branch to `genesis_root`
///    - Update `store.finalized_header` to new header
///
/// 5. **State commitment** (Commit new state root)
///    - Capture new head slot
///    - Compute hashes for header, sync_committee, next_sync_committee and execution_state_root
///
/// 6. **Post-State Hashing** (Output Generation)
///    - Compute `header` = new `store.finalized_header.beacon.tree_hash_root()`
///    - Compute `sync_committee_hash` = `store.current_sync_committee.tree_hash_root()`
///    - Compute `next_sync_committee_hash` or zero
///    - Extract `execution_state_root` from finalized header's payload
///    - Compute `store_hash` = `SHA-256(store)`
///
/// # Outputs (All Values Are Hash Commitments)
/// | Field                   | Type      | Description                          |
/// |-------------------------|-----------|--------------------------------------|
/// | `executionStateRoot`    | `B256`    | Execution layer state root           |
/// | `newHeader`             | `B256`    | New finalized header SSZ hash        |
/// | `nextSyncCommitteeHash` | `B256`    | Next committee hash or 0x0           |
/// | `newHead`               | `U256`    | New finalized slot number            |
/// | `prevHeader`            | `B256`    | Previous header SSZ hash             |
/// | `prevHead`              | `U256`    | Previous finalized slot              |
/// | `syncCommitteeHash`     | `B256`    | Current sync committee SSZ hash      |
/// | `startSyncCommitteeHash`| `B256`    | Initial sync committee SSZ hash      |
/// | `prevStoreHash`         | `B256`    | Input store hash                     |
/// | `storeHash`             | `B256`    | New store SHA-256                    |
///
/// # Error Conditions
/// 1. **Hash Chain Break**  
///    `calculated_prev_store_hash != prev_store_hash`  
///    → Invalid initial state
///
/// 2. **Invalid Update**  
///    Any `verify_update` returns error  
///    → Malformed or fraudulent update
///
/// 3. **Invalid Finality**  
///    `verify_finality_update` fails  
///    → Unverifiable final header
///
/// 4. **Missing Execution Root**  
///    `store.finalized_header.execution()` is None  
///    → Incomplete header data
///
pub fn consensus_program<S: ConsensusSpec>(proof_inputs: ConsensusProofInputs<S>) -> Result<ConsensusProofOutputs, ProgramError> {
    // Unpack inputs
    let ConsensusProofInputs {
        updates,
        finality_update,
        expected_current_slot,
        mut store,
        genesis_root,
        forks,
        store_hash: prev_store_hash,
    } = proof_inputs;

    // 1. Calculate old store hash and assert equality
    debug!("Hashing old store state and comparing with proof inputs store hash.");
    let calculated_prev_store_hash = sha256_hash_helios_store(&store)
        .map_err(|e| ProgramError::StoreHashingError(format!("Failed to hash store: {}", e)))?;
    if calculated_prev_store_hash != prev_store_hash {
        return Err(ProgramError::HashChainMismatch {
            expected: prev_store_hash,
            actual: calculated_prev_store_hash,
        });
    }
    debug!(
        "Old store state hash is valid: {}",
        calculated_prev_store_hash
    );

    // 2. State capture
    debug!("Committing prev_header, prev_head and start_sync_committee_hash.");
    let prev_head = store.finalized_header.beacon().slot;
    let prev_header: B256 = store.finalized_header.beacon().tree_hash_root(); // Could deprecate
    let start_sync_committee_hash: alloy_primitives::FixedBytes<32> =
        store.current_sync_committee.tree_hash_root(); // Could deprecate
    debug!("prev_head, prev_header and start_sync_committee_hash committed.");

    // 3. Apply sync committee updates, if any
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

    // 4. Apply finality update
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

    // 5. Commit new state root, header, and sync committee
    debug!("Committing head, header, sync_committee_hash, next_sync_committee_hash and execution_state_root.");
    let head = store.finalized_header.beacon().slot;
    let header: B256 = store.finalized_header.beacon().tree_hash_root(); // Could deprecate
    let sync_committee_hash: B256 = store.current_sync_committee.tree_hash_root(); // Could deprecate
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
    debug!("head, header, sync_committee_hash, next_sync_committee_hash and execution_state_root committed.");

    // 6. Calculated updated store hash to be validated in the next round
    debug!("Hashing updated store.");
    let store_hash = sha256_hash_helios_store(&store).map_err(|e| {
        ProgramError::StoreHashingError(format!("Failed to hash updated store: {}", e))
    })?;
    debug!("Hashing updated store complete: {}", store_hash);

    debug!("Packing outputs.");
    let proof_outputs = ConsensusProofOutputs {
        executionStateRoot: execution_state_root,
        newHeader: header,
        nextSyncCommitteeHash: next_sync_committee_hash,
        newHead: U256::from(head),
        prevHeader: prev_header,
        prevHead: U256::from(prev_head),
        syncCommitteeHash: sync_committee_hash,
        startSyncCommitteeHash: start_sync_committee_hash,
        prevStoreHash: prev_store_hash,
        storeHash: store_hash,
    };
    debug!("Packed outputs.");

    Ok(proof_outputs)
}

/// Zero-Knowledge Consensus State Transition Proof with MPT storage slot verification for Ethereum Light Client Updates with Result type
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
///      ▼               ▼              ▼
/// Initial State     Updates       Final State
///     Validation                 Commitment
/// ```
///
/// # Inputs (All Values Must Be Precomputed Hashes)
/// | Name                  | Type              | Description                      |
/// |-----------------------|-------------------|----------------------------------|
/// | `updates`             | `Vec<Update>`     | Ordered sync committee updates    |
/// | `finality_update`     | `FinalityUpdate`  | Finalized header proof           |
/// | `expected_current_slot` | `Slot`          | Current chain slot for validation |
/// | `store`               | `LightClientStore`| Full client state                |
/// | `genesis_root`        | `B256`            | Genesis block root               |
/// | `forks`               | `ForkData`        | Network fork versions            |
/// | `store_hash`          | `B256`            | SHA-256(store) from last proof   |
///
/// # Operations (In Exact Execution Order)
/// 1. **Initial Hash Validation** (Irreversible Check)
///    - Compute `SHA-256(store)`
///    - Assert: `calculated_prev_store_hash == prev_store_hash`
///
/// 2. **State Capture** (Pre-Update Snapshot)
///    - Record `prev_header` = `store.finalized_header.beacon.tree_hash_root()`
///    - Record `prev_head` = `store.finalized_header.beacon.slot`
///    - Record `start_sync_committee_hash` = `store.current_sync_committee.tree_hash_root()`
///
/// 3. **Update Processing** (Sequential, Atomic)
///    For each update in `sync_committee_updates`:
///    - Verify update signatures against current sync committee
///    - Check slot consistency with `expected_current_slot`
///    - Validate fork versions against `forks`
///    - Apply update to `store` state (mutates sync committees)
///
/// 4. **Finality Proof** (Header Finalization)
///    - Verify finality signatures against updated sync committee
///    - Validate finality branch to `genesis_root`
///    - Update `store.finalized_header` to new header
///
/// 5. **Verify storage slot proofs** (Verify MPT proof of storage slots)
///    - Extract provable executation state root
///    - Verify the source contracts storage slots
///
/// 6. **State commitment** (Commit new state root)
///    - Capture new head slot
///    - Compute hashes for header, sync_committee, next_sync_committee and execution_state_root
///
/// 7. **Post-State Hashing** (Output Generation)
///    - Compute `header` = new `store.finalized_header.beacon.tree_hash_root()`
///    - Compute `sync_committee_hash` = `store.current_sync_committee.tree_hash_root()`
///    - Compute `next_sync_committee_hash` or zero
///    - Extract `execution_state_root` from finalized header's payload
///    - Compute `store_hash` = `SHA-256(store)`
///
/// # Outputs (All Values Are Hash Commitments)
/// | Field                   | Type      | Description                          |
/// |-------------------------|-----------|--------------------------------------|
/// | `executionStateRoot`    | `B256`    | Execution layer state root           |
/// | `newHeader`             | `B256`    | New finalized header SSZ hash        |
/// | `nextSyncCommitteeHash` | `B256`    | Next committee hash or 0x0           |
/// | `newHead`               | `U256`    | New finalized slot number            |
/// | `prevHeader`            | `B256`    | Previous header SSZ hash             |
/// | `prevHead`              | `U256`    | Previous finalized slot              |
/// | `syncCommitteeHash`     | `B256`    | Current sync committee SSZ hash      |
/// | `startSyncCommitteeHash`| `B256`    | Initial sync committee SSZ hash      |
/// | `prevStoreHash`         | `B256`    | Input store hash                     |
/// | `storeHash`             | `B256`    | New store SHA-256                    |
///
/// # Error Conditions
/// 1. **Hash Chain Break**  
///    `calculated_prev_store_hash != prev_store_hash`  
///    → Invalid initial state
///
/// 2. **Invalid Update**  
///    Any `verify_update` returns error  
///    → Malformed or fraudulent update
///
/// 3. **Invalid Finality**  
///    `verify_finality_update` fails  
///    → Unverifiable final header
///
/// 4. **Missing Execution Root**  
///    `store.finalized_header.execution()` is None  
///    → Incomplete header data
///
/// 5. **Invalid MPT proof**  
///    `zk_verify_storage_slot_proofs` fails
///    → Invalid storage slots / account proof
///
pub fn consensus_mpt_program<S: ConsensusSpec>(
    proof_inputs: ProofInputs<S>,
) -> Result<ProofOutputs, ProgramError> {
    // Unpack inputs
    let ProofInputs {
        updates,
        finality_update,
        expected_current_slot,
        mut store,
        genesis_root,
        forks,
        store_hash: prev_store_hash,
        contract_storage,
    } = proof_inputs;

    // 1. Calculate old store hash and assert equality
    debug!("Hashing old store state and comparing with proof inputs store hash.");
    let calculated_prev_store_hash = sha256_hash_helios_store(&store)
        .map_err(|e| ProgramError::StoreHashingError(format!("Failed to hash store: {}", e)))?;
    if calculated_prev_store_hash != prev_store_hash {
        return Err(ProgramError::HashChainMismatch {
            expected: prev_store_hash,
            actual: calculated_prev_store_hash,
        });
    }
    debug!(
        "Old store state hash is valid: {}",
        calculated_prev_store_hash
    );

    // 2. State capture
    debug!("Committing prev_header, prev_head and start_sync_committee_hash.");
    let prev_head = store.finalized_header.beacon().slot;
    let prev_header: B256 = store.finalized_header.beacon().tree_hash_root(); // Could deprecate
    let start_sync_committee_hash: alloy_primitives::FixedBytes<32> =
        store.current_sync_committee.tree_hash_root(); // Could deprecate
    debug!("prev_head, prev_header and start_sync_committee_hash committed.");

    // 3. Apply sync committee updates, if any
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

    // 4. Apply finality update
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

    // 5. Verify storage slot proofs
    let execution_state_root_result = store.finalized_header.execution();
    if execution_state_root_result.is_err() {
        return Err(ProgramError::MissingExecutionRoot);
    }
    let execution = execution_state_root_result.unwrap();
    let execution_state_root = *execution.state_root();
    debug!("Verifying contract storage slots.");
    let verified_slots_result =
        verify_storage_slot_proofs(execution_state_root, contract_storage);
    if let Err(verified_slots_err) = verified_slots_result {
        return Err(ProgramError::MptError(verified_slots_err));
    }
    let verified_slots = verified_slots_result.unwrap();
    debug!("Contract storage slots are valid.");

    // 6. Commit new state root, header, and sync committee
    debug!("Committing head, header, sync_committee_hash, next_sync_committee_hash and execution_state_root.");
    let head = store.finalized_header.beacon().slot;
    let header: B256 = store.finalized_header.beacon().tree_hash_root(); // Could deprecate
    let sync_committee_hash: B256 = store.current_sync_committee.tree_hash_root(); // Could deprecate
    let next_sync_committee_hash: B256 = match &mut store.next_sync_committee {
        Some(next_sync_committee) => next_sync_committee.tree_hash_root(),
        None => B256::ZERO,
    };
    debug!("head, header, sync_committee_hash, next_sync_committee_hash and execution_state_root committed.");

    // 7. Calculated updated store hash to be validated in the next round
    debug!("Hashing updated store.");
    let store_hash = sha256_hash_helios_store(&store).map_err(|e| {
        ProgramError::StoreHashingError(format!("Failed to hash updated store: {}", e))
    })?;
    debug!("Hashing updated store complete: {}", store_hash);

    debug!("Packing outputs.");
    let proof_outputs = ProofOutputs {
        executionStateRoot: execution_state_root,
        newHeader: header,
        nextSyncCommitteeHash: next_sync_committee_hash,
        newHead: U256::from(head),
        prevHeader: prev_header,
        prevHead: U256::from(prev_head),
        syncCommitteeHash: sync_committee_hash,
        startSyncCommitteeHash: start_sync_committee_hash,
        prevStoreHash: prev_store_hash,
        storeHash: store_hash,
        verifiedContractStorageSlots: verified_slots
    };
    debug!("Packed outputs.");

    Ok(proof_outputs)
}

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

/// Zero-Knowledge Consensus State Transition Proof with MPT storage slot verification for Ethereum Light Client Updates
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
///      ▼               ▼              ▼
/// Initial State     Updates       Final State
///     Validation                 Commitment
/// ```
///
/// # Inputs (All Values Must Be Precomputed Hashes)
/// | Name                  | Type              | Description                      |
/// |-----------------------|-------------------|----------------------------------|
/// | `updates`             | `Vec<Update>`     | Ordered sync committee updates    |
/// | `finality_update`     | `FinalityUpdate`  | Finalized header proof           |
/// | `expected_current_slot` | `Slot`          | Current chain slot for validation |
/// | `store`               | `LightClientStore`| Full client state                |
/// | `genesis_root`        | `B256`            | Genesis block root               |
/// | `forks`               | `ForkData`        | Network fork versions            |
/// | `store_hash`          | `B256`            | SHA-256(store) from last proof   |
///
/// # Operations (In Exact Execution Order)
/// 1. **Initial Hash Validation** (Irreversible Check)
///    - Compute `SHA-256(store)`
///    - Assert: `calculated_prev_store_hash == prev_store_hash`
///
/// 2. **State Capture** (Pre-Update Snapshot)
///    - Record `prev_header` = `store.finalized_header.beacon.tree_hash_root()`
///    - Record `prev_head` = `store.finalized_header.beacon.slot`
///    - Record `start_sync_committee_hash` = `store.current_sync_committee.tree_hash_root()`
///
/// 3. **Update Processing** (Sequential, Atomic)
///    For each update in `sync_committee_updates`:
///    - Verify update signatures against current sync committee
///    - Check slot consistency with `expected_current_slot`
///    - Validate fork versions against `forks`
///    - Apply update to `store` state (mutates sync committees)
///
/// 4. **Finality Proof** (Header Finalization)
///    - Verify finality signatures against updated sync committee
///    - Validate finality branch to `genesis_root`
///    - Update `store.finalized_header` to new header
///
/// 5. **Verify storage slot proofs** (Verify MPT proof of storage slots)
///    - Extract provable executation state root
///    - Verify the source contracts storage slots
///
/// 6. **State commitment** (Commit new state root)
///    - Capture new head slot
///    - Compute hashes for header, sync_committee, next_sync_committee and execution_state_root
///
/// 7. **Post-State Hashing** (Output Generation)
///    - Compute `header` = new `store.finalized_header.beacon.tree_hash_root()`
///    - Compute `sync_committee_hash` = `store.current_sync_committee.tree_hash_root()`
///    - Compute `next_sync_committee_hash` or zero
///    - Extract `execution_state_root` from finalized header's payload
///    - Compute `store_hash` = `SHA-256(store)`
///
/// # Outputs (All Values Are Hash Commitments)
/// | Field                   | Type      | Description                          |
/// |-------------------------|-----------|--------------------------------------|
/// | `executionStateRoot`    | `B256`    | Execution layer state root           |
/// | `newHeader`             | `B256`    | New finalized header SSZ hash        |
/// | `nextSyncCommitteeHash` | `B256`    | Next committee hash or 0x0           |
/// | `newHead`               | `U256`    | New finalized slot number            |
/// | `prevHeader`            | `B256`    | Previous header SSZ hash             |
/// | `prevHead`              | `U256`    | Previous finalized slot              |
/// | `syncCommitteeHash`     | `B256`    | Current sync committee SSZ hash      |
/// | `startSyncCommitteeHash`| `B256`    | Initial sync committee SSZ hash      |
/// | `prevStoreHash`         | `B256`    | Input store hash                     |
/// | `storeHash`             | `B256`    | New store SHA-256                    |
///
/// # Panic Conditions (Total Program Abort)
/// 1. **Hash Chain Break**  
///    `calculated_prev_store_hash != prev_store_hash`  
///    → Invalid initial state
///
/// 2. **Invalid Update**  
///    Any `verify_update` returns error  
///    → Malformed or fraudulent update
///
/// 3. **Invalid Finality**  
///    `verify_finality_update` fails  
///    → Unverifiable final header
///
/// 4. **Missing Execution Root**  
///    `store.finalized_header.execution()` is None  
///    → Incomplete header data
///
/// 5. **Invalid MPT proof**  
///    `zk_verify_storage_slot_proofs` fails
///    → Invalid storage slots / account proof
///
pub fn zk_consensus_mpt_program<S: ConsensusSpec>(proof_inputs: ProofInputs<S>) -> ProofOutputs {
    // Unpack inputs
    let ProofInputs {
        updates,
        finality_update,
        expected_current_slot,
        mut store,
        genesis_root,
        forks,
        store_hash: prev_store_hash,
        contract_storage: contract_storage_slots,
    } = proof_inputs;

    // 1. Calculate old store hash and assert equality
    println!("Hashing old store state and comparing with proof inputs store hash.");
    let calculated_prev_store_hash = sha256_hash_helios_store(&store).unwrap();
    assert_eq!(calculated_prev_store_hash, prev_store_hash);
    println!(
        "Old store state hash is valid: {}",
        calculated_prev_store_hash
    );

    // 2. State capture
    println!("Committing prev_header, prev_head and start_sync_committee_hash.");
    let prev_head = store.finalized_header.beacon().slot;
    let prev_header: B256 = store.finalized_header.beacon().tree_hash_root(); // Could deprecate
    let start_sync_committee_hash: alloy_primitives::FixedBytes<32> =
        store.current_sync_committee.tree_hash_root(); // Could deprecate
    println!("prev_head, prev_header and start_sync_committee_hash committed.");

    // 3. Apply sync committee updates, if any
    for (index, update) in updates.iter().enumerate() {
        // update.finalized_header.beacon().slot; introduce printing this so we can see if we are applying updates beyond our head
        let finalized_beacon_slot = { update.finalized_header().beacon().slot };
        println!(
            "Processing update {} of {}. Update beacon finalized slot: {}",
            index + 1,
            updates.len(),
            finalized_beacon_slot
        );
        let update_is_valid =
            verify_update(update, expected_current_slot, &store, genesis_root, &forks).is_ok();

        if !update_is_valid {
            panic!("Update {} is invalid!", index + 1);
        }
        println!("Update {} is valid.", index + 1);
        apply_update(&mut store, update);
        println!("Applied update {}.", index + 1);
    }

    // 4. Apply finality update
    println!("Processing finality update.");
    let finality_update_is_valid = verify_finality_update(
        &finality_update,
        expected_current_slot,
        &store,
        genesis_root,
        &forks,
    )
    .is_ok();
    if !finality_update_is_valid {
        panic!("Finality update is invalid!");
    }
    println!("Finality update is valid.");
    apply_finality_update(&mut store, &finality_update);
    println!("Applied finality update.");

    // Should do an assertion here to ensure we have increased our head (we do check this downstream later)

    // 5. Verify storage slot proofs
    let execution_state_root = *store
        .finalized_header
        .execution()
        .expect("Execution payload doesn't exist.")
        .state_root();
    println!("Verifying contract storage slots.");
    let verified_slots =
        zk_verify_storage_slot_proofs(execution_state_root, contract_storage_slots);
    println!("Contract storage slots are valid.");

    // 6. Commit new state root, header, and sync committee
    println!("Committing head, header, sync_committee_hash, next_sync_committee_hash and execution_state_root.");
    let head = store.finalized_header.beacon().slot;
    let header: B256 = store.finalized_header.beacon().tree_hash_root(); // Could deprecate
    let sync_committee_hash: B256 = store.current_sync_committee.tree_hash_root(); // Could deprecate
    let next_sync_committee_hash: B256 = match &mut store.next_sync_committee {
        Some(next_sync_committee) => next_sync_committee.tree_hash_root(),
        None => B256::ZERO,
    };
    println!("head, header, sync_committee_hash, next_sync_committee_hash and execution_state_root committed.");

    // 7. Calculated updated store hash to be validated in the next round
    println!("Hashing updated store.");
    let store_hash = sha256_hash_helios_store(&store).unwrap();
    println!("Hashing updated store complete: {}", store_hash);

    println!("Packing outputs.");
    let proof_outputs = ProofOutputs {
        executionStateRoot: execution_state_root,
        newHeader: header,
        nextSyncCommitteeHash: next_sync_committee_hash,
        newHead: U256::from(head),
        prevHeader: prev_header,
        prevHead: U256::from(prev_head),
        syncCommitteeHash: sync_committee_hash,
        startSyncCommitteeHash: start_sync_committee_hash,
        prevStoreHash: prev_store_hash,
        storeHash: store_hash,
        verifiedContractStorageSlots: verified_slots
    };
    println!("Packed outputs.");

    proof_outputs
}
