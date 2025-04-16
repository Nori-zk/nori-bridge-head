#[allow(unused_imports)]
use std::{env, fs};
use sp1_build::{build_program_with_args, BuildArgs};
use sp1_sdk::{HashableKey, ProverClient};

fn main() {
    // Determine the current project directory (where Cargo.toml is located)
    let project_dir = env::current_dir().expect("Failed to get current directory");
    let cargo_dir = project_dir.parent().expect("Failed to find project root directory");

    // Use the correct relative paths based on the project root
    let nori_program_path = cargo_dir.join("nori-program");
    let nori_elf_dir = cargo_dir.join("nori-elf");
    let elf_path = nori_elf_dir.join("nori-sp1-helios-program");

    // Build the program using the relative paths
    build_program_with_args(
        nori_program_path.to_str().expect("Invalid path"),
        BuildArgs {
            docker: true,
            tag: "v4.1.3".to_string(),
            output_directory: Some(nori_elf_dir.to_str().expect("Invalid path").to_string()),
            ..Default::default()
        },
    );

    println!("ZK built.");

    if elf_path.exists() {
        // Read the program
        let elf_bytes = fs::read(&elf_path).expect("Failed to read ELF file");

        // Setup the client to generate the VK
        let client = ProverClient::from_env();
        let (_pk, vk) = client.setup(&elf_bytes);

        // Print the VK
        println!("VK: {}", vk.vk.bytes32());

        // Create a JSON string for the VK
        let json_string = format!("\"{}\"", vk.vk.bytes32());

        // Set the output file to .vk.json
        let vk_json_path = elf_path.with_extension("vk.json");

        // Write the file
        fs::write(&vk_json_path, json_string).expect("Failed to write VK JSON file");
        println!("VK written to {:?}", vk_json_path);

    } else {
        println!("ELF file does not exist yet. Skipping client setup.");
    }
}
