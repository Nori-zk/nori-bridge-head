use anyhow::Result;
use helios_consensus_core::{consensus_spec::MainnetConsensusSpec, types::LightClientStore};
use nori_hash::sha256_hash::sha256_hash_helios_store;
use nori_sp1_helios_program::consensus::zk_consensus_mpt_program;

static OLD_STORE_JSON: &str = include_str!("./data/store.11457376.json");

// This is from test with output normal-before-sync-committe-change.3.finalitychanges "Hashing updated store." 
// Its the serialized state of the store before the linear hash
// This represents an interesting scenario of an old checkpoint where the call to the RPC
// yields a zeroth update which has a finalized header slot of '11459104'
// which represents a period of 11459104/(32*256) = 1398.81640625
// This is very late in the period and is after the checkpoint value.
// This test aims to ensure we can sync our client and apply updates correctly.
// Previously the code erroneously applied the 0th update which was ahead of the checkpoint
// outside of the ZK, this had security implications.

/*#[tokio::test]
async fn store_sync() -> Result<()> {
    dotenv::dotenv().ok();
    // Reconstruct the old stores state...

    let old_store: LightClientStore<MainnetConsensusSpec> =
        serde_json::from_str(OLD_STORE_JSON).unwrap();

    // We need to hash this and compare it to our reconstructed store....(reconstructed from the checkpoint using the hack method)

    let old_store_hash = sha256_hash_helios_store(&old_store)?;

    // Prepare zk input

    let input_slot: u64 = 11457376;

    // [2025-04-10T19:37:17Z INFO  nori::sp1_prover] Getting finality update from input_head 11457376, store hash 0x18803bbc98959e260477aa6478e5f3bb162120ff20de9afcf85cc393ad710f3f

    let zk_input = prepare_zk_program_input(input_slot, old_store_hash).await?;

    // Now do the zk logic:

    zk_program(zk_input);

    Ok(())
}
*/