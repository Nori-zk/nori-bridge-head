/// Verification key hash of the Nori SP1 Helios program, as big-endian bytes.
pub const NORI_SP1_HELIOS_PROGRAM_VK: [u8; 32] =
    hex_json_to_bytes(include_bytes!("../nori-sp1-helios-program.vk.json"));

const fn hex_json_to_bytes(json: &[u8]) -> [u8; 32] {
    // json is "0x" followed by 64 hex digits, wrapped in quotes
    assert!(
        json.len() == 68,
        "vk json is not a quoted 32-byte hex string"
    );
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = hex_digit(json[3 + 2 * i]) << 4 | hex_digit(json[4 + 2 * i]);
        i += 1;
    }
    out
}

const fn hex_digit(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("vk json contains a non-hex character"),
    }
}
