use std::sync::Arc;

use circular_buffer::CircularBuffer;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        unix::{OwnedReadHalf, OwnedWriteHalf},
        UnixStream,
    },
    sync::{
        mpsc::{Receiver, Sender},
        Mutex,
    },
};
use vmproto::guest::{GuestExitCode, InitVmState, LogMessage};

const MAX_LINES_IN_BUFFER: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineExit {
    Unknown,
    ContainerExited(i32),
    GracefulShutdown,
    FailedToPullContainerImage,
}

pub enum MachineLog {
    VmLog(LogMessage),
    State(InitVmState, u64),
}

impl MachineLog {
    pub fn as_proto_log_message(&self) -> proto::node::LogMessage {
        match self {
            MachineLog::VmLog(s) => proto::node::LogMessage {
                message: Some(s.text.clone()),
                timestamp_ms: s.timestamp_ms as i64,
                log_type: s.message_type.as_str().to_string(),
                state: None,
            },
            MachineLog::State(s, timestamp_ms) => proto::node::LogMessage {
                timestamp_ms: *timestamp_ms as i64,
                log_type: "state".to_string(),
                message: None,
                state: Some(s.as_str().to_string()),
            },
        }
    }
}

impl From<GuestExitCode> for MachineExit {
    fn from(code: GuestExitCode) -> Self {
        match code {
            GuestExitCode::GracefulShutdown => MachineExit::GracefulShutdown,
            GuestExitCode::FailedToPullContainerImage => MachineExit::FailedToPullContainerImage,
            GuestExitCode::ContainerExited(code) => MachineExit::ContainerExited(code),
        }
    }
}

pub struct MachineCommunicator {
    stream: OwnedWriteHalf,
    log_subscribers: Vec<Sender<Arc<MachineLog>>>,
    log_buffer: CircularBuffer<MAX_LINES_IN_BUFFER, Arc<MachineLog>>,
    state: Option<(InitVmState, u64)>,
}

impl MachineCommunicator {
    pub async fn spawn(
        stream: UnixStream,
        stop_handler: tokio::sync::oneshot::Sender<MachineExit>,
    ) -> (Arc<Mutex<MachineCommunicator>>, tokio::task::JoinHandle<()>) {
        let (read, write) = stream.into_split();

        let handler = Arc::new(Mutex::new(MachineCommunicator {
            log_subscribers: Vec::new(),
            log_buffer: CircularBuffer::new(),
            stream: write,
            state: None,
        }));

        let jh = tokio::spawn(packet_handler(read, handler.clone(), stop_handler));

        (handler, jh)
    }

    async fn push_log(&mut self, data: MachineLog) {
        let data = Arc::from(data);
        self.log_buffer.push_front(data.clone());
        if self.log_subscribers.is_empty() {
            return;
        }

        let mut to_drop = Vec::new();

        for (i, tx) in self.log_subscribers.iter().enumerate() {
            if let Err(_) = tx.try_send(data.clone()) {
                to_drop.push(i);
            }
        }

        if !to_drop.is_empty() {
            let mut i = 0;
            self.log_subscribers.retain(|_| {
                let r = to_drop.contains(&i);
                i += 1;
                !r
            });
        }
    }

    pub fn subscribe_log(&mut self) -> Receiver<Arc<MachineLog>> {
        let (tx, rx) = tokio::sync::mpsc::channel(128);
        self.log_subscribers.push(tx);
        rx
    }

    pub fn clone_buffer_with_state(&self) -> Vec<Arc<MachineLog>> {
        let should_chain_last_state = self
            .state
            .map(|(s, t)| {
                self.log_buffer
                    .iter()
                    .find(|log| match &***log {
                        MachineLog::State(state, timestamp) => *state == s && *timestamp == t,
                        _ => false,
                    })
                    .is_some()
            })
            .unwrap_or(false);
        self.log_buffer
            .iter()
            .rev()
            .cloned()
            .chain(
                self.state
                    .as_ref()
                    .and_then(|(s, ts)| {
                        if should_chain_last_state {
                            None
                        } else {
                            Some(vec![Arc::new(MachineLog::State(*s, *ts))])
                        }
                    })
                    .into_iter()
                    .flatten(),
            )
            .collect()
    }

    fn drop_subscribers(&mut self) {
        self.log_subscribers.clear();
    }

    pub async fn write(&mut self, packet: vmproto::host::HostPacket) -> Result<(), std::io::Error> {
        let data = vmproto::host::serialize_host_packet(&packet);
        let len = (data.len() as u32).to_be_bytes();
        self.stream.write_all(&len).await?;
        self.stream.write_all(&data).await?;
        self.stream.flush().await?;
        Ok(())
    }

    pub async fn send_shutdown(&mut self) -> Result<(), std::io::Error> {
        self.write(vmproto::host::HostPacket::Shutdown).await
    }
}

async fn packet_handler(
    mut stream: OwnedReadHalf,
    handler: Arc<Mutex<MachineCommunicator>>,
    stop_handler: tokio::sync::oneshot::Sender<MachineExit>,
) {
    let mut exit = MachineExit::Unknown;
    loop {
        match read_from_stream(&mut stream).await {
            Ok(packet) => {
                log::trace!("Received packet: {:?}", packet);
                let mut handler = handler.lock().await;
                match packet {
                    vmproto::guest::GuestPacket::Log(log) => {
                        log::trace!("Received log packet: {:?}", log);
                        handler.push_log(MachineLog::VmLog(log)).await;
                    }
                    vmproto::guest::GuestPacket::Exited(exit_code) => {
                        log::info!("Machine exited with code: {:?}", exit_code);
                        exit = MachineExit::from(exit_code);
                        handler.state = None;
                    }
                    vmproto::guest::GuestPacket::VmState((state, timestamp_ms)) => {
                        log::trace!("Received VM state packet: {:?}", state);
                        handler.state = Some((state, timestamp_ms));
                        handler
                            .push_log(MachineLog::State(state, timestamp_ms))
                            .await;
                    }
                }
                log::trace!("Packet handled");
            }
            Err(_) => {
                break;
            }
        }
    }
    log::trace!("Packet handler loop exited, shutting down");
    let _ = handler.try_lock().map(|mut h| h.drop_subscribers());
    let _ = stop_handler.send(exit);
    log::trace!("Packet handler stopped");
}

async fn read_from_stream(
    stream: &mut (dyn tokio::io::AsyncRead + Unpin + Send),
) -> Result<vmproto::guest::GuestPacket, ()> {
    let mut len_buf = [0; 4];
    let len;
    match stream.read_exact(&mut len_buf).await {
        Ok(_) => {
            len = u32::from_be_bytes(len_buf) as usize;
        }
        Err(e) => {
            log::debug!("Error reading length from vsock: {}", e);
            return Err(());
        }
    }
    log::trace!("Reading {} bytes from vsock", len);

    if len == 0 || len > vmproto::MAX_PACKET_SIZE {
        log::debug!("Invalid packet length: {}", len);
        return Err(());
    }

    let mut buf: Vec<u8> = vec![0; len];
    match stream.read_exact(&mut buf).await {
        Ok(0) => {
            log::debug!("Vsock connection closed");
            return Err(());
        }
        Ok(_) => {
            let msg = vmproto::guest::deserialize_guest_packet(&buf[..]);
            match msg {
                Ok(packet) => return Ok(packet),
                Err(e) => {
                    log::debug!("Error deserializing packet: {}", e);
                    return Err(());
                }
            }
        }
        Err(e) => {
            log::error!("Error reading from vsock: {}", e);
            return Err(());
        }
    }
}
