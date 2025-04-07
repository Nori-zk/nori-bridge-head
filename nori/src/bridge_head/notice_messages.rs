use alloy_primitives::FixedBytes;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

// ================  BASE TYPES ================ //
/// Base message types (enum)
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum NoticeMessageType {
    BridgeHeadStarted,
    BridgeHeadWarning,
    BridgeHeadJobCreated,
    BridgeHeadJobSucceeded,
    BridgeHeadJobFailed,
    BridgeHeadFinalityTransitionDetected,
    BridgeHeadAdvanced,
}

// ================ MACRO FOR MESSAGE TYPES ================ //
#[macro_export]
macro_rules! define_message {
    ($msg_type:ident, $ext_type:ty) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct $msg_type {
            pub datetime_iso: String,
            pub extension: $ext_type,
            pub message_type: NoticeMessageType,
        }

        impl $msg_type {
            pub fn new(datetime_iso: String, extension: $ext_type) -> Self {
                Self {
                    datetime_iso,
                    extension,
                    message_type: NoticeMessageType::$msg_type,
                }
            }
        }
    };
}

// ================ EXTENSION STRUCTS ================ //
// Message extensions
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NoticeExtensionBridgeHeadStarted {
    pub latest_beacon_slot: u64,
    pub current_head: u64,
    pub store_hash: FixedBytes<32>,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NoticeExtensionBridgeHeadWarning {
    pub message: String,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NoticeExtensionBridgeHeadJobCreated {
    pub job_id: u64,
    pub input_slot: u64,
    pub input_store_hash: FixedBytes<32>,
    pub expected_output_slot: u64,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NoticeExtensionBridgeHeadJobSucceeded {
    pub job_id: u64,
    pub input_slot: u64,
    pub input_store_hash: FixedBytes<32>,
    pub output_slot: u64,
    pub output_store_hash: FixedBytes<32>,
    pub next_sync_committee: FixedBytes<32>,
    pub execution_state_root: FixedBytes<32>,
    pub elapsed_sec: f64,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NoticeExtensionBridgeHeadJobFailed {
    pub job_id: u64,
    pub input_slot: u64,
    pub input_store_hash: FixedBytes<32>,
    pub expected_output_slot: u64,
    pub message: String,
    pub elapsed_sec: f64,
    pub n_job_in_buffer: u64,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NoticeExtensionBridgeHeadFinalityTransitionDetected {
    pub slot: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NoticeExtensionBridgeHeadAdvanced {
    pub head: u64,
    pub next_sync_committee: FixedBytes<32>,
    pub store_hash: FixedBytes<32>
}

// ================ GENERATE MESSAGES VIA MACRO ================ //
define_message!(BridgeHeadStarted, NoticeExtensionBridgeHeadStarted);
define_message!(BridgeHeadWarning, NoticeExtensionBridgeHeadWarning);
define_message!(BridgeHeadJobCreated, NoticeExtensionBridgeHeadJobCreated);
define_message!(
    BridgeHeadJobSucceeded,
    NoticeExtensionBridgeHeadJobSucceeded
);
define_message!(BridgeHeadJobFailed, NoticeExtensionBridgeHeadJobFailed);
define_message!(
    BridgeHeadFinalityTransitionDetected,
    NoticeExtensionBridgeHeadFinalityTransitionDetected
);
define_message!(BridgeHeadAdvanced, NoticeExtensionBridgeHeadAdvanced);

#[derive(Clone)]
pub enum BridgeHeadNoticeMessage {
    Started(BridgeHeadStarted),
    Warning(BridgeHeadWarning),
    JobCreated(BridgeHeadJobCreated),
    JobSucceeded(BridgeHeadJobSucceeded),
    JobFailed(BridgeHeadJobFailed),
    FinalityTransitionDetected(BridgeHeadFinalityTransitionDetected),
    HeadAdvanced(BridgeHeadAdvanced),
}

impl Serialize for BridgeHeadNoticeMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            BridgeHeadNoticeMessage::Started(msg) => msg.serialize(serializer),
            BridgeHeadNoticeMessage::Warning(msg) => msg.serialize(serializer),
            BridgeHeadNoticeMessage::JobCreated(msg) => msg.serialize(serializer),
            BridgeHeadNoticeMessage::JobSucceeded(msg) => msg.serialize(serializer),
            BridgeHeadNoticeMessage::JobFailed(msg) => msg.serialize(serializer),
            BridgeHeadNoticeMessage::FinalityTransitionDetected(msg) => msg.serialize(serializer),
            BridgeHeadNoticeMessage::HeadAdvanced(msg) => msg.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for BridgeHeadNoticeMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Helper struct to extract message_type
        #[derive(Deserialize)]
        struct Helper {
            message_type: NoticeMessageType,
            #[serde(flatten)]
            other: Map<String, Value>,
        }

        let mut helper = Helper::deserialize(deserializer)?;
        // Add message_type back into our Map collection.
        helper.other.insert(
            "message_type".to_string(),
            serde_json::to_value(&helper.message_type).map_err(D::Error::custom)?,
        );
        let json_value = Value::Object(helper.other);

        match helper.message_type {
            NoticeMessageType::BridgeHeadStarted => {
                let msg = BridgeHeadStarted::deserialize(json_value).map_err(D::Error::custom)?;
                Ok(BridgeHeadNoticeMessage::Started(msg))
            }
            NoticeMessageType::BridgeHeadWarning => {
                let msg = BridgeHeadWarning::deserialize(json_value).map_err(D::Error::custom)?;
                Ok(BridgeHeadNoticeMessage::Warning(msg))
            }
            NoticeMessageType::BridgeHeadJobCreated => {
                let msg =
                    BridgeHeadJobCreated::deserialize(json_value).map_err(D::Error::custom)?;
                Ok(BridgeHeadNoticeMessage::JobCreated(msg))
            }
            NoticeMessageType::BridgeHeadJobSucceeded => {
                let msg =
                    BridgeHeadJobSucceeded::deserialize(json_value).map_err(D::Error::custom)?;
                Ok(BridgeHeadNoticeMessage::JobSucceeded(msg))
            }
            NoticeMessageType::BridgeHeadJobFailed => {
                let msg = BridgeHeadJobFailed::deserialize(json_value).map_err(D::Error::custom)?;
                Ok(BridgeHeadNoticeMessage::JobFailed(msg))
            }
            NoticeMessageType::BridgeHeadFinalityTransitionDetected => {
                let msg = BridgeHeadFinalityTransitionDetected::deserialize(json_value)
                    .map_err(D::Error::custom)?;
                Ok(BridgeHeadNoticeMessage::FinalityTransitionDetected(msg))
            }
            NoticeMessageType::BridgeHeadAdvanced => {
                let msg = BridgeHeadAdvanced::deserialize(json_value).map_err(D::Error::custom)?;
                Ok(BridgeHeadNoticeMessage::HeadAdvanced(msg))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum BridgeHeadNoticeMessageExtension {
    Started(NoticeExtensionBridgeHeadStarted),
    Warning(NoticeExtensionBridgeHeadWarning),
    JobCreated(NoticeExtensionBridgeHeadJobCreated),
    JobSucceeded(NoticeExtensionBridgeHeadJobSucceeded),
    JobFailed(NoticeExtensionBridgeHeadJobFailed),
    FinalityTransitionDetected(NoticeExtensionBridgeHeadFinalityTransitionDetected),
    HeadAdvanced(NoticeExtensionBridgeHeadAdvanced),
}

impl BridgeHeadNoticeMessageExtension {
    pub fn into_message(self, datetime_iso: String) -> BridgeHeadNoticeMessage {
        match self {
            Self::Started(ext) => {
                BridgeHeadNoticeMessage::Started(BridgeHeadStarted::new(datetime_iso, ext))
            }
            Self::Warning(ext) => {
                BridgeHeadNoticeMessage::Warning(BridgeHeadWarning::new(datetime_iso, ext))
            }
            Self::JobCreated(ext) => {
                BridgeHeadNoticeMessage::JobCreated(BridgeHeadJobCreated::new(datetime_iso, ext))
            }
            Self::JobSucceeded(ext) => BridgeHeadNoticeMessage::JobSucceeded(
                BridgeHeadJobSucceeded::new(datetime_iso, ext),
            ),
            Self::JobFailed(ext) => {
                BridgeHeadNoticeMessage::JobFailed(BridgeHeadJobFailed::new(datetime_iso, ext))
            }
            Self::FinalityTransitionDetected(ext) => {
                BridgeHeadNoticeMessage::FinalityTransitionDetected(
                    BridgeHeadFinalityTransitionDetected::new(datetime_iso, ext),
                )
            }
            Self::HeadAdvanced(ext) => {
                BridgeHeadNoticeMessage::HeadAdvanced(BridgeHeadAdvanced::new(datetime_iso, ext))
            }
        }
    }
}
