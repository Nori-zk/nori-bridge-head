use std::env;
use std::path::PathBuf;
use std::process;

/// Check if npm is install (works for windows / mac)
fn is_npm_installed() -> bool {
    process::Command::new("npm")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Install @nori-zk/ethereum-token-bridge via npm
fn install_solidity_contracts(contracts_dir: PathBuf) -> bool {
    process::Command::new("npm")
        .arg("ci")
        .current_dir(contracts_dir)
        .output()
        .map(|out| {
            if !out.status.success() {
                eprintln!("cargo:error=npm error: {}", String::from_utf8_lossy(&out.stderr));
            }
            out.status.success()
        })
        .unwrap_or_else(|e| {
            eprintln!("cargo:error=Could not execute npm: {}", e);
            false
        })
}

/// Pre-build hook
fn main() {
    let contracts_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let contracts_npm_dir = contracts_dir.join("node_modules/@nori-zk/ethereum-token-bridge");
    let abi_path = contracts_dir.join("node_modules/@nori-zk/ethereum-token-bridge/build/artifacts/contracts/NoriTokenBridge.sol/NoriTokenBridge.json");
    let gen_path = contracts_dir.join("src/lib.rs");

    if contracts_npm_dir.exists() {
        return;
    }

    // Tell Cargo when to re-run this script
    println!("cargo:rerun-if-changed={}", contracts_dir.join("package.json").display());
    println!("cargo:rerun-if-changed={}", contracts_dir.join("package-lock.json").display());

    println!("cargo:info=Solidity contracts package @nori-zk/ethereum-token-bridge is not installed. Attempting to install them.");

    if !is_npm_installed() {
        println!("cargo:warning=This project needs npm installed, in order to install solidity contracts, which are defined in an external package: @nori-zk/ethereum-token-bridge");
        process::exit(1);
    }

    if !install_solidity_contracts(contracts_dir) {
        process::exit(1);
    }

    // 2. Create the bindings file ONLY after npm is done
    // We use a raw string so the path in the macro is absolute
    let content = format!(
        r#"use alloy::sol;
sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    NoriStateBridge,
    "{}"
);"#,
        abi_path.to_str().unwrap().replace("\\", "/") // Ensure cross-platform paths
    );

    std::fs::write(gen_path, content).expect("Failed to write generated_bindings.rs");
}
