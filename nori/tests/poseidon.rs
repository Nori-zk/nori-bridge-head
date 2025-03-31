use alloy_primitives::FixedBytes;
use nori::{
    helios::{get_client, get_latest_checkpoint},
    poseidon_hash::poseidon_hash_helios_store,
};
use tokio::time::Instant;
use std::fmt::{format, Write};

fn bytes_to_hex(vec: &[u8]) -> String {
    vec.iter().fold(String::with_capacity(vec.len() * 2), |mut output, &byte| {
        write!(&mut output, "{:02x}", byte).expect("Failed to write hex byte");
        output
    })
}

fn hex_char_to_nibble(c: u8) -> Result<u8, String> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(format!("Invalid hex character: {}", c as char)),
    }
}

fn hex_to_fixed_bytes(hex_str: &str) -> FixedBytes<32> {
    // Strip 0x prefix and validate length
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    if hex_str.len() != 64 {
        panic!("Invalid hex length for 32-byte conversion");
    }

    let mut array = [0u8; 32];
    let bytes = hex_str.as_bytes();
    
    for i in 0..32 {
        let offset = i * 2;
        let high = hex_char_to_nibble(bytes[offset]).unwrap();
        let low = hex_char_to_nibble(bytes[offset + 1]).unwrap();
        array[i] = (high << 4) | low;
    }
    
    FixedBytes::new(array)
}


#[tokio::test]
async fn serialize_helios_store_test() {
    dotenv::dotenv().ok();

    let hex_str = "bd5c14018b693f33a389d9d63ce0033bd65bc0472fb06e07e1cf25f2945f4d84";
    let helios_checkpoint = hex_to_fixed_bytes(hex_str);

    // Get latest beacon checkpoint
    //let helios_checkpoint = get_latest_checkpoint().await.unwrap();

    print!("helios_checkpoint {}", helios_checkpoint);

    // Get the client from the beacon checkpoint
    let helios_polling_client = get_client(helios_checkpoint).await.unwrap();

    let before = Instant::now();
    let hash = poseidon_hash_helios_store(&helios_polling_client.store).unwrap();
    let elapsed = Instant::now().duration_since(before).as_secs_f64();

    //let hex = bytes_to_hex(&hash);
    let hex = format!("{:02x}", hash);

    println!("hex {} calculate in {} seconds", hex, elapsed);

    assert!(hex == "a63ffae75aba725f2b7ddf91006c3266da81d46d61ecb9ee15ff52bebe56bc1b")
}
