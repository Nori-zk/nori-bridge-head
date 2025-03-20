use alloy_primitives::FixedBytes;
use serde::{Deserialize, Serialize};

/// Base notice message types
#[derive(Serialize, Deserialize, Clone)]
pub enum NoticeMessageType {
    BridgeHeadStarted,
    BridgeHeadWarning,
    BridgeHeadJobCreated,
    BridgeHeadJobSucceeded,
    BridgeHeadJobFailed,
    BridgeHeadFinalityTransitionDetected,
    BridgeHeadAdvanceRequested,
    BridgeHeadAdvanced,
}

// Message extensions
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeExtensionBridgeHeadStarted {}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeExtensionBridgeHeadWarning {
    pub message: String
}
#[derive(Serialize, Deserialize, Clone)]
pub struct  NoticeExtensionBridgeHeadJobCreated {
    pub input_slot: u64,
    pub expected_output_slot: u64,
    pub job_idx: u64
}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeExtensionBridgeHeadJobSucceeded {
    pub input_slot: u64,
    pub output_slot: u64,
    pub job_idx: u64,
    pub next_sync_committee: FixedBytes<32>,
    pub elapsed_sec: f64
}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeExtensionBridgeHeadJobFailed {
    pub input_slot: u64,
    pub expected_output_slot: u64,
    pub job_idx: u64,
    pub message: String,
    pub elapsed_sec: f64,
    pub n_job_in_buffer: u64,
}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeExtensionBridgeHeadFinalityTransitionDetected {
    pub slot: u64
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeExtensionBridgeHeadAdvanced {
    pub head: u64,
    pub next_sync_committee: FixedBytes<32>
}

#[derive(Serialize, Deserialize, Clone)]
pub enum BridgeHeadNoticeMessageExtension {
    Started(NoticeExtensionBridgeHeadStarted),
    Warning(NoticeExtensionBridgeHeadWarning),
    JobCreated(NoticeExtensionBridgeHeadJobCreated),
    JobSucceeded(NoticeExtensionBridgeHeadJobSucceeded),
    JobFailed(NoticeExtensionBridgeHeadJobFailed),
    FinalityTransitionDetected(NoticeExtensionBridgeHeadFinalityTransitionDetected),
    HeadAdvanced(NoticeExtensionBridgeHeadAdvanced)    
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeMessage {
    pub datetime_iso: String,
    pub message_type: NoticeMessageType,
    pub extension: BridgeHeadNoticeMessageExtension
}

pub fn get_notice_message_type(extension: &BridgeHeadNoticeMessageExtension) -> NoticeMessageType {
    match extension {
        BridgeHeadNoticeMessageExtension::Started(_) => NoticeMessageType::BridgeHeadStarted,
        BridgeHeadNoticeMessageExtension::Warning(_) => NoticeMessageType::BridgeHeadWarning,
        BridgeHeadNoticeMessageExtension::JobCreated(_) => NoticeMessageType::BridgeHeadJobCreated,
        BridgeHeadNoticeMessageExtension::JobSucceeded(_) => NoticeMessageType::BridgeHeadJobSucceeded,
        BridgeHeadNoticeMessageExtension::JobFailed(_) => NoticeMessageType::BridgeHeadJobFailed,
        BridgeHeadNoticeMessageExtension::FinalityTransitionDetected(_) => NoticeMessageType::BridgeHeadFinalityTransitionDetected,
        BridgeHeadNoticeMessageExtension::HeadAdvanced(_) => NoticeMessageType::BridgeHeadAdvanced,
    }
}