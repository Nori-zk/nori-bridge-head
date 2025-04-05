use alloy_primitives::FixedBytes;
use std::fmt::Write;

pub fn bytes_to_hex(vec: &[u8]) -> String {
    vec.iter()
        .fold(String::with_capacity(vec.len() * 2), |mut output, &byte| {
            write!(&mut output, "{:02x}", byte).expect("Failed to write hex byte");
            output
        })
}

pub fn hex_char_to_nibble(c: u8) -> Result<u8, String> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(format!("Invalid hex character: {}", c as char)),
    }
}

pub fn hex_to_fixed_bytes(hex_str: &str) -> FixedBytes<32> {
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

pub fn hex_to_u64(hex_str: &str) -> Result<u64, String> {
    // Strip 0x prefix if it exists
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);

    // Ensure that the string length is appropriate for a u64 (16 hex digits)
    if hex_str.len() != 16 {
        return Err(format!("Invalid hex length for u64 conversion. Expected 16 characters but got {}", hex_str.len()));
    }

    let mut result = 0u64;
    
    for (i, c) in hex_str.chars().enumerate() {
        let nibble = hex_char_to_nibble(c as u8)?;
        result |= (nibble as u64) << ((15 - i) * 4);
    }

    Ok(result)
}