use alloy_primitives::{Address, Bytes, FixedBytes, B256, U256};
use alloy_sol_types::sol;
use alloy_trie::TrieAccount;
use helios_consensus_core::consensus_spec::ConsensusSpec;
use helios_consensus_core::types::Forks;
use helios_consensus_core::types::{FinalityUpdate, LightClientStore, Update};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StorageSlot {
    pub key: B256,             // raw 32 byte storage slot key e.g. for slot 0: 0x000...00
    pub expected_value: U256, // raw `keccak256(abi.encode(target, data));` that we store in `HubPoolStore.sol`
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

sol! {
    struct VerifiedContractStorageSlot {
        bytes32 key;
        bytes32 value;
        address contractAddress;
    }

    struct ProofOutputs {
        bytes32 executionStateRoot; //0-31 [0..32]
        bytes32 newHeader; //32->63 [32..64]
        bytes32 nextSyncCommitteeHash; //64->95 [64..96]
        uint256 newHead; //96->127 [96..128]
        bytes32 prevHeader; //128->159 [128..160]
        uint256 prevHead; //160->191 [160..192]
        bytes32 syncCommitteeHash; //192->223 [192..224]
        bytes32 startSyncCommitteeHash; //224->255 [224..256]
        bytes32 prevStoreHash; //256-> 287 [256..288]
        bytes32 storeHash; //288->319 [288..320]
        VerifiedContractStorageSlot[] verifiedContractStorageSlots; // 32 bytes offset [320..352] and length. [352..384]... then [32 byte key, value and an address??]
    }

    struct ConsensusProofOutputs {
        bytes32 executionStateRoot;
        bytes32 newHeader;
        bytes32 nextSyncCommitteeHash;
        uint256 newHead;
        bytes32 prevHeader;
        uint256 prevHead;
        bytes32 syncCommitteeHash;
        bytes32 startSyncCommitteeHash;
        bytes32 prevStoreHash;
        bytes32 storeHash;
    }
}
