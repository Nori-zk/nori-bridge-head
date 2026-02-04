use std::env;
use std::path::PathBuf;
use std::process::{self, Command};

/// Check if npm is install (works for windows / mac)
fn is_npm_installed() -> bool {
    Command::new("npm")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Install @nori-zk/ethereum-token-bridge via npm
fn install_solidity_contracts(contracts_dir: PathBuf) -> bool {
    Command::new("npm")
        .arg("ci")
        .current_dir(contracts_dir)
        .output()
        .map(|out| {
            if !out.status.success() {
                eprintln!(
                    "cargo:error=npm error: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            out.status.success()
        })
        .unwrap_or_else(|e| {
            eprintln!("cargo:error=Could not execute npm: {}", e);
            false
        })
}

const GENERATED_BINDINGS_HEADER: &str =
    "// @generated: build.rs will overwrite this with alloy::sol! bindings.";

/// Configures Git to ignore local changes to the generated bindings file.
/// This works by telling Git to only "see" the header string when staging.
fn setup_git_ignore_filter() {
    // Check if we are in a git repo before trying to run git commands
    let is_git = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if is_git {
        let clean_cmd = format!("printf '{}'", GENERATED_BINDINGS_HEADER);

        let _ = Command::new("git")
            .args(["config", "filter.ignore-bindings.clean", &clean_cmd])
            .status();

        let _ = Command::new("git")
            .args(["config", "filter.ignore-bindings.smudge", "cat"])
            .status();
    }
}

/// Pre-build hook
fn main() {
    let contracts_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let contracts_npm_dir = contracts_dir.join("node_modules/@nori-zk/ethereum-token-bridge");
    let abi_path = contracts_dir.join("node_modules/@nori-zk/ethereum-token-bridge/build/artifacts/contracts/NoriTokenBridge.sol/NoriTokenBridge.json");
    let gen_path = contracts_dir.join("src/lib.rs");

    // Initialise the self-healing Git filter
    setup_git_ignore_filter();

    // Check if the Solidity contracts are already present in node_modules
    let contracts_installed = contracts_npm_dir.exists();

    // Check if lib.rs exists AND contains actual generated code (not the placeholder)
    let bindings_generated = std::fs::read_to_string(&gen_path)
        .is_ok_and(|content| !content.contains(GENERATED_BINDINGS_HEADER));

    // If we have both the source contracts and the generated bindings, skip the build steps.
    if contracts_installed && bindings_generated {
        return;
    }

    // Tell Cargo when to re-run this script
    println!(
        "cargo:rerun-if-changed={}",
        contracts_dir.join("package.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        contracts_dir.join("package-lock.json").display()
    );

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
