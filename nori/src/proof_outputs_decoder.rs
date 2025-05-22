use alloy_primitives::{Address, B256, U256};
use anyhow::{anyhow, Result};
use log::info;
use serde::{Deserialize, Serialize};

/*
 struct VerifiedStorageSlot {
        bytes32 key;
        bytes32 value;
        address contractAddress;
    }
*/

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedStorageSlot {
    pub key: B256,
    pub value: B256,
    pub contract_address: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedProofOutputs {
    pub execution_state_root: B256,
    pub new_header: B256,
    pub next_sync_committee_hash: B256,
    pub new_head: U256,
    pub prev_header: B256,
    pub prev_head: U256,
    pub sync_committee_hash: B256,
    pub start_sync_committee_hash: B256,
    pub prev_store_hash: B256,
    pub store_hash: B256,
    pub verified_storage_slots: Vec<VerifiedStorageSlot>,
}

/*
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
*/
impl DecodedProofOutputs {
    pub fn from_abi(bytes: &[u8]) -> Result<Self> {
        // Minimum length is 352 bytes (static fields + array offset).
        if bytes.len() < 352 {
            return Err(anyhow!(
                "Byte slice too short for static fields and array offset"
            ));
        }

        info!(
            "[ABI Decoder] Starting decoding. Input length: {} bytes",
            bytes.len()
        );

        // Extract the byte slices for each field
        let execution_state_root = B256::from_slice(&bytes[0..32]);
        let new_header = B256::from_slice(&bytes[32..64]);
        let next_sync_committee_hash = B256::from_slice(&bytes[64..96]);

        // Convert uint256 (big-endian bytes) into U256
        let new_head_bytes: [u8; 32] = bytes[96..128]
            .try_into()
            .map_err(|_| anyhow!("Failed to convert new_head"))?;

        let new_head = U256::from_be_bytes(new_head_bytes);

        let prev_header = B256::from_slice(&bytes[128..160]);
        let prev_head_bytes: [u8; 32] = bytes[160..192]
            .try_into()
            .map_err(|_| anyhow!("Failed to convert prev_head"))?;

        let prev_head = U256::from_be_bytes(prev_head_bytes);
        let sync_committee_hash = B256::from_slice(&bytes[192..224]);
        let start_sync_committee_hash = B256::from_slice(&bytes[224..256]);

        let prev_store_hash = B256::from_slice(&bytes[256..288]);
        let store_hash = B256::from_slice(&bytes[288..320]);

        // Get storage slots array offset (bytes 320..352)
        let offset_bytes: [u8; 32] = bytes[320..352]
            .try_into()
            .map_err(|_| anyhow!("Invalid storage slots offset"))?;

        info!("offset_bytes {:?}", offset_bytes);

        let offset_u256 = U256::from_be_bytes(offset_bytes);

        info!("offset_u256 {:?}", offset_u256);

        // Parse array length (first 32 bytes at offset)
        let len_bytes: [u8; 32] = bytes[352..384]
            .try_into()
            .map_err(|_| anyhow!("Invalid array length"))?;
        info!("len_bytes {:?}", len_bytes);

        // Check if offset fits in usize and is reasonable
        if offset_u256 > U256::from(usize::MAX) {
            return Err(anyhow!("Storage slots offset too large: {}", offset_u256));
        }

        // Convert to usize safely
        let offset: usize = offset_u256
            .try_into()
            .map_err(|_| anyhow!("Storage slots offset conversion failed: {}", offset_u256))?;

        // Check array data starts within the byte slice
        if offset > bytes.len() || offset + 32 > bytes.len() {
            return Err(anyhow!("Storage slots data out of bounds"));
        }

        let array_len = U256::from_be_bytes(len_bytes)
            .try_into()
            .map_err(|_| anyhow!("Array length too large for usize"))?;

        // Check each element fits within the remaining bytes
        let elements_start = offset + 32;
        let element_size = 96; // 3 * 32 bytes per element
        let total_elements_size = array_len * element_size;
        // Check for overflow or out-of-bounds
        let end_opt = elements_start.checked_add(total_elements_size);
        if end_opt.is_none() || end_opt.unwrap() > bytes.len() {
            return Err(anyhow!("Storage slots exceed byte slice length"));
        }

        // Parse each element
        let mut verified_storage_slots = Vec::with_capacity(array_len);
        for i in 0..array_len {
            let start = elements_start + i * element_size;
            let end = start + element_size;
            let element_bytes = &bytes[start..end];

            let key = B256::from_slice(&element_bytes[0..32]);
            let value = B256::from_slice(&element_bytes[32..64]);
            let address_bytes: [u8; 20] = element_bytes[76..96]
                .try_into()
                .map_err(|_| anyhow!("Invalid address in element {}", i))?;
            let contract_address = Address::from(address_bytes);

            verified_storage_slots.push(VerifiedStorageSlot {
                key,
                value,
                contract_address,
            });
        }

        // Return the decoded struct
        Ok(DecodedProofOutputs {
            execution_state_root,
            new_header,
            next_sync_committee_hash,
            new_head,
            prev_header,
            prev_head,
            sync_committee_hash,
            start_sync_committee_hash,
            prev_store_hash,
            store_hash,
            verified_storage_slots,
        })
    }
}
