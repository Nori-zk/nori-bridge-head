use alloy_primitives::Address;
use anyhow::{Context, Result};
use std::env;

/// Address of the NoriProofRequestQueue, the account every storage proof is
/// anchored on and the address committed to the destination chain.
pub fn get_proof_queue_address() -> Result<Address> {
    let proof_queue_address = env::var("NORI_PROOF_QUEUE_ADDRESS")
        .context("Missing NORI_PROOF_QUEUE_ADDRESS in environment")?
        .parse::<Address>()
        .context("Invalid Ethereum address format")?;
    Ok(proof_queue_address)
}