#![no_main]
sp1_zkvm::entrypoint!(main);
use alloy_sol_types::SolValue;
use nori_sp1_helios_program::sp1_helios::zk_program;

pub fn main() {
    // Decode zk input
    let encoded_inputs = sp1_zkvm::io::read_vec();
    // Run nori sp1 helios zk program
    let proof_outputs = zk_program(encoded_inputs);
    // Encode and emit zk output
    sp1_zkvm::io::commit_slice(&proof_outputs.abi_encode());
}
