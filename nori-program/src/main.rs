#![no_main]
sp1_zkvm::entrypoint!(main);
use alloy_sol_types::SolValue;
use nori_sp1_helios_program::sp1_helios::zk_program;

pub fn main() {
    // Read zk input
    let encoded_inputs = sp1_zkvm::io::read_vec();

    // Decode inputs
    println!("Decoding inputs");
    let proof_inputs = serde_cbor::from_slice(&encoded_inputs).unwrap();
    println!("Decoded inputs");

    // Run nori sp1 helios zk program
    let proof_outputs = zk_program(proof_inputs);

    // Write zk output
    sp1_zkvm::io::commit_slice(&proof_outputs.abi_encode());
}
