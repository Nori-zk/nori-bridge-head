use alloy_primitives::{hex, B256, U256};
use serde_json::{json, Value};
use sha2_v0_10_8::{Digest, Sha256};

fn sha256_hash(input: &[B256]) -> B256 {
    let mut hash = Sha256::new();
    for element in input {
        hash.update(element);
    }
    B256::from_slice(&hash.finalize())
}

fn b256_to_hex(b256: B256) -> String {
    format!("0x{}", hex::encode(b256))
}

fn b256_from_u64(n: u64) -> B256 {
    B256::from(U256::from(n))
}

fn main() {
    let mut vectors: Vec<Value> = Vec::new();

    // hash([i]) for i = 0..10000
    for i in 0u64..10000 {
        vectors.push(json!({
            "inputs": [i],
            "output": b256_to_hex(sha256_hash(&[b256_from_u64(i)]))
        }));
    }

    // hash([i, i+1]) for i = 0..5000
    for i in 0u64..5000 {
        vectors.push(json!({
            "inputs": [i, i + 1],
            "output": b256_to_hex(sha256_hash(&[b256_from_u64(i), b256_from_u64(i + 1)]))
        }));
    }

    // hash([i, i+1, i+2]) for i = 0..5000
    for i in 0u64..5000 {
        vectors.push(json!({
            "inputs": [i, i + 1, i + 2],
            "output": b256_to_hex(sha256_hash(&[
                b256_from_u64(i),
                b256_from_u64(i + 1),
                b256_from_u64(i + 2),
            ]))
        }));
    }

    println!("{}", serde_json::to_string(&vectors).unwrap());
}
