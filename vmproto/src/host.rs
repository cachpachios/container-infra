use bitcode::{Decode, Encode};
// Host -> Guest
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum HostPacket {
    Shutdown,
}

pub fn serialize_host_packet(packet: &HostPacket) -> Vec<u8> {
    bitcode::encode(packet)
}

pub fn deserialize_host_packet(data: &[u8]) -> Result<HostPacket, bitcode::Error> {
    bitcode::decode(data)
}
