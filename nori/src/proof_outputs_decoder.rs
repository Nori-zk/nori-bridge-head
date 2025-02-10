use alloy_primitives::{B256, U256};
use serde::{Deserialize, Serialize};
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedProofOutputs {
    pub execution_state_root: B256,
    pub new_header: B256,
    pub next_sync_committee_hash: B256,
    pub new_head: U256,
    pub prev_header: B256,
    pub prev_head: U256,
    pub sync_committee_hash: B256,
    pub start_sync_committee_hash: B256
}

impl DecodedProofOutputs {
    pub fn from_abi(bytes: &[u8]) -> Result<Self> {
        // Ensure the bytes have the correct length (224 bytes for all fields)
        if bytes.len() != 256 {
            return Err(anyhow::anyhow!("Invalid byte slice length"));
        }

        // Extract the byte slices for each field
        let execution_state_root = B256::from_slice(&bytes[0..32]);
        let new_header = B256::from_slice(&bytes[32..64]);
        let next_sync_committee_hash = B256::from_slice(&bytes[64..96]);

        // Convert uint256 (big-endian bytes) into U256
        let new_head_bytes: [u8; 32] = bytes[96..128]
            .try_into()
            .map_err(|_| anyhow::anyhow!("Failed to convert new_head"))?;

        let new_head = U256::from_be_bytes(new_head_bytes);

        let prev_header = B256::from_slice(&bytes[128..160]);
        let prev_head_bytes: [u8; 32] = bytes[160..192]
            .try_into()
            .map_err(|_| anyhow::anyhow!("Failed to convert prev_head"))?;

        let prev_head = U256::from_be_bytes(prev_head_bytes);
        let sync_committee_hash = B256::from_slice(&bytes[192..224]);
        let start_sync_committee_hash = B256::from_slice(&bytes[224..256]);

        // Return the decoded struct
        Ok(DecodedProofOutputs {
            execution_state_root,
            new_header,
            next_sync_committee_hash,
            new_head,
            prev_header,
            prev_head,
            sync_committee_hash,
            start_sync_committee_hash
        })
    }
}