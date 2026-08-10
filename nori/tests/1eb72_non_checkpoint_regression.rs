use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use nori_sp1_helios_primitives::types::ProofInputs;
use nori_sp1_helios_program::consensus::consensus_mpt_program;
use std::path::PathBuf;

#[test]
fn consensus_mpt_program_accepts_non_checkpoint_slot() {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("non_checkpoint_proof_inputs.10650047.cbor");

    let cbor_bytes = std::fs::read(&fixture_path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", fixture_path.display(), e));

    let proof_inputs: ProofInputs<MainnetConsensusSpec> =
        serde_cbor::from_slice(&cbor_bytes).expect("failed to deserialize fixture");

    let result = consensus_mpt_program(proof_inputs, false);

    assert!(
        result.is_err(),
        "consensus_mpt_program must reject non-checkpoint slots (1eb72)"
    );
}
