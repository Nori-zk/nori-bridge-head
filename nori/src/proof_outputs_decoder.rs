use alloy_primitives::{Address, B256, U256};
use anyhow::{anyhow, Result};
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

impl DecodedProofOutputs {
    pub fn from_abi(bytes: &[u8]) -> Result<Self> {
        // Ensure the bytes have the correct length (224 bytes for all fields)
        if bytes.len() != 320 {
            return Err(anyhow!("Invalid byte slice length"));
        }

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

        let offset = U256::from_be_bytes(offset_bytes)
            .try_into()
            .map_err(|_| anyhow!("Storage slots offset conversion failed"))?;

        // Validate array data location
        if offset + 32 > bytes.len() {
            return Err(anyhow!("Storage slots data out of bounds"));
        }

        // Parse array length
        let len_start = offset;
        let len_bytes: [u8; 32] = bytes[len_start..len_start + 32]
            .try_into()
            .map_err(|_| anyhow!("Invalid array length slice"))?;

        let array_len = U256::from_be_bytes(len_bytes)
            .try_into()
            .map_err(|_| anyhow!("Invalid array length value"))?;

        // Parse each storage slot
        let mut verified_storage_slots = Vec::with_capacity(array_len);
        for i in 0..array_len {
            let start = offset + 32 + i * 96;
            if start + 96 > bytes.len() {
                return Err(anyhow!("Storage slot {} out of bounds", i));
            }

            let slot_bytes = &bytes[start..start + 96];
            let key = B256::from_slice(&slot_bytes[0..32]);
            let value = B256::from_slice(&slot_bytes[32..64]);

            // Extract address from last 20 bytes of 32-byte field
            let address_bytes: [u8; 20] = slot_bytes[76..96].try_into()?;
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
