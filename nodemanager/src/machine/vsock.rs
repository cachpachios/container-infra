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
use vmproto::guest::LogMessage;

const MAX_LINES_IN_BUFFER: usize = 256;

pub struct MachineCommunicator {
    stream: OwnedWriteHalf,
    log_subscribers: Vec<Sender<Arc<LogMessage>>>,
    log_buffer: CircularBuffer<MAX_LINES_IN_BUFFER, Arc<LogMessage>>,
}

impl MachineCommunicator {
    pub async fn spawn(
        stream: UnixStream,
        stop_handler: tokio::sync::oneshot::Sender<()>,
    ) -> (Arc<Mutex<MachineCommunicator>>, tokio::task::JoinHandle<()>) {
        let (read, write) = stream.into_split();

        let handler = Arc::new(Mutex::new(MachineCommunicator {
            log_subscribers: Vec::new(),
            log_buffer: CircularBuffer::new(),
            stream: write,
        }));

        let jh = tokio::spawn(packet_handler(read, handler.clone(), stop_handler));

        (handler, jh)
    }

    async fn handle_packet(&mut self, packet: vmproto::guest::GuestPacket) {
        match packet {
            vmproto::guest::GuestPacket::Log(log) => {
                log::trace!("Received log packet: {:?}", log);
                self.push_log(log).await;
            }
            vmproto::guest::GuestPacket::Exited(exit_code) => {
                //TODO!!!
                log::info!("Machine exited with code: {:?}", exit_code);
            }
        }
    }

    async fn push_log(&mut self, data: LogMessage) {
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

    pub fn subscribe_log(&mut self) -> Receiver<Arc<LogMessage>> {
        let (tx, rx) = tokio::sync::mpsc::channel(128);
        self.log_subscribers.push(tx);
        rx
    }

    pub fn clone_buffer(&self) -> Vec<Arc<LogMessage>> {
        self.log_buffer.iter().rev().cloned().collect()
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
    stop_handler: tokio::sync::oneshot::Sender<()>,
) {
    loop {
        match read_from_stream(&mut stream).await {
            Ok(packet) => {
                log::trace!("Received packet: {:?}", packet);
                let mut handler = handler.lock().await;
                handler.handle_packet(packet).await;
                log::trace!("Packet handled");
            }
            Err(_) => {
                break;
            }
        }
    }
    log::trace!("Packet handler loop exited, shutting down");
    let _ = handler.try_lock().map(|mut h| h.drop_subscribers());
    let _ = stop_handler.send(());
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
