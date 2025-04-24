use core::str;
use futures_lite::io::AsyncReadExt;
use std::sync::Arc;
use tokio::sync::{mpsc::Sender, Mutex};

use async_process::ChildStdout;

const MAX_LINES: usize = 1000;
const MAX_BYTES: usize = 1024 * 1024;
const MAX_LINE_LENGTH: usize = 1024 * 4;

struct LogBuf {
    buf: Vec<Option<String>>,
    size: usize,
}

impl LogBuf {
    fn new() -> Self {
        LogBuf {
            buf: Vec::with_capacity(MAX_LINES + 1),
            size: 0,
        }
    }

    fn push(&mut self, line: String) {
        self.size += line.len();
        self.buf.push(Some(line));

        let mut dropped = 0;
        if self.size > MAX_BYTES {
            for i in 0..self.buf.len() {
                self.size -= self.buf[i].take().map_or(0, |s| s.len());
                dropped += 1;
                if self.size <= MAX_BYTES {
                    break;
                }
            }
        }

        if self.buf.len() > MAX_LINES {
            for i in 0..self.buf.len() {
                dropped += 1;
                if let Some(s) = self.buf[i].take() {
                    self.size -= s.len();
                    break;
                }
            }
        }

        if dropped > 10 {
            self.buf.retain(|s| s.is_some());
        }
    }
}

pub struct LogHandler {
    log: LogBuf,
    subscribers: Vec<Sender<Arc<str>>>,
}

impl LogHandler {
    pub async fn spawn(out: ChildStdout) -> (Arc<Mutex<LogHandler>>, tokio::task::JoinHandle<()>) {
        let handler = Arc::new(Mutex::new(LogHandler {
            log: LogBuf::new(),
            subscribers: Vec::new(),
        }));

        let handler_clone = handler.clone();
        let jh = tokio::spawn(async move {
            stdout_handler(out, handler_clone).await;
        });

        (handler, jh)
    }

    async fn push(&mut self, data: &str) {
        self.log.push(data.to_string());
        if self.subscribers.is_empty() {
            return;
        }

        let data_arc: Arc<str> = Arc::from(data);

        let mut to_drop = Vec::new();

        for (i, tx) in self.subscribers.iter().enumerate() {
            if let Err(_) = tx.send(data_arc.clone()).await {
                to_drop.push(i);
            }
        }

        if !to_drop.is_empty() {
            let mut i = 0;
            self.subscribers.retain(|_| {
                let r = to_drop.contains(&i);
                i += 1;
                !r
            });
        }
    }

    pub fn subscribe(&mut self, tx: Sender<Arc<str>>) {
        self.subscribers.push(tx);
    }

    pub fn peak_buffer(&self) -> Vec<&String> {
        self.log.buf.iter().filter_map(|s| s.as_ref()).collect()
    }
}

async fn stdout_handler(mut out: ChildStdout, handler: Arc<Mutex<LogHandler>>) {
    let mut buf = [0; MAX_LINE_LENGTH];
    let mut pos = 0;
    loop {
        pos = pos.min(MAX_LINE_LENGTH);
        let buf_slice = &mut buf[pos..];
        match out.read(buf_slice).await {
            Ok(0) => {
                log::debug!("Firecracker process exited");
                break;
            }
            Ok(n) => {
                let newline_index = buf_slice[..n].iter().position(|&b| b == b'\n');
                if let Some(newline_index) = newline_index {
                    let index = pos + newline_index;
                    let line = String::from_utf8_lossy(&buf[..index]);
                    handler.lock().await.push(&line).await;

                    let next_line_buffered = n - newline_index - 1;
                    if next_line_buffered > 0 {
                        buf.copy_within(index + 1..index + next_line_buffered, 0);
                        pos = next_line_buffered;
                    } else {
                        pos = 0;
                    }
                } else {
                    pos += n;
                    if pos >= MAX_LINE_LENGTH - 1024 {
                        const OVERFLOW: &[u8] = b"OVERFLOW";
                        pos = MAX_LINE_LENGTH - 1024;
                        buf[pos..].copy_from_slice(OVERFLOW);
                        pos += OVERFLOW.len();
                    }
                }
            }
            Err(_) => {
                log::error!("Error reading from firecracker stdout?");
                break;
            }
        }
    }
}
