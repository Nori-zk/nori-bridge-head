use alloy_primitives::FixedBytes;
use serde::{Deserialize, Serialize};

/// Base notice message types
#[derive(Serialize, Deserialize, Clone)]
pub enum NoticeMessageType {
    Started,
    Warning,
    JobCreated,
    JobSucceeded,
    JobFailed,
    FinalityTransitionDetected,
    AdvanceRequested,
    HeadAdvanced,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeBaseMessage {
    pub timestamp: String,
    pub message_type: NoticeMessageType,
    pub current_head: u64,
    pub next_head: u64,
    pub working_head: u64,
    pub last_beacon_finality_head_checked: u64,
    pub last_job_duration_seconds: f64,
    pub time_until_next_finality_transition_seconds: f64
}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeStarted {}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeWarning {
    pub message: String
}
#[derive(Serialize, Deserialize, Clone)]
pub struct  NoticeJobCreated {
    pub slot: u64,
    pub job_idx: u64
}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeJobSucceeded {
    pub slot: u64,
    pub job_idx: u64,
    pub next_sync_committee: FixedBytes<32>
}
#[derive(Serialize, Deserialize, Clone)]
pub struct  NoticeJobFailed {
    pub slot: u64,
    pub job_idx: u64,
    pub message: String
}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeFinalityTransitionDetected {
    pub slot: u64
}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeAdvanceRequested {

}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeHeadAdvanced {
    pub slot: u64,
    pub next_sync_committee: FixedBytes<32>
}

#[derive(Serialize, Deserialize, Clone)]
pub enum NoticeMessageExtension {
    Started(NoticeStarted),
    Warning(NoticeWarning),
    JobCreated(NoticeJobCreated),
    JobSucceeded(NoticeJobSucceeded),
    JobFailed(NoticeJobFailed),
    FinalityTransitionDetected(NoticeFinalityTransitionDetected),
    AdvanceRequested(NoticeAdvanceRequested),
    HeadAdvanced(NoticeHeadAdvanced)    
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeMessage {
    pub base: NoticeBaseMessage,
    pub extension: NoticeMessageExtension
}

pub fn get_notice_message_type(extension: &NoticeMessageExtension) -> NoticeMessageType {
    match extension {
        NoticeMessageExtension::Started(_) => NoticeMessageType::Started,
        NoticeMessageExtension::Warning(_) => NoticeMessageType::Warning,
        NoticeMessageExtension::JobCreated(_) => NoticeMessageType::JobCreated,
        NoticeMessageExtension::JobSucceeded(_) => NoticeMessageType::JobSucceeded,
        NoticeMessageExtension::JobFailed(_) => NoticeMessageType::JobFailed,
        NoticeMessageExtension::FinalityTransitionDetected(_) => NoticeMessageType::FinalityTransitionDetected,
        NoticeMessageExtension::AdvanceRequested(_) => NoticeMessageType::AdvanceRequested,
        NoticeMessageExtension::HeadAdvanced(_) => NoticeMessageType::HeadAdvanced,
    }
}