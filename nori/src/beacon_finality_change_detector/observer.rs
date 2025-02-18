use async_trait::async_trait;
use anyhow::{Ok, Result};
use crate::bridge_head::handles::NoriBridgeHeadBeaconFinalityChangeHandle;

#[async_trait]
pub trait BeaconFinalityChangeEventObserver: Send + Sync {
    async fn on_beacon_change(&mut self, slot: u64) -> Result<()>;
}
pub struct BeaconFinalityChangeEmitter {
    bridge_head: NoriBridgeHeadBeaconFinalityChangeHandle,
}

impl BeaconFinalityChangeEmitter {
    pub fn new(bridge_head: NoriBridgeHeadBeaconFinalityChangeHandle) -> Self {
        Self { bridge_head }
    }
}

#[async_trait]
impl BeaconFinalityChangeEventObserver for BeaconFinalityChangeEmitter {
    async fn on_beacon_change(&mut self, slot: u64) -> Result<()> {
        let _ = self.bridge_head.on_beacon_change(slot).await;
        Ok(())
    }
}