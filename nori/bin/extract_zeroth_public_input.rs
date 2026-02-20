use alloy_primitives::U256;
use sp1_sdk::{Elf, HashableKey, Prover, ProverClient, ProvingKey};
use std::{env, fs};

#[tokio::main]
async fn main() {
    // Determine the ELF path.
    let project_dir = env::current_dir().expect("Failed to get current directory");
    let cargo_dir = project_dir.parent().expect("Failed to find project root directory");
    let nori_elf_dir = cargo_dir.join("nori-elf");
    let elf_path = nori_elf_dir.join("nori-sp1-helios-program");
    let output_path = elf_path.with_extension("pi0.json");

    // Load ELF and derive the VK.
    let elf_bytes: &'static [u8] = fs::read(&elf_path).expect("Failed to read ELF file").leak();
    let client = ProverClient::builder().mock().build().await;
    let pk = client.setup(Elf::Static(elf_bytes)).await.expect("Failed to setup proving key");

    // bytes32() = "0x<hex>" of hash_bn254() — same value as public_inputs[0] in a real Plonk proof.
    let vk_bytes32 = pk.verifying_key().bytes32();
    let hex = vk_bytes32.strip_prefix("0x").expect("bytes32 should start with 0x");
    let decimal_pi0 = U256::from_str_radix(hex, 16).expect("invalid hex").to_string();

    println!("pi0: {}", decimal_pi0);

    let json_string = format!("\"{}\"", decimal_pi0);
    println!("Attempting to write to {:?}", output_path);
    fs::write(&output_path, json_string).expect("Failed to write pi0 JSON file");
    println!("plonk pi0 written to {:?}", output_path);
}
