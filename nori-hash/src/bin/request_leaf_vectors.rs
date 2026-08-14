//! Emits shared test vectors for `hash_request_leaf` as JSON on stdout.
//!
//! The o1js `provableRequestLeafHash` must reproduce every `leaf` value here.
//! Run: `cargo run --bin request_leaf_vectors > request-leaf-vectors.json`

use alloy_primitives::{hex, Address, B256, U256};
use nori_hash::merkle_poseidon_fixed::hash_request_leaf;
use o1_utils::FieldHelpers;
use serde_json::{json, Value};

struct Case {
    name: &'static str,
    target: Address,
    collection_keys_count: u8,
    collection_key_0: B256,
    collection_key_1: B256,
    value: U256,
}

fn leading_byte(byte: u8) -> B256 {
    let mut bytes = [0u8; 32];
    bytes[0] = byte;
    B256::from(bytes)
}

fn main() {
    let cases = [
        Case {
            name: "all_zero",
            target: Address::ZERO,
            collection_keys_count: 0,
            collection_key_0: B256::ZERO,
            collection_key_1: B256::ZERO,
            value: U256::ZERO,
        },
        Case {
            name: "one_key",
            target: Address::repeat_byte(0x11),
            collection_keys_count: 1,
            collection_key_0: B256::repeat_byte(0x22),
            collection_key_1: B256::ZERO,
            value: U256::from(7u64),
        },
        // Same inputs as one_key: only the count differs, so the leaves must too.
        Case {
            name: "two_keys_second_zero",
            target: Address::repeat_byte(0x11),
            collection_keys_count: 2,
            collection_key_0: B256::repeat_byte(0x22),
            collection_key_1: B256::ZERO,
            value: U256::from(7u64),
        },
        Case {
            name: "two_keys",
            target: Address::repeat_byte(0x11),
            collection_keys_count: 2,
            collection_key_0: B256::repeat_byte(0x22),
            collection_key_1: B256::repeat_byte(0x33),
            value: U256::from(1_000_000u64),
        },
        // Exercises the bytes packed into field 1 separately from their tails.
        Case {
            name: "leading_bytes_only",
            target: Address::ZERO,
            collection_keys_count: 2,
            collection_key_0: leading_byte(0xaa),
            collection_key_1: leading_byte(0xbb),
            value: U256::from_be_bytes({
                let mut bytes = [0u8; 32];
                bytes[0] = 0xcc;
                bytes
            }),
        },
        Case {
            name: "max_bytes",
            target: Address::repeat_byte(0xff),
            collection_keys_count: u8::MAX,
            collection_key_0: B256::repeat_byte(0xff),
            collection_key_1: B256::repeat_byte(0xff),
            value: U256::MAX,
        },
        // Shaped like a bridge deposit: keccak-like key, bridge-unit value.
        Case {
            name: "bridge_deposit",
            target: Address::from(hex!("00000000219ab540356cbb839cbe05303d7705fa")),
            collection_keys_count: 1,
            collection_key_0: B256::from(hex!(
                "1b848805a3db129b6b41adca52c9b6f380d58dc9c283f73ce17466a01b90d361"
            )),
            collection_key_1: B256::ZERO,
            value: U256::from(1_000_000u64),
        },
    ];

    let vectors: Vec<Value> = cases
        .iter()
        .map(|case| {
            let leaf = hash_request_leaf(
                &case.target,
                case.collection_keys_count,
                &case.collection_key_0,
                &case.collection_key_1,
                &case.value,
            )
            .expect("leaf hash");

            json!({
                "name": case.name,
                "target": format!("0x{}", hex::encode(case.target.as_slice())),
                "collectionKeysCount": case.collection_keys_count,
                "collectionKeys": [
                    format!("0x{}", hex::encode(case.collection_key_0.as_slice())),
                    format!("0x{}", hex::encode(case.collection_key_1.as_slice())),
                ],
                "value": format!("0x{}", hex::encode(case.value.to_be_bytes::<32>())),
                "leaf": leaf.to_biguint().to_string(),
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&vectors).unwrap());
}
