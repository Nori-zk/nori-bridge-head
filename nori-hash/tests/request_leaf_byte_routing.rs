//! Proves every one of the 117 input bytes hash_request_leaf packs (target
//! 20, count 1, key_0 32, key_1 32, value 32) lands in the correct output
//! field at the correct offset, checked independently of
//! pack_request_leaf_fields itself via arithmetic on the documented field
//! layout. Also renders the checked cases to
//! `test-vectors/proof-request-queue/request-leaf-vectors.json`, which the
//! o1js `provableRequestLeafHash` must reproduce every `leaf` value from.
//!
//! Run: `cargo test -p nori-hash --test request_leaf_byte_routing`

mod byte_routing {
    use alloy_primitives::{hex, Address, B256, U256};
    use mina_curves::pasta::Fp;
    use nori_hash::merkle_poseidon_fixed::{hash_request_leaf, pack_request_leaf_fields};
    use o1_utils::FieldHelpers;
    use serde_json::{json, Value};
    use std::path::PathBuf;

    struct Case {
        name: String,
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

    const SINGLE_BYTE_MARKER: u8 = 0xa5;

    fn field_at(byte_index: usize) -> &'static str {
        match byte_index {
            0..=19 => "target",
            20 => "count",
            21..=52 => "key_0",
            53..=84 => "key_1",
            85..=116 => "value",
            _ => unreachable!("byte_index out of range for a 117-byte hash_request_leaf payload"),
        }
    }

    // (field index 0..4, byte offset within that field's 32-byte buffer) for
    // where byte_index lands in pack_request_leaf_fields' output.
    fn field_and_offset(byte_index: usize) -> (usize, usize) {
        match byte_index {
            0..=19 => (0, byte_index),
            20 => (0, 20),
            21 => (0, 21),
            22..=52 => (1, byte_index - 22),
            53 => (0, 22),
            54..=84 => (2, byte_index - 54),
            85 => (0, 23),
            86..=116 => (3, byte_index - 86),
            _ => unreachable!("byte_index out of range for a 117-byte hash_request_leaf payload"),
        }
    }

    // Expected pack_request_leaf_fields output for a single-byte case, from
    // the documented field layout.
    fn expected_fields(byte_index: usize) -> [Fp; 4] {
        let (field_index, offset) = field_and_offset(byte_index);
        let mut bytes = [0u8; 32];
        bytes[offset] = SINGLE_BYTE_MARKER;
        let mut fields = [Fp::from(0u64); 4];
        fields[field_index] = Fp::from_bytes(&bytes).expect("marker byte fits a single field");
        fields
    }

    fn single_byte_case(byte_index: usize) -> Case {
        let mut target_bytes = [0u8; 20];
        let mut count = 0u8;
        let mut key_0_bytes = [0u8; 32];
        let mut key_1_bytes = [0u8; 32];
        let mut value_bytes = [0u8; 32];

        match byte_index {
            0..=19 => target_bytes[byte_index] = SINGLE_BYTE_MARKER,
            20 => count = SINGLE_BYTE_MARKER,
            21..=52 => key_0_bytes[byte_index - 21] = SINGLE_BYTE_MARKER,
            53..=84 => key_1_bytes[byte_index - 53] = SINGLE_BYTE_MARKER,
            85..=116 => value_bytes[byte_index - 85] = SINGLE_BYTE_MARKER,
            _ => unreachable!("byte_index out of range for a 117-byte hash_request_leaf payload"),
        }

        let target = Address::from(target_bytes);
        let collection_key_0 = B256::from(key_0_bytes);
        let collection_key_1 = B256::from(key_1_bytes);
        let value = U256::from_be_bytes(value_bytes);

        let actual =
            pack_request_leaf_fields(&target, count, &collection_key_0, &collection_key_1, &value)
                .expect("packing succeeds for a single marker byte");
        let expected = expected_fields(byte_index);
        assert_eq!(
            actual, expected,
            "byte {} ({}): pack_request_leaf_fields produced {:?}, expected {:?}",
            byte_index, field_at(byte_index), actual, expected
        );

        Case {
            name: format!("byte_{:03}_{}", byte_index, field_at(byte_index)),
            target,
            collection_keys_count: count,
            collection_key_0,
            collection_key_1,
            value,
        }
    }

    fn single_byte_cases() -> Vec<Case> {
        (0..117).map(single_byte_case).collect()
    }

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../test-vectors/proof-request-queue/request-leaf-vectors.json")
    }

    fn all_cases() -> Vec<Case> {
        let mut cases = vec![
            Case {
                name: "all_zero".to_string(),
                target: Address::ZERO,
                collection_keys_count: 0,
                collection_key_0: B256::ZERO,
                collection_key_1: B256::ZERO,
                value: U256::ZERO,
            },
            Case {
                name: "one_key".to_string(),
                target: Address::repeat_byte(0x11),
                collection_keys_count: 1,
                collection_key_0: B256::repeat_byte(0x22),
                collection_key_1: B256::ZERO,
                value: U256::from(7u64),
            },
            // Same inputs as one_key: only the target differs, so the leaves must too.
            Case {
                name: "one_key_foreign_target".to_string(),
                target: Address::repeat_byte(0xff),
                collection_keys_count: 1,
                collection_key_0: B256::repeat_byte(0x22),
                collection_key_1: B256::ZERO,
                value: U256::from(7u64),
            },
            // Same inputs as one_key: only the count differs, so the leaves must too.
            Case {
                name: "two_keys_second_zero".to_string(),
                target: Address::repeat_byte(0x11),
                collection_keys_count: 2,
                collection_key_0: B256::repeat_byte(0x22),
                collection_key_1: B256::ZERO,
                value: U256::from(7u64),
            },
            Case {
                name: "two_keys".to_string(),
                target: Address::repeat_byte(0x11),
                collection_keys_count: 2,
                collection_key_0: B256::repeat_byte(0x22),
                collection_key_1: B256::repeat_byte(0x33),
                value: U256::from(1_000_000u64),
            },
            // Exercises the bytes packed into field 1 separately from their tails.
            Case {
                name: "leading_bytes_only".to_string(),
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
                name: "max_bytes".to_string(),
                target: Address::repeat_byte(0xff),
                collection_keys_count: u8::MAX,
                collection_key_0: B256::repeat_byte(0xff),
                collection_key_1: B256::repeat_byte(0xff),
                value: U256::MAX,
            },
            // Shaped like a bridge deposit: keccak-like key, bridge-unit value.
            Case {
                name: "bridge_deposit".to_string(),
                target: Address::from(hex!("00000000219ab540356cbb839cbe05303d7705fa")),
                collection_keys_count: 1,
                collection_key_0: B256::from(hex!(
                    "1b848805a3db129b6b41adca52c9b6f380d58dc9c283f73ce17466a01b90d361"
                )),
                collection_key_1: B256::ZERO,
                value: U256::from(1_000_000u64),
            },
        ];
        cases.extend(single_byte_cases());
        cases
    }

    fn render_vectors(cases: &[Case]) -> Vec<Value> {
        cases
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
            .collect()
    }

    #[test]
    fn every_byte_routes_to_the_correct_field() {
        let vectors = render_vectors(&all_cases());
        std::fs::write(
            fixture_path(),
            format!("{}\n", serde_json::to_string_pretty(&vectors).unwrap()),
        )
        .expect("write request-leaf-vectors.json");
    }
}
