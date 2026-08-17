# Deprecated, pending removal

`nori-contract-bindings` (generated `NoriTokenBridge`/`TokensLocked`) is no longer read directly; its only consumer is the deprecated `code_challenge_to_storage_slots`. It is generated from the ABI so it cannot carry `#[deprecated]`. Remove the crate, its `nori/Cargo.toml` dependency, and the `abi`/`build.rs` when that function goes.
