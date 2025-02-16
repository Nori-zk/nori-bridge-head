use async_trait::async_trait;

#[async_trait]
pub trait BeaconFinalityChangeEventObserver {
    async fn on_beacon_change(&mut self, slot: u64) {}
}