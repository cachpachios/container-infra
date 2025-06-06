use bitcode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum GuestExitCode {
    GracefulShutdown, // Requested by the host to shut down gracefully
    FailedToPullContainerImage,
    ContainerExited(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum InitVmState {
    Online,
    PullingContainerImage,
    ExecutingContainer,
}

impl InitVmState {
    pub fn as_str(&self) -> &'static str {
        match self {
            InitVmState::Online => "online",
            InitVmState::PullingContainerImage => "pulling_container_image",
            InitVmState::ExecutingContainer => "executing_container",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum LogMessageType {
    System, // System messages, e.g., startup logs
    Stdout, // Standard output from the container
    Stderr, // Standard error from the container
}

impl LogMessageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogMessageType::System => "system",
            LogMessageType::Stdout => "stdout",
            LogMessageType::Stderr => "stderr",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct LogMessage {
    pub text: String,
    pub timestamp_ms: u64, // Timestamp in milliseconds since unix epoch
    pub message_type: LogMessageType,
}

pub fn get_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl LogMessage {
    pub fn new(text: String, message_type: LogMessageType) -> Self {
        LogMessage {
            text,
            timestamp_ms: get_timestamp_ms(),
            message_type,
        }
    }

    pub fn system(text: String) -> Self {
        Self::new(text, LogMessageType::System)
    }

    pub fn stdout(text: String) -> Self {
        Self::new(text, LogMessageType::Stdout)
    }

    pub fn stderr(text: String) -> Self {
        Self::new(text, LogMessageType::Stderr)
    }
}

// Guest -> Host
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum GuestPacket {
    Log(LogMessage),
    VmState((InitVmState, u64)), // (state, timestamp_ms)
    Exited(GuestExitCode),
}

pub fn serialize_guest_packet(packet: &GuestPacket) -> Vec<u8> {
    bitcode::encode(packet)
}

pub fn deserialize_guest_packet(data: &[u8]) -> Result<GuestPacket, bitcode::Error> {
    bitcode::decode(data)
}
