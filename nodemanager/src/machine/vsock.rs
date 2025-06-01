use std::sync::Arc;

use circular_buffer::CircularBuffer;
use tokio::{
    io::AsyncReadExt,
    net::UnixStream,
    sync::{
        mpsc::{Receiver, Sender},
        Mutex,
    },
};

const MAX_LINES_IN_BUFFER: usize = 256;

pub struct MachineCommunicator {
    log_subscribers: Vec<Sender<Arc<str>>>,
    log_buffer: CircularBuffer<MAX_LINES_IN_BUFFER, String>,
}

impl MachineCommunicator {
    pub async fn spawn(
        stream: UnixStream,
        stop_handler: tokio::sync::oneshot::Sender<()>,
    ) -> (Arc<Mutex<MachineCommunicator>>, tokio::task::JoinHandle<()>) {
        let handler = Arc::new(Mutex::new(MachineCommunicator {
            log_subscribers: Vec::new(),
            log_buffer: CircularBuffer::new(),
        }));

        let jh = tokio::spawn(packet_handler(stream, handler.clone(), stop_handler));

        (handler, jh)
    }

    async fn handle_packet(&mut self, packet: vmproto::guest::GuestPacket) {
        match packet {
            vmproto::guest::GuestPacket::Log(log) => {
                log::trace!("Received log packet: {:?}", log);
                self.push_log(&log.text).await;
            }
            vmproto::guest::GuestPacket::Exited(exit_code) => {
                //TODO!!!
                log::info!("Machine exited with code: {:?}", exit_code);
            }
        }
    }

    async fn push_log(&mut self, data: &str) {
        self.log_buffer.push_front(data.to_string());
        if self.log_subscribers.is_empty() {
            return;
        }

        let data_arc: Arc<str> = Arc::from(data);

        let mut to_drop = Vec::new();

        for (i, tx) in self.log_subscribers.iter().enumerate() {
            if let Err(_) = tx.try_send(data_arc.clone()) {
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

    pub fn subscribe_log(&mut self) -> Receiver<Arc<str>> {
        let (tx, rx) = tokio::sync::mpsc::channel(128);
        self.log_subscribers.push(tx);
        rx
    }

    pub fn clone_buffer(&self) -> Vec<String> {
        self.log_buffer.iter().cloned().collect()
    }

    fn drop_subscribers(&mut self) {
        self.log_subscribers.clear();
    }
}

async fn packet_handler(
    mut stream: UnixStream,
    handler: Arc<Mutex<MachineCommunicator>>,
    stop_handler: tokio::sync::oneshot::Sender<()>,
) {
    loop {
        match read_from_stream(&mut stream).await {
            Ok(packet) => {
                log::trace!("Received packet: {:?}", packet);
                let mut handler = handler.lock().await;
                handler.handle_packet(packet).await;
            }
            Err(_) => {
                log::error!("Error reading from stream, closing connection");
                break;
            }
        }
    }
    handler.lock().await.drop_subscribers();
    let _ = stop_handler.send(());
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
            log::error!("Error reading length from vsock: {}", e);
            return Err(());
        }
    }
    log::trace!("Reading {} bytes from vsock", len);

    let mut buf: Vec<u8> = vec![0; len];
    match stream.read_exact(&mut buf).await {
        Ok(0) => {
            log::error!("Vsock connection closed");
            return Err(());
        }
        Ok(_) => {
            let msg = vmproto::guest::deserialize_guest_packet(&buf[..]);
            match msg {
                Ok(packet) => return Ok(packet),
                Err(e) => {
                    log::error!("Error deserializing packet: {}", e);
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
