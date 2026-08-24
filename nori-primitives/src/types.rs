use crate::storage_layout::{MAX_COLLECTION_KEYS, QUEUE_ENTRY_WORDS};
use alloy_primitives::{Address, Bytes, FixedBytes, B256, U256};
use alloy_trie::TrieAccount;
use anyhow::{bail, Context, Result};
use helios_consensus_core::consensus_spec::ConsensusSpec;
use helios_consensus_core::types::Forks;
use helios_consensus_core::types::{FinalityUpdate, LightClientStore, Update};
use serde::{Deserialize, Serialize};

/// One queue entry: the claimed field values plus one MPT proof per storage
/// word. The values are witness data; each is pinned by verifying its word
/// proof at the entry's computed storage location.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QueueEntryProof {
    pub target: Address,
    pub slot_key: B256,
    pub collection_keys_count: u8,
    pub collection_keys: [B256; MAX_COLLECTION_KEYS],
    /// Proofs for words `base ..= base + 4`, against the queue's storage root.
    pub word_proofs: [Vec<Bytes>; QUEUE_ENTRY_WORDS],
}

/// One requested storage slot under a target's storage root.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TargetSlotProof {
    pub key: B256,
    /// Claimed value. Zero claims absence, pinned by an exclusion proof.
    pub value: U256,
    pub mpt_proof: Vec<Bytes>,
}

/// Account and storage proofs for one target contract in a batch.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TargetStorageProof {
    /// Address of the consumer contract whose storage is being proven.
    pub target_address: Address,
    /// `None` claims the account does not exist, pinned by an exclusion proof.
    pub account: Option<TrieAccount>,
    /// Proof against the execution state root; inclusion or exclusion.
    pub account_mpt_proof: Vec<Bytes>,
    pub slots: Vec<TargetSlotProof>,
}

