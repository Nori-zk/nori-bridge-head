#![no_main]
sp1_zkvm::entrypoint!(main);
use alloy_sol_types::SolValue;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use nori_sp1_helios_primitives::types::ProofInputs;
use nori_sp1_helios_program::consensus::zk_consensus_mpt_program;

pub fn main() {
    // Read zk input
    let encoded_inputs = sp1_zkvm::io::read_vec();

    // Decode inputs
    println!("Decoding inputs");
    let proof_inputs: ProofInputs<MainnetConsensusSpec> = serde_cbor::from_slice(&encoded_inputs).unwrap();
    println!("Decoded inputs");

    // Run nori sp1 helios zk program
    let proof_outputs = zk_consensus_mpt_program(proof_inputs);

    // Write zk output
    sp1_zkvm::io::commit_slice(&proof_outputs.abi_encode());
}
