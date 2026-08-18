//! Emits shared test vectors for queue entry storage locations as JSON on stdout.
//!
//! Each case pins the five consecutive storage keys the guest derives for queue
//! entry `index`, against the `_requests` mapping at
//! `QUEUE_REQUESTS_STORAGE_INDEX`. Solidity must produce the same keys for
//! `_requests[index]`'s five words.
//!
//! Run: `cargo run --example entry_location_vectors -p nori-sp1-helios-primitives`

use alloy_primitives::{hex, U256};
use nori_sp1_helios_primitives::types::{
    mapping_entry_location, storage_slot_of_index, struct_word_slot, QUEUE_ENTRY_WORDS,
    QUEUE_HEAD_STORAGE_INDEX, QUEUE_REQUESTS_STORAGE_INDEX,
};
use serde_json::{json, Value};

/// Word order within one entry, matching `NoriProofRequestQueue.Request`.
const WORD_NAMES: [&str; QUEUE_ENTRY_WORDS] = [
    "target",
    "slotKey",
    "collectionKeysCount",
    "collectionKeys0",
    "collectionKeys1",
];

fn main() {
    // Index 0 and 1 cover the base case; the rest catch a truncated or
    // sign-extended key derivation.
    let indices: [U256; 6] = [
        U256::from(0u64),
        U256::from(1u64),
        U256::from(42u64),
        U256::from(u32::MAX),
        U256::from(u64::MAX),
        U256::MAX,
    ];

    let entries: Vec<Value> = indices
        .iter()
        .map(|index| {
            let base = mapping_entry_location(*index, QUEUE_REQUESTS_STORAGE_INDEX);
            let words: Vec<Value> = (0..QUEUE_ENTRY_WORDS)
                .map(|word_index| {
                    let slot = struct_word_slot(base, word_index as u8);
                    json!({
                        "name": WORD_NAMES[word_index],
                        "wordIndex": word_index,
                        "slot": format!("0x{}", hex::encode(slot)),
                    })
                })
                .collect();

            json!({
                "index": index.to_string(),
                "base": format!("0x{}", hex::encode(base)),
                "words": words,
            })
        })
        .collect();

    let vectors = json!({
        "headSlot": format!(
            "0x{}",
            hex::encode(storage_slot_of_index(QUEUE_HEAD_STORAGE_INDEX))
        ),
        "requestsMappingSlotIndex": QUEUE_REQUESTS_STORAGE_INDEX,
        "entryWords": QUEUE_ENTRY_WORDS,
        "entries": entries,
    });

    println!("{}", serde_json::to_string_pretty(&vectors).unwrap());
}
