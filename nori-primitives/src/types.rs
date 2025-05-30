use alloy_primitives::{keccak256, Address, Bytes, B256, U256};
use alloy_trie::TrieAccount;
use anyhow::{bail, Context, Result};
use helios_consensus_core::consensus_spec::ConsensusSpec;
use helios_consensus_core::types::Forks;
use helios_consensus_core::types::{FinalityUpdate, LightClientStore, Update};
use serde::{Deserialize, Serialize};

// TODO FIX ME FIND A BETTER PLACE FOR THIS!
pub const SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX: u8 = 1u8;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StorageSlot {
    pub key: B256, // raw 32 byte storage slot key e.g. for slot 0: 0x000...00
    pub slot_key_address: Address, // address associated with the slot key
    pub expected_value: U256, // raw `keccak256(abi.encode(target, data));`
    pub mpt_proof: Vec<Bytes>, // contract-specific MPT proof
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ContractStorage {
    pub address: Address,
    pub expected_value: TrieAccount,
    pub mpt_proof: Vec<Bytes>, // global MPT proof
    pub storage_slots: Vec<StorageSlot>,
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
    pub contract_storage: ContractStorage,
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

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct ExecutionStateProof {
    #[serde(rename = "executionStateRoot")]
    pub execution_state_root: B256,
    #[serde(rename = "executionStateBranch")]
    pub execution_state_branch: Vec<B256>,
    pub gindex: String,
}

/*pub struct VerifiedContractStorageSlot {
    pub slot_key_address: Address,
    pub value: U256,
}*/

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofOutputs {
    pub input_slot: u64,                            // [  0..  8] u64
    pub input_store_hash: B256,                     // [  8.. 40] bytes32
    pub output_slot: u64,                           // [ 40.. 48] u64
    pub output_store_hash: B256,                    // [ 48.. 80] bytes32
    pub execution_state_root: B256,                 // [ 80..112] bytes32
    pub verified_contract_storage_slots_root: B256, // [112..144] bytes32
    pub next_sync_committee_hash: B256,             // [144..176] bytes32
}

impl ProofOutputs {
    pub const SIZE: usize = 176;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];

        buf[0..8].copy_from_slice(&self.input_slot.to_be_bytes());
        buf[8..40].copy_from_slice(&self.input_store_hash.0);
        buf[40..48].copy_from_slice(&self.output_slot.to_be_bytes());
        buf[48..80].copy_from_slice(&self.output_store_hash.0);
        buf[80..112].copy_from_slice(&self.execution_state_root.0);
        buf[112..144].copy_from_slice(&self.verified_contract_storage_slots_root.0);
        buf[144..176].copy_from_slice(&self.next_sync_committee_hash.0);

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
        let verified_contract_storage_slots_root = B256::from_slice(&bytes[112..144]);
        let next_sync_committee_hash = B256::from_slice(&bytes[144..176]);

        Ok(Self {
            input_slot,
            input_store_hash,
            output_slot,
            output_store_hash,
            execution_state_root,
            verified_contract_storage_slots_root,
            next_sync_committee_hash,
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
}

impl ConsensusProofOutputs {
    pub const SIZE: usize = 144;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];

        buf[0..8].copy_from_slice(&self.input_slot.to_be_bytes());
        buf[8..40].copy_from_slice(&self.input_store_hash.0);
        buf[40..48].copy_from_slice(&self.output_slot.to_be_bytes());
        buf[48..80].copy_from_slice(&self.output_store_hash.0);
        buf[80..112].copy_from_slice(&self.execution_state_root.0);
        buf[112..144].copy_from_slice(&self.next_sync_committee_hash.0);

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
        let next_sync_committee_hash = B256::from_slice(&bytes[144..176]);

        Ok(Self {
            input_slot,
            input_store_hash,
            output_slot,
            output_store_hash,
            execution_state_root,
            next_sync_committee_hash
        })
    }
}

// TODO FIX ME FIND A BETTER PLACE FOR THIS!

/// Returns the storage slot for a given address in a mapping at the specified index
///
/// This follows Solidity's ABI encoding rules:
/// 1. Each value is padded to 32 bytes
/// 2. Address is left-padded with zeros (12 bytes of zeros, then 20 bytes of address)
/// 3. Uint8 is right-padded with zeros (1 byte value, then 31 bytes of zeros)
/// 4. The encoding is big-endian
pub fn get_storage_location_for_key(address: Address, mapping_index: u8) -> B256 {
    // Encode the key and index as Solidity's abi.encode would
    let mut encoded = vec![0u8; 64]; // 2 × 32 bytes

    // Place address (padded to 32 bytes) in first slot
    // Address is already 20 bytes, so we need to place it at offset 12 (32-20)
    encoded[12..32].copy_from_slice(address.as_slice());

    // Place mapping_index (padded to 32 bytes) in second slot
    encoded[63] = mapping_index;

    // Hash the encoded data to get the storage slot
    keccak256(&encoded)
}
