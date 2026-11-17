//! A stand-in for a model provider, for tests: answers every request with a canned status and a
//! body sent in timed pieces (like a stream), and remembers what it was sent.

use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

/// A piece of the response body, sent `delay` after the previous one.
pub struct Chunk {
    pub delay: Duration,
    pub bytes: Vec<u8>,
}

impl Chunk {
    pub fn now(bytes: impl Into<Vec<u8>>) -> Self {
        Self { delay: Duration::ZERO, bytes: bytes.into() }
    }

    pub fn after(delay: Duration, bytes: impl Into<Vec<u8>>) -> Self {
        Self { delay, bytes: bytes.into() }
    }
}

pub struct Fake {
    pub url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    /// Everything received so far: each request's head and body as text.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

/// Reads one HTTP request (head and body) off the connection.
fn read_request(stream: &mut TcpStream) -> String {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    let head_end = loop {
        let n = stream.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break data.len();
        }
        data.extend_from_slice(&buf[..n]);
        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&data[..head_end]).to_ascii_lowercase();
    let length = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while data.len() < head_end + length {
        let n = stream.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }
    String::from_utf8_lossy(&data).into_owned()
}

/// Starts a server that takes the request and then never answers (a stalled provider). It holds
/// the connection open for `hold`, then closes it.
pub fn serve_silent(hold: Duration) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/messages", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            log.lock().unwrap().push(read_request(&mut stream));
            thread::sleep(hold);
        }
    });
    Fake { url, requests }
}

/// Starts a server on a free local port. Its `url` answers every request with `status` and the
/// chunks, then closes the connection.
pub fn serve(status: u16, chunks: Vec<Chunk>) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/messages", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let request = read_request(&mut stream);
            log.lock().unwrap().push(request);
            let head = format!("HTTP/1.1 {status} Test\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n");
            if stream.write_all(head.as_bytes()).is_err() {
                continue;
            }
            let _ = stream.flush();
            for chunk in &chunks {
                thread::sleep(chunk.delay);
                if stream.write_all(&chunk.bytes).is_err() {
                    break;
                }
                let _ = stream.flush();
            }
        }
    });
    Fake { url, requests }
}
