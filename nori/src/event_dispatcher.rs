use async_trait::async_trait;
use anyhow::Result;

#[async_trait]
pub trait NoriBridgeEventListener<T: Clone, Q: Clone>: Send + Sync {
    async fn on_proof(&mut self, proof_job_data: T) -> Result<()>;
    async fn on_notice(&mut self, notice_data: Q) -> Result<()>;
}