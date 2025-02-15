use alloy_primitives::FixedBytes;
use serde::{Deserialize, Serialize};

/// Base notice message types
#[derive(Serialize, Deserialize)]
pub enum NoriNoticeMessageType {
    Started,
    Warning,
    JobCreated,
    JobSucceeded,
    JobFailed,
    FinalityTransitionDetected,
    AdvanceRequested,
    HeadAdvanced,
}

#[derive(Serialize, Deserialize)]
pub struct NoriBridgeHeadNoticeBaseMessage {
    pub timestamp: String,
    pub message_type: NoriNoticeMessageType,
    pub current_head: u64,
    pub next_head: u64,
    pub working_head: u64,
    pub last_beacon_finality_head_checked: u64,
    pub last_job_duration_seconds: f64,
    pub time_until_next_finality_transition_seconds: f64
}
#[derive(Serialize, Deserialize)]
pub struct NoriBridgeHeadNoticeStarted {}
#[derive(Serialize, Deserialize)]
pub struct NoriBridgeHeadNoticeWarning {
    pub message: String
}
#[derive(Serialize, Deserialize)]
pub struct  NoriBridgeHeadNoticeJobCreated {
    pub slot: u64,
    pub job_idx: u64
}
#[derive(Serialize, Deserialize)]
pub struct NoriBridgeHeadNoticeJobSucceeded {
    pub slot: u64,
    pub job_idx: u64,
    pub next_sync_committee: FixedBytes<32>
}
#[derive(Serialize, Deserialize)]
pub struct  NoriBridgeHeadNoticeJobFailed {
    pub slot: u64,
    pub job_idx: u64,
    pub message: String
}
#[derive(Serialize, Deserialize)]
pub struct NoriBridgeHeadNoticeFinalityTransitionDetected {
    pub slot: u64
}
#[derive(Serialize, Deserialize)]
pub struct NoriBridgeHeadNoticeAdvanceRequested {

}
#[derive(Serialize, Deserialize)]
pub struct NoriBridgeHeadNoticeHeadAdvanced {
    pub slot: u64,
    pub next_sync_committee: FixedBytes<32>
}

#[derive(Serialize, Deserialize)]
pub enum NoriBridgeHeadMessageExtension {
    NoriBridgeHeadNoticeStarted(NoriBridgeHeadNoticeStarted),
    NoriBridgeHeadNoticeWarning(NoriBridgeHeadNoticeWarning),
    NoriBridgeHeadNoticeJobCreated(NoriBridgeHeadNoticeJobCreated),
    NoriBridgeHeadNoticeJobSucceeded(NoriBridgeHeadNoticeJobSucceeded),
    NoriBridgeHeadNoticeJobFailed(NoriBridgeHeadNoticeJobFailed),
    NoriBridgeHeadNoticeFinalityTransitionDetected(NoriBridgeHeadNoticeFinalityTransitionDetected),
    NoriBridgeHeadNoticeAdvanceRequested(NoriBridgeHeadNoticeAdvanceRequested),
    NoriBridgeHeadNoticeHeadAdvanced(NoriBridgeHeadNoticeHeadAdvanced)    
}

#[derive(Serialize, Deserialize)]
pub struct NoriBridgeHeadMessage {
    pub base: NoriBridgeHeadNoticeBaseMessage,
    pub extension: NoriBridgeHeadMessageExtension
}