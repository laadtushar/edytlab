//! A loopback HTTP server for the downloader tests.
//!
//! Binds `127.0.0.1:0` and serves canned bodies from background threads,
//! one request per connection with `Connection: close`. Nothing here
//! leaves the machine, so CI downloads nothing.
//!
//! Each path has a [`Route`] that says what the server does with it:
//! honour `Range` or ignore it, answer with an error status, cut the
//! connection part-way through the first response, answer slowly. Every
//! request is recorded as `(path, Range header)`, which is what the tests
//! assert on to prove a download resumed, restarted or never happened.

#![allow(dead_code)] // Each test binary uses a different part.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// What the server does for one path.
#[derive(Clone)]
pub struct Route {
    body: Vec<u8>,
    status: u16,
    honour_range: bool,
    reject_range: bool,
    cut_after: Option<usize>,
    chunk_delay: Option<Duration>,
    head_delay: Option<Duration>,
    location: Option<String>,
}

impl Route {
    /// `200` with `body`, honouring `Range: bytes=N-` with a `206`.
    pub fn new(body: impl Into<Vec<u8>>) -> Self {
        Self {
            body: body.into(),
            status: 200,
            honour_range: true,
            reject_range: false,
            cut_after: None,
            chunk_delay: None,
            head_delay: None,
            location: None,
        }
    }

    /// Answer every request with a `302` to `to` and an empty body. `to`
    /// is sent as the `Location` header verbatim, so it can be a path on
    /// this server or an absolute URL anywhere.
    pub fn redirect(mut self, to: impl Into<String>) -> Self {
        self.status = 302;
        self.location = Some(to.into());
        self
    }

    /// Answer every request with this status and an empty body.
    pub fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    /// Ignore `Range` and send the whole body with `200`.
    pub fn ignore_range(mut self) -> Self {
        self.honour_range = false;
        self
    }

    /// Answer any request that has a `Range` header with `416`.
    pub fn reject_range(mut self) -> Self {
        self.reject_range = true;
        self
    }

    /// On the **first** request for this path only: send the headers for
    /// the full body, then close the connection after `n` bytes of it.
    pub fn cut_after(mut self, n: usize) -> Self {
        self.cut_after = Some(n);
        self
    }

    /// Pause between 16 KiB pieces of the body.
    pub fn chunk_delay(mut self, delay: Duration) -> Self {
        self.chunk_delay = Some(delay);
        self
    }

    /// Wait this long after receiving a request before answering at all.
    pub fn head_delay(mut self, delay: Duration) -> Self {
        self.head_delay = Some(delay);
        self
    }
}

/// One request the server saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub path: String,
    /// The `Range` header, verbatim.
    pub range: Option<String>,
}

struct Shared {
    routes: Mutex<HashMap<String, Route>>,
    hits: Mutex<Vec<Hit>>,
    stop: AtomicBool,
}

pub struct Server {
    addr: std::net::SocketAddr,
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start(routes: impl IntoIterator<Item = (&'static str, Route)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        listener.set_nonblocking(true).expect("nonblocking");
        let addr = listener.local_addr().expect("local addr");
        let shared = Arc::new(Shared {
            routes: Mutex::new(
                routes
                    .into_iter()
                    .map(|(p, r)| (p.to_string(), r))
                    .collect(),
            ),
            hits: Mutex::new(Vec::new()),
            stop: AtomicBool::new(false),
        });
        let accept = {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                while !shared.stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let shared = Arc::clone(&shared);
                            thread::spawn(move || {
                                let _ = handle(stream, &shared);
                            });
                        }
                        Err(_) => thread::sleep(Duration::from_millis(5)),
                    }
                }
            })
        };
        Self {
            addr,
            shared,
            accept: Some(accept),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// Replace what a path does, for tests that change the server's mind
    /// between two fetches.
    pub fn set_route(&self, path: &str, route: Route) {
        self.shared
            .routes
            .lock()
            .unwrap()
            .insert(path.to_string(), route);
    }

    /// Every request for `path`, oldest first.
    pub fn hits(&self, path: &str) -> Vec<Hit> {
        self.shared
            .hits
            .lock()
            .unwrap()
            .iter()
            .filter(|h| h.path == path)
            .cloned()
            .collect()
    }

    pub fn total_hits(&self) -> usize {
        self.shared.hits.lock().unwrap().len()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
    }
}

fn handle(mut stream: TcpStream, shared: &Shared) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;

    // Read up to the blank line that ends the request head.
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte)? == 0 {
            return Ok(());
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let mut lines = head.split("\r\n");
    let path = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();
    let range = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("range"))
        .map(|(_, v)| v.trim().to_string());

    let (route, first) = {
        let mut hits = shared.hits.lock().unwrap();
        let first = !hits.iter().any(|h| h.path == path);
        hits.push(Hit {
            path: path.clone(),
            range: range.clone(),
        });
        (shared.routes.lock().unwrap().get(&path).cloned(), first)
    };
    let Some(route) = route else {
        return respond(&mut stream, 404, &[], &[], None, None);
    };

    if let Some(delay) = route.head_delay {
        thread::sleep(delay);
    }
    if route.status != 200 {
        let extra: Vec<String> = route
            .location
            .iter()
            .map(|to| format!("Location: {to}"))
            .collect();
        return respond(&mut stream, route.status, &extra, &[], None, None);
    }

    let len = route.body.len();
    let start = range
        .as_deref()
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.strip_suffix('-'))
        .and_then(|n| n.parse::<usize>().ok());

    let cut = if first { route.cut_after } else { None };
    match start {
        Some(_) if route.reject_range => {
            let cr = format!("Content-Range: bytes */{len}");
            respond(&mut stream, 416, &[cr], &[], None, None)
        }
        Some(from) if route.honour_range => {
            if from >= len {
                let cr = format!("Content-Range: bytes */{len}");
                return respond(&mut stream, 416, &[cr], &[], None, None);
            }
            let cr = format!("Content-Range: bytes {from}-{}/{len}", len - 1);
            respond(
                &mut stream,
                206,
                &[cr],
                &route.body[from..],
                cut,
                route.chunk_delay,
            )
        }
        _ => respond(&mut stream, 200, &[], &route.body, cut, route.chunk_delay),
    }
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    extra: &[String],
    body: &[u8],
    cut_after: Option<usize>,
    chunk_delay: Option<Duration>,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        206 => "Partial Content",
        302 => "Found",
        404 => "Not Found",
        416 => "Range Not Satisfiable",
        _ => "Status",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for line in extra {
        head.push_str(line);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;

    // A cut sends fewer bytes than Content-Length promised.
    let send = &body[..cut_after.map_or(body.len(), |n| n.min(body.len()))];
    for piece in send.chunks(16 * 1024) {
        stream.write_all(piece)?;
        if let Some(delay) = chunk_delay {
            thread::sleep(delay);
        }
    }
    stream.flush()?;
    stream.shutdown(Shutdown::Both)
}
