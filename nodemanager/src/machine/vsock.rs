use tokio::io::AsyncReadExt;

pub async fn read_from_stream(
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
