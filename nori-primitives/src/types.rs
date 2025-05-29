use alloy_primitives::{keccak256, Address, Bytes, FixedBytes, B256, U256};
use alloy_sol_types::sol;
use alloy_trie::TrieAccount;
use helios_consensus_core::consensus_spec::ConsensusSpec;
use helios_consensus_core::types::Forks;
use helios_consensus_core::types::{FinalityUpdate, LightClientStore, Update};
use serde::{Deserialize, Serialize};

// TODO FIX ME FIND A BETTER PLACE FOR THIS!
pub const SOURCE_CONTRACT_LOCKED_TOKENS_STORAGE_INDEX: u8 = 1u8;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StorageSlot {
    pub key: B256,                  // raw 32 byte storage slot key e.g. for slot 0: 0x000...00
    pub slot_key_address: Address,  // address associated with the slot key
    pub expected_value: U256,       // raw `keccak256(abi.encode(target, data));`
    pub mpt_proof: Vec<Bytes>,      // contract-specific MPT proof
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
    pub store_hash: FixedBytes<32>,
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
    pub store_hash: FixedBytes<32>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct ExecutionStateProof {
    #[serde(rename = "executionStateRoot")]
    pub execution_state_root: B256,
    #[serde(rename = "executionStateBranch")]
    pub execution_state_branch: Vec<B256>,
    pub gindex: String,
}

// https://docs.soliditylang.org/en/develop/abi-spec.html
sol! {
    struct VerifiedContractStorageSlot {
        address slotKeyAddress;
        bytes32 value;
    } 

    struct ProofOutputs {
        bytes32 executionStateRoot;                                                          //0-31    [0  ..32 ]
        bytes32 newHeader;                                                                   //32-63   [32 ..64 ]
        bytes32 nextSyncCommitteeHash;                                                       //64-95   [64 ..96 ]
        uint256 newHead;                                                                     //96-127  [96 ..128]
        bytes32 prevHeader;                                                                  //128-159 [128..160]
        uint256 prevHead;                                                                    //160-191 [160..192]
        bytes32 syncCommitteeHash;                                                           //192-223 [192..224]
        bytes32 startSyncCommitteeHash;                                                      //224-255 [224..256]
        bytes32 prevStoreHash;                                                               //256-287 [256..288]
        bytes32 storeHash;                                                                   //288-319 [288..320]
        bytes32 verifiedContractStorageSlotsRoot;                                            //320-351 [320..352]
    }

    struct ConsensusProofOutputs {
        bytes32 executionStateRoot;                                                          //0-31    [0  ..32 ]
        bytes32 newHeader;                                                                   //32-63   [32 ..64 ]
        bytes32 nextSyncCommitteeHash;                                                       //64-95   [64 ..96 ]
        uint256 newHead;                                                                     //96-127  [96 ..128]
        bytes32 prevHeader;                                                                  //128-159 [128..160]
        uint256 prevHead;                                                                    //160-191 [160..192]
        bytes32 syncCommitteeHash;                                                           //192-223 [192..224]
        bytes32 startSyncCommitteeHash;                                                      //224-255 [224..256]
        bytes32 prevStoreHash;                                                               //256-287 [256..288]
        bytes32 storeHash;                                                                   //288-319 [288..320]
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

