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

// Base message type
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeBaseMessage {
    pub timestamp: String,
    //pub current_head: u64,
    //pub next_slot: u64,
    //pub time_until_next_finality_transition_seconds: f64
}

// Message extensions
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeStarted {}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeWarning {
    pub message: String
}
#[derive(Serialize, Deserialize, Clone)]
pub struct  NoticeJobCreated {
    pub input_slot: u64,
    pub expected_output_slot: u64,
    pub job_idx: u64
}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeJobSucceeded {
    pub input_slot: u64,
    pub output_slot: u64,
    pub job_idx: u64,
    pub next_sync_committee: FixedBytes<32>,
    pub elapsed_sec: f64
}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeJobFailed {
    pub input_slot: u64,
    pub expected_output_slot: u64,
    pub job_idx: u64,
    pub message: String,
    pub elapsed_sec: f64,
    pub n_job_in_buffer: u64,
}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeFinalityTransitionDetected {
    pub slot: u64
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeHeadAdvanced {
    pub head: u64,
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
    HeadAdvanced(NoticeHeadAdvanced)    
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NoticeMessage {
    pub base: NoticeBaseMessage,
    pub message_type: NoticeMessageType,
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
        NoticeMessageExtension::HeadAdvanced(_) => NoticeMessageType::HeadAdvanced,
    }
}