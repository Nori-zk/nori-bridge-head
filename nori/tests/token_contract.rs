use ethers::contract::Abigen;
use std::path::Path;
use std::{env, fs};

#[test]
fn generate_token_bridge_bindings() {
    // Get the project root from cargo manifest dir
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let project_root = Path::new(manifest_dir).parent().unwrap();

    println!("fuck {}", project_root.display());
    
    // Input ABI path (relative to project root)
    let abi_path = project_root
        .join("nori-contracts")
        .join("artifacts")
        .join("contracts")
        .join("NoriTokenBridge.sol")
        .join("NoriTokenBridge.json");

    // Debug output to verify paths
    println!("Looking for ABI at: {}", abi_path.display());
    println!("Absolute ABI path: {}", fs::canonicalize(&abi_path).unwrap().display());

    // Output bindings path (relative to project root)
    let out_dir = project_root.join("nori").join("src").join("contract_watcher").join("bindings");
    let out_path = out_dir.join("nori_token_bridge.rs");

    // Generate bindings
    fs::create_dir_all(&out_dir).expect("Failed to create output directory");
    
    let abi_content = fs::read_to_string(&abi_path)
        .unwrap_or_else(|_| panic!("Failed to read ABI file at {}", abi_path.display()));

    Abigen::new("NoriTokenBridge", abi_content)
        .expect("Failed to parse ABI")
        .generate()
        .expect("Failed to generate bindings")
        .write_to_file(&out_path)
        .expect("Failed to write bindings file");

    println!("Successfully generated bindings at: {}", out_path.display());
}