/// Queue state and proofs for one batch of entries.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QueueStorage {
    /// Address of the NoriProofRequestQueue contract.
    pub proof_request_queue_address: Address,
    pub proof_request_queue_account: TrieAccount,
    pub proof_request_queue_account_mpt_proof: Vec<Bytes>,
    pub head: u64,
    /// Proof of slot `QUEUE_HEAD_STORAGE_INDEX`; an exclusion proof when `head` is 0.
    pub head_mpt_proof: Vec<Bytes>,
    /// Cursor the destination chain has settled at, and this batch resumes from.
    pub input_cursor: u64,
    /// Entries in index order, starting at `input_cursor`.
    pub entries: Vec<QueueEntryProof>,
    /// One per distinct target address referenced by `entries`.
    pub targets: Vec<TargetStorageProof>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProofInputs<S: ConsensusSpec> {
    pub updates: Vec<Update<S>>,
    pub finality_update: FinalityUpdate<S>,
    pub expected_current_slot: u64,
    pub store: LightClientStore<S>,
    pub genesis_root: B256,
    pub forks: Forks,
    pub store_hash: B256,
    pub queue_storage: QueueStorage,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProofInputsWithWindow<S: ConsensusSpec> {
    pub input_slot: u64,
    pub expected_output_slot: u64,
    pub input_block_number: u64,
    pub expected_output_block_number: u64,
    pub proof_inputs: ProofInputs<S>,
    pub expected_output_store_hash: FixedBytes<32>,
    pub expected_output_queue_cursor: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DualProofInputsWithWindow<S: ConsensusSpec> {
    pub current_window: ProofInputsWithWindow<S>,
    pub next_window: Option<ProofInputsWithWindow<S>>
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConsensusProofInputs<S: ConsensusSpec> {
    pub updates: Vec<Update<S>>,
    pub finality_update: FinalityUpdate<S>,
    pub expected_current_slot: u64,
    pub store: LightClientStore<S>,
    pub genesis_root: B256,
    pub forks: Forks,
    pub store_hash: B256,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExecutionStateProof {
    #[serde(rename = "executionStateRoot")]
    pub execution_state_root: B256,
    #[serde(rename = "executionStateBranch")]
    pub execution_state_branch: Vec<B256>,
    pub gindex: String,
}

/// One verified request, carrying everything its leaf is hashed from.
///
/// Published in cursor order so a consumer can rebuild the committed tree and
/// derive Merkle paths without re-reading Ethereum.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VerifiedRequest {
    pub target: Address,
    pub collection_keys_count: u8,
    pub collection_keys: [B256; MAX_COLLECTION_KEYS],
    pub value: U256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofOutputs {
    pub input_slot: u64,                            // [  0..  8] u64
    pub input_store_hash: B256,                     // [  8.. 40] bytes32
    pub output_slot: u64,                           // [ 40.. 48] u64
    pub output_store_hash: B256,                    // [ 48.. 80] bytes32
    pub execution_state_root: B256,                 // [ 80..112] bytes32
    pub verified_requests_root: B256,               // [112..144] bytes32
    pub next_sync_committee_hash: B256,             // [144..176] bytes32
    /// NoriProofRequestQueue address; the account every storage proof anchors on.
    pub proof_request_queue_address: Address,       // [176..196] bytes20
    /// Cursor this proof resumed from; the destination chain asserts it matches
    /// the cursor it has stored.
    pub input_queue_cursor: u64,                    // [196..204] u64
    /// Cursor after draining this batch.
    pub output_queue_cursor: u64,                   // [204..212] u64
    /// Execution block number of the finalized output header.
    pub output_block_number: u64,                   // [212..220] u64
}

impl ProofOutputs {
    pub const SIZE: usize = 220;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];

        buf[0..8].copy_from_slice(&self.input_slot.to_be_bytes());
        buf[8..40].copy_from_slice(&self.input_store_hash.0);
        buf[40..48].copy_from_slice(&self.output_slot.to_be_bytes());
        buf[48..80].copy_from_slice(&self.output_store_hash.0);
        buf[80..112].copy_from_slice(&self.execution_state_root.0);
        buf[112..144].copy_from_slice(&self.verified_requests_root.0);
        buf[144..176].copy_from_slice(&self.next_sync_committee_hash.0);
        buf[176..196].copy_from_slice(self.proof_request_queue_address.as_slice()); // BE
        buf[196..204].copy_from_slice(&self.input_queue_cursor.to_be_bytes());
        buf[204..212].copy_from_slice(&self.output_queue_cursor.to_be_bytes());
        buf[212..220].copy_from_slice(&self.output_block_number.to_be_bytes());

        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != Self::SIZE {
            bail!(
                "Invalid input length for ProofOutputs: expected {} bytes, got {}",
                Self::SIZE,
                bytes.len()
            );
        }

        let input_slot_bytes: [u8; 8] = bytes[0..8]
            .try_into()
            .context("Failed to parse input_slot bytes")?;
        let input_slot = u64::from_be_bytes(input_slot_bytes);

        let input_store_hash = B256::from_slice(&bytes[8..40]);
        // Assuming B256::from_slice does not fail; if it can fail, handle accordingly

        let output_slot_bytes: [u8; 8] = bytes[40..48]
            .try_into()
            .context("Failed to parse output_slot bytes")?;
        let output_slot = u64::from_be_bytes(output_slot_bytes);

        let output_store_hash = B256::from_slice(&bytes[48..80]);
        let execution_state_root = B256::from_slice(&bytes[80..112]);
        let verified_requests_root = B256::from_slice(&bytes[112..144]);
        let next_sync_committee_hash = B256::from_slice(&bytes[144..176]);
        let proof_request_queue_address = Address::from_slice(&bytes[176..196]);

        let input_queue_cursor_bytes: [u8; 8] = bytes[196..204]
            .try_into()
            .context("Failed to parse input_queue_cursor bytes")?;
        let input_queue_cursor = u64::from_be_bytes(input_queue_cursor_bytes);

        let output_queue_cursor_bytes: [u8; 8] = bytes[204..212]
            .try_into()
            .context("Failed to parse output_queue_cursor bytes")?;
        let output_queue_cursor = u64::from_be_bytes(output_queue_cursor_bytes);

        let output_block_number_bytes: [u8; 8] = bytes[212..220]
            .try_into()
            .context("Failed to parse output_block_number bytes")?;
        let output_block_number = u64::from_be_bytes(output_block_number_bytes);

        Ok(Self {
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
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusProofOutputs {
    pub input_slot: u64,                // [  0..  8] u64
    pub input_store_hash: B256,         // [  8.. 40] bytes32
    pub output_slot: u64,               // [ 40.. 48] u64
    pub output_store_hash: B256,        // [ 48.. 80] bytes32
    pub execution_state_root: B256,     // [ 80..112] bytes32
    pub next_sync_committee_hash: B256, // [112..144] bytes32
    pub output_block_number: u64,       // [144..152] u64
}

impl ConsensusProofOutputs {
    pub const SIZE: usize = 152;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];

        buf[0..8].copy_from_slice(&self.input_slot.to_be_bytes());
        buf[8..40].copy_from_slice(&self.input_store_hash.0);
        buf[40..48].copy_from_slice(&self.output_slot.to_be_bytes());
        buf[48..80].copy_from_slice(&self.output_store_hash.0);
        buf[80..112].copy_from_slice(&self.execution_state_root.0);
        buf[112..144].copy_from_slice(&self.next_sync_committee_hash.0);
        buf[144..152].copy_from_slice(&self.output_block_number.to_be_bytes());

        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != Self::SIZE {
            anyhow::bail!(
                "Invalid input length for ConsensusProofOutputs: expected {} bytes, got {}",
                Self::SIZE,
                bytes.len()
            );
        }

        let input_slot_bytes: [u8; 8] = bytes[0..8]
            .try_into()
            .context("Failed to parse input_slot bytes")?;
        let input_slot = u64::from_be_bytes(input_slot_bytes);

        let input_store_hash = B256::from_slice(&bytes[8..40]);

        let output_slot_bytes: [u8; 8] = bytes[40..48]
            .try_into()
            .context("Failed to parse output_slot bytes")?;
        let output_slot = u64::from_be_bytes(output_slot_bytes);

        let output_store_hash = B256::from_slice(&bytes[48..80]);
        let execution_state_root = B256::from_slice(&bytes[80..112]);
        let next_sync_committee_hash = B256::from_slice(&bytes[112..144]);

        let output_block_number_bytes: [u8; 8] = bytes[144..152]
            .try_into()
            .context("Failed to parse output_block_number bytes")?;
        let output_block_number = u64::from_be_bytes(output_block_number_bytes);

        Ok(Self {
            input_slot,
            input_store_hash,
            output_slot,
            output_store_hash,
            execution_state_root,
            next_sync_committee_hash,
            output_block_number,
        })
    }
}

#[cfg(test)]
mod proof_outputs_tests {
    use super::*;

    fn sample() -> ProofOutputs {
        ProofOutputs {
            input_slot: 1,
            input_store_hash: B256::repeat_byte(0x11),
            output_slot: 2,
            output_store_hash: B256::repeat_byte(0x22),
            execution_state_root: B256::repeat_byte(0x33),
            verified_requests_root: B256::repeat_byte(0x44),
            next_sync_committee_hash: B256::repeat_byte(0x55),
            proof_request_queue_address: Address::repeat_byte(0x66),
            input_queue_cursor: 3,
            output_queue_cursor: 9,
            output_block_number: 12345,
        }
    }

    #[test]
    fn round_trips_through_bytes() {
        let bytes = sample().to_bytes();
        assert_eq!(bytes.len(), ProofOutputs::SIZE);

        let decoded = ProofOutputs::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.input_queue_cursor, 3);
        assert_eq!(decoded.output_queue_cursor, 9);
        assert_eq!(decoded.output_block_number, 12345);
        assert_eq!(decoded.to_bytes(), bytes);
    }

    /// The destination chain verifier reads these offsets, so they are part of the contract.
    #[test]
    fn cursors_and_block_number_occupy_the_final_twenty_four_bytes() {
        let bytes = sample().to_bytes();
        assert_eq!(&bytes[196..204], &3u64.to_be_bytes());
        assert_eq!(&bytes[204..212], &9u64.to_be_bytes());
        assert_eq!(&bytes[212..220], &12345u64.to_be_bytes());
    }

    #[test]
    fn rejects_a_wrong_length_buffer() {
        let bytes = sample().to_bytes();
        ProofOutputs::from_bytes(&bytes[..ProofOutputs::SIZE - 1]).unwrap_err();
    }
}
