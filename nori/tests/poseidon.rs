/*use nori::helios::get_client;
use nori_hash::poseidon_hash::poseidon_hash_helios_store;
use tokio::time::Instant;

// Import test utils
mod test_utils;
use test_utils::hex_to_fixed_bytes;

#[tokio::test]
async fn serialize_helios_store_test() {
    dotenv::dotenv().ok();

    let hex_str = "bd5c14018b693f33a389d9d63ce0033bd65bc0472fb06e07e1cf25f2945f4d84";
    let helios_checkpoint = hex_to_fixed_bytes(hex_str);

    println!("helios_checkpoint {}", helios_checkpoint);

    // Get the client from the beacon checkpoint
    let helios_polling_client = get_client(helios_checkpoint).await.unwrap();

    let before = Instant::now();
    let hash = poseidon_hash_helios_store(&helios_polling_client.store).unwrap();
    let elapsed = Instant::now().duration_since(before).as_secs_f64();

    //let hex = bytes_to_hex(&hash);
    let hex = format!("{:02x}", hash);

    println!("hex {} calculate in {} seconds", hex, elapsed);

    assert!(hex == "a63ffae75aba725f2b7ddf91006c3266da81d46d61ecb9ee15ff52bebe56bc1b")
}*/ // Deprecated
