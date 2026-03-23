use std::env;
use std::path::PathBuf;
use std::process::Command;

const GENERATED_BINDINGS_HEADER: &str =
    "// @generated: build.rs will overwrite this with alloy::sol! bindings.";

/// Base URL for raw GitHub content from nori-bridge-sdk
const RAW_BASE: &str = "https://raw.githubusercontent.com/Nori-zk/nori-bridge-sdk";

/// Contract names to generate bindings for.
/// Each entry follows the repo convention: contracts/ethereum/artifacts/contracts/{NAME}.sol/{NAME}.json
const CONTRACTS: &[&str] = &[
    "NoriTokenBridge",
];

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

/// Read the git ref (branch, tag, or commit hash) from bridge-sdk.ref
fn read_bridge_sdk_ref(manifest_dir: &PathBuf) -> String {
    let ref_path = manifest_dir.join("bridge-sdk.ref");
    std::fs::read_to_string(&ref_path)
        .unwrap_or_else(|e| {
            panic!(
                "Failed to read {}: {}. This file must contain a git ref (branch, tag, or commit hash) for nori-bridge-sdk.",
                ref_path.display(),
                e
            )
        })
        .trim()
        .to_string()
}

/// Download a contract ABI JSON from GitHub at the pinned ref into abi/
fn fetch_abi(abi_dir: &PathBuf, git_ref: &str, contract_name: &str) -> PathBuf {
    let url = format!(
        "{}/{}/contracts/ethereum/artifacts/contracts/{}.sol/{}.json",
        RAW_BASE, git_ref, contract_name, contract_name
    );
    let abi_path = abi_dir.join(format!("{}.json", contract_name));

    let body = reqwest::blocking::get(&url)
        .unwrap_or_else(|e| panic!("Failed to fetch ABI from {}: {}", url, e))
        .error_for_status()
        .unwrap_or_else(|e| panic!("Failed to fetch ABI from {}: {}", url, e))
        .bytes()
        .unwrap_or_else(|e| panic!("Failed to read response body from {}: {}", url, e));

    std::fs::write(&abi_path, &body)
        .unwrap_or_else(|e| panic!("Failed to write {}: {}", abi_path.display(), e));

    abi_path
}

/// Pre-build hook
fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let abi_dir = manifest_dir.join("abi");
    let gen_path = manifest_dir.join("src/lib.rs");

    // Initialise the self-healing Git filter
    setup_git_ignore_filter();

    // Re-run when the pinned ref changes
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("bridge-sdk.ref").display()
    );

    // Re-run when the abi/ directory is missing or its contents change
    println!(
        "cargo:rerun-if-changed={}",
        abi_dir.display()
    );

    // Check if lib.rs already has generated bindings
    let bindings_generated = std::fs::read_to_string(&gen_path)
        .is_ok_and(|content| !content.trim().contains(GENERATED_BINDINGS_HEADER.trim()));

    // Check all ABI files are present in abi/
    let all_abis_present = CONTRACTS.iter().all(|name| {
        abi_dir.join(format!("{}.json", name)).exists()
    });

    if all_abis_present && bindings_generated {
        return;
    }

    // Ensure abi/ directory exists
    std::fs::create_dir_all(&abi_dir).expect("Failed to create abi/ directory");

    // Read the pinned git ref and fetch all ABIs
    let git_ref = read_bridge_sdk_ref(&manifest_dir);
    println!("cargo:warning=Fetching contract ABIs from nori-bridge-sdk @ {}", git_ref);

    let mut sol_blocks = Vec::new();
    for contract_name in CONTRACTS {
        let abi_path = fetch_abi(&abi_dir, &git_ref, contract_name);
        sol_blocks.push(format!(
            r#"sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    {},
    "{}"
);"#,
            contract_name,
            abi_path.to_str().unwrap().replace("\\", "/")
        ));
    }

    let content = format!("use alloy::sol;\n\n{}\n", sol_blocks.join("\n\n"));
    std::fs::write(gen_path, content).expect("Failed to write generated lib.rs");
}
