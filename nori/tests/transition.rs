use alloy_primitives::B256;
use nori::helios::{get_checkpoint, get_client, get_finality_updates};
use tree_hash::TreeHash;

#[tokio::test]
async fn transition() {
    dotenv::dotenv().ok();
    // {"slot_head":11457376,"next_sync_committee":"0x645786a3a13bd020081a5cd29c88d891a069647f693eeac0d1e68d8f5bc1723c","store_hash":"0x18803bbc98959e260477aa6478e5f3bb162120ff20de9afcf85cc393ad710f3f"}
    // Get latest beacon checkpoint
    let helios_checkpoint = get_checkpoint(11457376).await.unwrap();

    let mut helios_update_client = get_client(helios_checkpoint).await.unwrap();

    let str = serde_json::to_string(&helios_update_client.store).unwrap();

    println!("{}", str);

    let mut sync_committee_updates = get_finality_updates(&helios_update_client).await.unwrap();

    if !sync_committee_updates.is_empty() {
        let next_sync_committee = B256::from_slice(
            sync_committee_updates[0]
                .next_sync_committee()
                .tree_hash_root()
                .as_ref(),
        );

        println!("next_sync_committee {}", next_sync_committee);

        let temp_update = sync_committee_updates.remove(0);

        println!("temp_update {}", serde_json::to_string(&temp_update).unwrap());
    }

    // Re init helios client
    //
}
