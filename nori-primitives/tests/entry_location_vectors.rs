//! Renders shared test vectors for queue entry storage locations to
//! `test-vectors/proof-request-queue/entry-location-vectors.json`.
//!
//! Each case pins the five consecutive storage keys the guest derives for
//! queue entry `index`, against the `_requests` mapping at
//! `QUEUE_REQUESTS_STORAGE_INDEX`. Solidity must produce the same keys for
//! `_requests[index]`'s five words.
//!
//! Run: `cargo test -p nori-sp1-helios-primitives --test entry_location_vectors`

mod entry_location {
    use alloy_primitives::{hex, U256};
    use nori_sp1_helios_primitives::storage_layout::{
        mapping_entry_location, storage_slot_of_index, struct_word_slot, QUEUE_ENTRY_WORDS,
        QUEUE_HEAD_STORAGE_INDEX, QUEUE_REQUESTS_STORAGE_INDEX,
    };
    use serde_json::{json, Value};
    use std::path::PathBuf;

    /// Word order within one entry, matching `NoriProofRequestQueue.Request`.
    const WORD_NAMES: [&str; QUEUE_ENTRY_WORDS] = [
        "target",
        "slotKey",
        "collectionKeysCount",
        "collectionKeys0",
        "collectionKeys1",
    ];

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../test-vectors/proof-request-queue/entry-location-vectors.json")
    }

    #[test]
    fn entry_location_vectors_render() {
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
                let base_u256 = U256::from_be_bytes(base.0);

                let words: Vec<Value> = (0..QUEUE_ENTRY_WORDS)
                    .map(|word_index| {
                        let slot = struct_word_slot(base, word_index as u8);
                        let slot_u256 = U256::from_be_bytes(slot.0);
                        assert_eq!(
                            slot_u256 - base_u256,
                            U256::from(word_index as u8),
                            "index {}, word {}: struct_word_slot is not base + word_index",
                            index,
                            word_index
                        );

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

        std::fs::write(
            fixture_path(),
            format!("{}\n", serde_json::to_string_pretty(&vectors).unwrap()),
        )
        .expect("write entry-location-vectors.json");
    }
}
