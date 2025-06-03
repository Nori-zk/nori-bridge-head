use alloy_primitives::FixedBytes;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use helios_consensus_core::types::FinalityUpdate;
use nori_sp1_helios_primitives::types::{ProofInputs, VerifiedContractStorageSlot};
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

// ================  BASE TYPES ================ //
/// Base message types (enum)
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum TransitionNoticeMessageType {
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
            pub message_type: TransitionNoticeMessageType,
        }

        impl $msg_type {
            pub fn new(datetime_iso: String, extension: $ext_type) -> Self {
                Self {
                    datetime_iso,
                    extension,
                    message_type: TransitionNoticeMessageType::$msg_type,
                }
            }
        }
    };
}

// ================ EXTENSIONS ================== //
// Message extensions
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransitionNoticeExtensionBridgeHeadStarted {
    pub latest_beacon_slot: u64,
    pub current_slot: u64,
    pub store_hash: FixedBytes<32>,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransitionNoticeExtensionBridgeHeadWarning {
    pub message: String,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransitionNoticeExtensionBridgeHeadJobCreated {
    pub job_id: u64,
    pub input_slot: u64,
    pub input_store_hash: FixedBytes<32>,
    pub expected_output_slot: u64,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransitionNoticeExtensionBridgeHeadJobSucceeded {
    pub job_id: u64,
    pub input_slot: u64,
    pub input_store_hash: FixedBytes<32>,
    pub output_slot: u64,
    pub output_store_hash: FixedBytes<32>,
    pub execution_state_root: FixedBytes<32>,
    pub contract_storage_slots: Vec<VerifiedContractStorageSlot>,
    pub elapsed_sec: f64,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransitionNoticeExtensionBridgeHeadJobFailed {
    pub job_id: u64,
    pub input_slot: u64,
    pub input_store_hash: FixedBytes<32>,
    pub expected_output_slot: u64,
    pub message: String,
    pub elapsed_sec: f64,
    pub n_job_in_buffer: u64,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransitionNoticeNoticeExtensionBridgeHeadFinalityTransitionDetected {
    pub slot: u64,
    #[serde(skip_serializing)]
    pub input_slot: u64,
    #[serde(skip_serializing)]
    pub proof_inputs: Box<ProofInputs<MainnetConsensusSpec>>,
    //#[serde(skip_serializing)]
    //pub finality_update: FinalityUpdate<MainnetConsensusSpec>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransitionNoticeExtensionBridgeHeadAdvanced {
    pub slot: u64,
    pub store_hash: FixedBytes<32>
}

#[derive(Serialize, Deserialize, Clone)]
pub enum TransitionNoticeBridgeHeadMessageExtension {
    Started(TransitionNoticeExtensionBridgeHeadStarted),
    Warning(TransitionNoticeExtensionBridgeHeadWarning),
    JobCreated(TransitionNoticeExtensionBridgeHeadJobCreated),
    JobSucceeded(TransitionNoticeExtensionBridgeHeadJobSucceeded),
    JobFailed(TransitionNoticeExtensionBridgeHeadJobFailed),
    FinalityTransitionDetected(TransitionNoticeNoticeExtensionBridgeHeadFinalityTransitionDetected),
    HeadAdvanced(TransitionNoticeExtensionBridgeHeadAdvanced),
}

// ================ GENERATE MESSAGES VIA MACRO ================ //

define_message!(BridgeHeadStarted, TransitionNoticeExtensionBridgeHeadStarted);
define_message!(BridgeHeadWarning, TransitionNoticeExtensionBridgeHeadWarning);
define_message!(BridgeHeadJobCreated, TransitionNoticeExtensionBridgeHeadJobCreated);
define_message!(
    BridgeHeadJobSucceeded,
    TransitionNoticeExtensionBridgeHeadJobSucceeded
);
define_message!(BridgeHeadJobFailed, TransitionNoticeExtensionBridgeHeadJobFailed);
define_message!(
    BridgeHeadFinalityTransitionDetected,
    TransitionNoticeNoticeExtensionBridgeHeadFinalityTransitionDetected
);
define_message!(BridgeHeadAdvanced, TransitionNoticeExtensionBridgeHeadAdvanced);

// =============== NOTICE TYPE ENUMS ============================= //
#[derive(Clone)]
pub enum TransitionNoticeBridgeHeadMessage {
    Started(BridgeHeadStarted),
    Warning(BridgeHeadWarning),
    JobCreated(BridgeHeadJobCreated),
    JobSucceeded(BridgeHeadJobSucceeded),
    JobFailed(BridgeHeadJobFailed),
    FinalityTransitionDetected(BridgeHeadFinalityTransitionDetected),
    HeadAdvanced(BridgeHeadAdvanced),
}

// =============== MARSHALLING =================================== //

impl Serialize for TransitionNoticeBridgeHeadMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            TransitionNoticeBridgeHeadMessage::Started(msg) => msg.serialize(serializer),
            TransitionNoticeBridgeHeadMessage::Warning(msg) => msg.serialize(serializer),
            TransitionNoticeBridgeHeadMessage::JobCreated(msg) => msg.serialize(serializer),
            TransitionNoticeBridgeHeadMessage::JobSucceeded(msg) => msg.serialize(serializer),
            TransitionNoticeBridgeHeadMessage::JobFailed(msg) => msg.serialize(serializer),
            TransitionNoticeBridgeHeadMessage::FinalityTransitionDetected(msg) => msg.serialize(serializer),
            TransitionNoticeBridgeHeadMessage::HeadAdvanced(msg) => msg.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for TransitionNoticeBridgeHeadMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Helper struct to extract message_type
        #[derive(Deserialize)]
        struct Helper {
            message_type: TransitionNoticeMessageType,
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
            TransitionNoticeMessageType::BridgeHeadStarted => {
                let msg = BridgeHeadStarted::deserialize(json_value).map_err(D::Error::custom)?;
                Ok(TransitionNoticeBridgeHeadMessage::Started(msg))
            }
            TransitionNoticeMessageType::BridgeHeadWarning => {
                let msg = BridgeHeadWarning::deserialize(json_value).map_err(D::Error::custom)?;
                Ok(TransitionNoticeBridgeHeadMessage::Warning(msg))
            }
            TransitionNoticeMessageType::BridgeHeadJobCreated => {
                let msg =
                    BridgeHeadJobCreated::deserialize(json_value).map_err(D::Error::custom)?;
                Ok(TransitionNoticeBridgeHeadMessage::JobCreated(msg))
            }
            TransitionNoticeMessageType::BridgeHeadJobSucceeded => {
                let msg =
                    BridgeHeadJobSucceeded::deserialize(json_value).map_err(D::Error::custom)?;
                Ok(TransitionNoticeBridgeHeadMessage::JobSucceeded(msg))
            }
            TransitionNoticeMessageType::BridgeHeadJobFailed => {
                let msg = BridgeHeadJobFailed::deserialize(json_value).map_err(D::Error::custom)?;
                Ok(TransitionNoticeBridgeHeadMessage::JobFailed(msg))
            }
            TransitionNoticeMessageType::BridgeHeadFinalityTransitionDetected => {
                let msg = BridgeHeadFinalityTransitionDetected::deserialize(json_value)
                    .map_err(D::Error::custom)?;
                Ok(TransitionNoticeBridgeHeadMessage::FinalityTransitionDetected(msg))
            }
            TransitionNoticeMessageType::BridgeHeadAdvanced => {
                let msg = BridgeHeadAdvanced::deserialize(json_value).map_err(D::Error::custom)?;
                Ok(TransitionNoticeBridgeHeadMessage::HeadAdvanced(msg))
            }
        }
    }
}

impl TransitionNoticeBridgeHeadMessageExtension {
    pub fn into_message(self, datetime_iso: String) -> TransitionNoticeBridgeHeadMessage {
        match self {
            Self::Started(ext) => {
                TransitionNoticeBridgeHeadMessage::Started(BridgeHeadStarted::new(datetime_iso, ext))
            }
            Self::Warning(ext) => {
                TransitionNoticeBridgeHeadMessage::Warning(BridgeHeadWarning::new(datetime_iso, ext))
            }
            Self::JobCreated(ext) => {
                TransitionNoticeBridgeHeadMessage::JobCreated(BridgeHeadJobCreated::new(datetime_iso, ext))
            }
            Self::JobSucceeded(ext) => TransitionNoticeBridgeHeadMessage::JobSucceeded(
                BridgeHeadJobSucceeded::new(datetime_iso, ext),
            ),
            Self::JobFailed(ext) => {
                TransitionNoticeBridgeHeadMessage::JobFailed(BridgeHeadJobFailed::new(datetime_iso, ext))
            }
            Self::FinalityTransitionDetected(ext) => {
                TransitionNoticeBridgeHeadMessage::FinalityTransitionDetected(
                    BridgeHeadFinalityTransitionDetected::new(datetime_iso, ext),
                )
            }
            Self::HeadAdvanced(ext) => {
                TransitionNoticeBridgeHeadMessage::HeadAdvanced(BridgeHeadAdvanced::new(datetime_iso, ext))
            }
        }
    }
}
