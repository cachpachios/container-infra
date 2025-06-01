use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
};

use vmproto::guest::{serialize_guest_packet, GuestPacket, LogMessage, LogMessageType};
use vsock::VsockStream;

const BUFFER_SIZE: usize = 1024 * 4;

#[derive(Debug)]
pub enum CommErrors {
    UnableToConnect,
    #[allow(dead_code)]
    IoError(std::io::Error),
}

pub struct HostCommunication {
    stream: VsockStream,
}

impl HostCommunication {
    pub fn new(port: u32) -> Result<Self, CommErrors> {
        let stream: VsockStream = VsockStream::connect_with_cid_port(vsock::VMADDR_CID_HOST, port)
            .map_err(|e| {
                log::error!("Unable to connect to vsock: {}", e);
                CommErrors::UnableToConnect
            })?;
        stream.set_read_timeout(None).map_err(CommErrors::IoError)?;
        stream
            .set_write_timeout(None)
            .map_err(CommErrors::IoError)?;
        Ok(HostCommunication { stream })
    }

    pub fn write(&mut self, packet: GuestPacket) -> Result<(), CommErrors> {
        self.write_without_flush(packet)?;
        self.flush()?;
        Ok(())
    }

    fn write_without_flush(&mut self, packet: GuestPacket) -> Result<(), CommErrors> {
        let data = serialize_guest_packet(&packet);

        self.stream
            .write_all(&(data.len() as u32).to_be_bytes())
            .map_err(CommErrors::IoError)?;
        self.stream.write_all(&data).map_err(CommErrors::IoError)?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), CommErrors> {
        self.stream.flush().map_err(CommErrors::IoError)?;
        Ok(())
    }

    pub fn log_system_message(&mut self, message: String) {
        log::debug!("{}", message);
        self.write(GuestPacket::Log(LogMessage::system(message)))
            .unwrap();
    }
}

pub fn spawn_pipe_to_log(
    comm: Arc<Mutex<HostCommunication>>,
    mut pipe: Box<dyn Read + Send>,
    log_type: LogMessageType,
) -> std::thread::JoinHandle<()> {
    log::debug!("Spawning logger thread");
    std::thread::spawn(move || loop {
        let mut buf = [0; BUFFER_SIZE + 1024];
        let mut pos = 0;
        loop {
            pos = pos.min(BUFFER_SIZE);
            let buf_slice = &mut buf[pos..];
            match pipe.read(buf_slice) {
                Ok(0) => {
                    log::debug!("Pipe closed. No more data to read.");
                    break;
                }
                Ok(n) => {
                    let newline_index = buf_slice[..n].iter().position(|&b| b == b'\n');
                    if let Some(newline_index) = newline_index {
                        let index = pos + newline_index;
                        let line = String::from_utf8_lossy(&buf[..index]);
                        comm.lock()
                            .unwrap()
                            .write(GuestPacket::Log(LogMessage::new(
                                line.to_string(),
                                log_type,
                            )))
                            .unwrap();

                        let next_line_buffered = n - newline_index - 1;
                        if next_line_buffered > 0 {
                            buf.copy_within(index + 1..index + next_line_buffered, 0);
                            pos = next_line_buffered;
                        } else {
                            pos = 0;
                        }
                    } else {
                        pos += n;
                        if pos >= BUFFER_SIZE - 1024 {
                            const OVERFLOW: &[u8] = b"???...???";
                            pos = BUFFER_SIZE - 1024;
                            buf[pos..pos + OVERFLOW.len()].copy_from_slice(OVERFLOW);
                            pos += OVERFLOW.len();
                        }
                    }
                }
                Err(e) => {
                    log::error!("Error reading from pipe. Error: {}", e);
                    break;
                }
            }
        }
    })
}
