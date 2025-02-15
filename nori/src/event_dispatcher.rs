use async_trait::async_trait;
use anyhow::Result;

#[async_trait]
pub trait NoriBridgeEventListener<T: Clone>: Send + Sync {
    async fn on_proof(&mut self, proof_job_data: T) -> Result<()>;
}