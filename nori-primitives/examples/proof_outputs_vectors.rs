//! Emits shared test vectors for the `ProofOutputs` byte encoding as JSON on stdout.
//!
//! Consumers that decode or re-assemble these bytes must reproduce every
//! `bytes` value here from the corresponding fields.
//!
//! Run: `cargo run --example proof_outputs_vectors -p nori-sp1-helios-primitives`

use alloy_primitives::{hex, Address, B256};
use nori_sp1_helios_primitives::types::ProofOutputs;
use serde_json::{json, Value};

struct Case {
    name: &'static str,
    outputs: ProofOutputs,
}

fn byte_filled(byte: u8) -> B256 {
    B256::repeat_byte(byte)
}

fn main() {
    let cases = [
        Case {
            name: "all_zero",
            outputs: ProofOutputs {
                input_slot: 0,
                input_store_hash: B256::ZERO,
                output_slot: 0,
                output_store_hash: B256::ZERO,
                execution_state_root: B256::ZERO,
                verified_contract_storage_slots_root: B256::ZERO,
                next_sync_committee_hash: B256::ZERO,
                proof_request_queue_address: Address::ZERO,
                input_queue_cursor: 0,
                output_queue_cursor: 0,
                output_block_number: 0,
            },
        },
        // Every field distinct, so a transposed pair of offsets shows up.
        Case {
            name: "distinct_fields",
            outputs: ProofOutputs {
                input_slot: 1,
                input_store_hash: byte_filled(0x02),
                output_slot: 3,
                output_store_hash: byte_filled(0x04),
                execution_state_root: byte_filled(0x05),
                verified_contract_storage_slots_root: byte_filled(0x06),
                next_sync_committee_hash: byte_filled(0x07),
                proof_request_queue_address: Address::repeat_byte(0x08),
                input_queue_cursor: 9,
                output_queue_cursor: 10,
                output_block_number: 11,
            },
        },
        // Cursors and block number above 2^32 to catch a 32-bit truncation in
        // either direction.
        Case {
            name: "wide_counters",
            outputs: ProofOutputs {
                input_slot: u64::MAX,
                input_store_hash: byte_filled(0xff),
                output_slot: u64::MAX,
                output_store_hash: byte_filled(0xff),
                execution_state_root: byte_filled(0xff),
                verified_contract_storage_slots_root: byte_filled(0xff),
                next_sync_committee_hash: byte_filled(0xff),
                proof_request_queue_address: Address::repeat_byte(0xff),
                input_queue_cursor: 0x0000_0001_0000_0000,
                output_queue_cursor: 0xffff_ffff_ffff_ffff,
                output_block_number: 0x0000_0001_0000_0000,
            },
        },
        // Shaped like a live update: a non-empty batch drained from a mid-queue cursor.
        Case {
            name: "batch_of_five",
            outputs: ProofOutputs {
                input_slot: 10_887_424,
                input_store_hash: B256::from(hex!(
                    "2f4c9c1d5f6a7b8c9d0e1f2a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e"
                )),
                output_slot: 10_887_488,
                output_store_hash: B256::from(hex!(
                    "3a5d0e2e607b8c9dae1f2a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f"
                )),
                execution_state_root: B256::from(hex!(
                    "4b6e1f3f718c9daebf2a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f80"
                )),
                verified_contract_storage_slots_root: B256::from(hex!(
                    "5c7f2040829daebfc03b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091"
                )),
                next_sync_committee_hash: B256::from(hex!(
                    "6d8031519aaebfc0d14c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2"
                )),
                proof_request_queue_address: Address::from(hex!(
                    "00000000219ab540356cbb839cbe05303d7705fa"
                )),
                input_queue_cursor: 37,
                output_queue_cursor: 42,
                output_block_number: 21_551_842,
            },
        },
    ];

    let vectors: Vec<Value> = cases
        .iter()
        .map(|case| {
            let outputs = &case.outputs;
            let bytes = outputs.to_bytes();

            // Round-trip here so a vector can never record an encoding the
            // decoder disagrees with.
            let decoded = ProofOutputs::from_bytes(&bytes).expect("decode");
            assert_eq!(decoded.to_bytes(), bytes);

            json!({
                "name": case.name,
                "inputSlot": outputs.input_slot.to_string(),
                "inputStoreHash": format!("0x{}", hex::encode(outputs.input_store_hash)),
                "outputSlot": outputs.output_slot.to_string(),
                "outputStoreHash": format!("0x{}", hex::encode(outputs.output_store_hash)),
                "executionStateRoot": format!("0x{}", hex::encode(outputs.execution_state_root)),
                "verifiedContractDepositsRoot": format!(
                    "0x{}",
                    hex::encode(outputs.verified_contract_storage_slots_root)
                ),
                "nextSyncCommitteeHash": format!(
                    "0x{}",
                    hex::encode(outputs.next_sync_committee_hash)
                ),
                "proofRequestQueueAddress": format!(
                    "0x{}",
                    hex::encode(outputs.proof_request_queue_address.as_slice())
                ),
                "inputQueueCursor": outputs.input_queue_cursor.to_string(),
                "outputQueueCursor": outputs.output_queue_cursor.to_string(),
                "outputBlockNumber": outputs.output_block_number.to_string(),
                "bytes": format!("0x{}", hex::encode(bytes)),
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&vectors).unwrap());
}
