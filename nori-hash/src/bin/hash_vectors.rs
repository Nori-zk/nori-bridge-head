use mina_curves::pasta::Fp;
use mina_poseidon::{
    constants::PlonkSpongeConstantsKimchi,
    pasta::{fp_kimchi, FULL_ROUNDS},
    poseidon::{ArithmeticSponge as Poseidon, Sponge as _},
};
use o1_utils::FieldHelpers;
use serde_json::{json, Value};

fn poseidon_hash(input: &[Fp]) -> Fp {
    let mut hash =
        Poseidon::<Fp, PlonkSpongeConstantsKimchi, FULL_ROUNDS>::new(fp_kimchi::static_params());
    hash.absorb(input);
    hash.squeeze()
}

fn fp_to_decimal(fp: Fp) -> String {
    fp.to_biguint().to_string()
}

fn fp_from_u64(n: u64) -> Fp {
    Fp::from(n)
}

fn main() {
    let mut vectors: Vec<Value> = Vec::new();

    // hash([i]) for i = 0..10000
    for i in 0u64..10000 {
        vectors.push(json!({
            "inputs": [i],
            "output": fp_to_decimal(poseidon_hash(&[fp_from_u64(i)]))
        }));
    }

    // hash([i, i+1]) for i = 0..5000
    for i in 0u64..5000 {
        vectors.push(json!({
            "inputs": [i, i + 1],
            "output": fp_to_decimal(poseidon_hash(&[fp_from_u64(i), fp_from_u64(i + 1)]))
        }));
    }

    // hash([i, i+1, i+2]) for i = 0..5000
    for i in 0u64..5000 {
        vectors.push(json!({
            "inputs": [i, i + 1, i + 2],
            "output": fp_to_decimal(poseidon_hash(&[
                fp_from_u64(i),
                fp_from_u64(i + 1),
                fp_from_u64(i + 2),
            ]))
        }));
    }

    println!("{}", serde_json::to_string(&vectors).unwrap());
}
