//! The redirect listener of RFC 8252 §7.3: a one-shot HTTP server on the
//! loopback interface that receives the browser's redirect.
//!
//! The order of a sign-in is: [`LoopbackListener::bind`], build the
//! authorization URL from [`LoopbackListener::redirect_uri`], then
//! [`LoopbackListener::authorize`], which hands that URL to the caller's
//! `open_url` (the system browser; tests pass a closure that plays the
//! browser) and waits for the callback. The socket is listening before the
//! browser opens, so the redirect cannot arrive first.
//!
//! What it accepts, and what it does with everything else:
//!
//! - It binds `127.0.0.1` only, never a wildcard address, on the preferred
//!   port if that is free and otherwise on one the OS picks. On Windows the
//!   socket is opened with `SO_EXCLUSIVEADDRUSE`, so another process cannot
//!   bind the same port with `SO_REUSEADDR` and take the redirect; elsewhere
//!   `SO_REUSEADDR` is left off (`tokio::net::TcpListener::bind` would set it).
//! - One `GET` of the configured path, with a `Host` header naming the bound
//!   host and port (which turns away DNS-rebinding pages), carrying `code` or
//!   `error`, ends the wait. Any other request gets a 404 or a 400 and the
//!   listener keeps waiting, until the timeout (five minutes by default) or
//!   [`CancelHandle::cancel`].
//! - The request head is capped at 8 KiB and 64 headers, a connection has 10
//!   seconds to send it, at most 16 connections are read at once, and a
//!   connection that never speaks (browsers open some speculatively) does not
//!   hold up the others.
//! - The callback's `state` is compared with the expected one in constant
//!   time, before anything else in it is looked at; a missing or different
//!   `state` ends the attempt with [`LoopbackError::StateMismatch`].
//!   `error=access_denied` is [`LoopbackError::AccessDenied`], any other
//!   `error` is [`LoopbackError::Authorization`], and the code is never
//!   returned alongside an error.
//! - The browser is shown a fixed page that echoes nothing from the request,
//!   with `Cache-Control: no-store`, `Referrer-Policy: no-referrer`, a CSP of
//!   `default-src 'none'`, and no CORS headers, so no other page can read it.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinSet;

use super::pkce::State;
use super::SecretString;

/// How long [`LoopbackListener::authorize`] waits for the callback.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// The request line plus headers, through the blank line.
const MAX_REQUEST_HEAD: usize = 8 * 1024;
const MAX_HEADERS: usize = 64;
/// How long one connection may take to send its request head, or to take
/// the response.
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
/// Connections read at the same time; more are closed unread.
const MAX_CONNECTIONS: usize = 16;
const LISTEN_BACKLOG: u32 = 16;
const MAX_PATH_LEN: usize = 256;
/// The provider's `error_description` is kept to this many characters.
const MAX_DESCRIPTION_CHARS: usize = 300;
const MAX_ERROR_CODE_CHARS: usize = 64;
/// After answering, how long and how much to read of what the client is
/// still sending, so closing does not reset the connection.
const LINGER: Duration = Duration::from_secs(1);
const MAX_DRAIN: usize = 64 * 1024;

const SUCCESS_PAGE: &str = "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>Signed in</title></head>\n<body><p>You can close this tab and return to edytlab.</p></body></html>\n";
const FAILURE_PAGE: &str = "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>Sign-in did not finish</title></head>\n<body><p>Sign-in did not finish. You can close this tab and return to edytlab.</p></body></html>\n";

/// The host name in the redirect URI. The listener binds `127.0.0.1` in
/// both cases; the name decides the `redirect_uri` and the `Host` header it
/// accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopbackHost {
    /// `127.0.0.1`, which RFC 8252 §8.3 recommends over `localhost`.
    Ipv4Loopback,
    /// `localhost`, for a provider that accepts only that name. Browsers
    /// that try `::1` first fall back to `127.0.0.1` when it refuses.
    Localhost,
}

impl LoopbackHost {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ipv4Loopback => "127.0.0.1",
            Self::Localhost => "localhost",
        }
    }
}

/// Where to listen and for how long.
#[derive(Clone, Debug)]
pub struct LoopbackConfig {
    pub host: LoopbackHost,
    /// The callback path, such as `/auth/callback`, matched exactly.
    pub path: String,
    /// Tried first; if it cannot be bound, the OS picks a port.
    pub preferred_port: Option<u16>,
    pub timeout: Duration,
}

impl LoopbackConfig {
    pub fn new(host: LoopbackHost, path: impl Into<String>) -> Self {
        Self {
            host,
            path: path.into(),
            preferred_port: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_preferred_port(mut self, port: u16) -> Self {
        self.preferred_port = Some(port);
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Ends a pending [`LoopbackListener::authorize`] with
/// [`LoopbackError::Cancelled`]. Cancelling before the wait starts counts.
#[derive(Clone)]
pub struct CancelHandle(Arc<watch::Sender<bool>>);

impl CancelHandle {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
}

impl fmt::Debug for CancelHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CancelHandle")
            .field(&*self.0.borrow())
            .finish()
    }
}

/// A callback whose `state` matched: the code, and every other parameter
/// the provider sent (`state` and `code` removed), such as an issued
/// `client_id` or the granted `scope`.
pub struct Callback {
    pub code: SecretString,
    pub params: BTreeMap<String, String>,
}

impl fmt::Debug for Callback {
    // Parameter names only: a provider could put anything in the values.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Callback")
            .field("code", &self.code)
            .field("params", &self.params.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoopbackError {
    #[error(
        "invalid callback path {0:?}: it must start with '/' and have no query, fragment, \
         space or control character"
    )]
    InvalidPath(String),
    #[error("could not start the sign-in listener on 127.0.0.1: {0}")]
    Bind(#[source] io::Error),
    #[error("could not open the browser: {0}")]
    OpenBrowser(String),
    #[error("sign-in timed out")]
    TimedOut,
    #[error("sign-in was cancelled")]
    Cancelled,
    #[error("the sign-in response did not belong to this attempt (state mismatch)")]
    StateMismatch,
    #[error("access was denied")]
    AccessDenied { description: Option<String> },
    #[error("the sign-in failed: {error}")]
    Authorization {
        error: String,
        description: Option<String>,
    },
}

/// A bound listener, waiting to be given the authorization URL.
pub struct LoopbackListener {
    listener: TcpListener,
    host: LoopbackHost,
    port: u16,
    path: Arc<str>,
    timeout: Duration,
    cancel: Arc<watch::Sender<bool>>,
}

impl fmt::Debug for LoopbackListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoopbackListener")
            .field("redirect_uri", &self.redirect_uri())
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl LoopbackListener {
    /// Binds `127.0.0.1`: the preferred port if it can, else one the OS
    /// picks. Call it before opening the browser.
    pub async fn bind(config: LoopbackConfig) -> Result<Self, LoopbackError> {
        check_path(&config.path)?;
        let listener = match config.preferred_port.filter(|&p| p != 0) {
            Some(port) => match listen_on(port) {
                Ok(listener) => listener,
                Err(_) => listen_on(0).map_err(LoopbackError::Bind)?,
            },
            None => listen_on(0).map_err(LoopbackError::Bind)?,
        };
        let port = listener.local_addr().map_err(LoopbackError::Bind)?.port();
        let (cancel, _) = watch::channel(false);
        Ok(Self {
            listener,
            host: config.host,
            port,
            path: config.path.into(),
            timeout: config.timeout,
            cancel: Arc::new(cancel),
        })
    }

    /// The port actually bound.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// `http://<host>:<port><path>`, to send as `redirect_uri` in both the
    /// authorization request and the code exchange.
    pub fn redirect_uri(&self) -> String {
        format!("http://{}:{}{}", self.host.as_str(), self.port, self.path)
    }

    pub fn cancel_handle(&self) -> CancelHandle {
        CancelHandle(self.cancel.clone())
    }

    /// Opens `authorize_url` with `open_url`, then waits for the callback.
    ///
    /// Returns the code when the callback's `state` matches
    /// `expected_state` and it carries no `error`. Consumes the listener:
    /// it accepts one callback, and the port is released on return.
    pub async fn authorize<F, E>(
        self,
        authorize_url: &str,
        open_url: F,
        expected_state: &State,
    ) -> Result<Callback, LoopbackError>
    where
        F: FnOnce(&str) -> Result<(), E>,
        E: fmt::Display,
    {
        if *self.cancel.borrow() {
            return Err(LoopbackError::Cancelled);
        }
        open_url(authorize_url).map_err(|e| LoopbackError::OpenBrowser(e.to_string()))?;
        self.wait(expected_state).await
    }

    async fn wait(self, expected_state: &State) -> Result<Callback, LoopbackError> {
        let deadline = tokio::time::Instant::now() + self.timeout;
        let mut cancelled = self.cancel.subscribe();
        let expected_host: Arc<str> = format!("{}:{}", self.host.as_str(), self.port).into();
        let bare_host_ok = self.port == 80;
        let mut connections: JoinSet<Option<(TcpStream, BTreeMap<String, String>)>> =
            JoinSet::new();

        loop {
            tokio::select! {
                biased;
                _ = cancelled.wait_for(|c| *c) => return Err(LoopbackError::Cancelled),
                _ = tokio::time::sleep_until(deadline) => return Err(LoopbackError::TimedOut),
                Some(joined) = connections.join_next(), if !connections.is_empty() => {
                    let Ok(Some((mut stream, params))) = joined else { continue };
                    let outcome = evaluate(params, expected_state);
                    let page = if outcome.is_ok() {
                        (200, "OK", SUCCESS_PAGE)
                    } else {
                        (400, "Bad Request", FAILURE_PAGE)
                    };
                    respond(&mut stream, page.0, page.1, "text/html; charset=utf-8", page.2).await;
                    return outcome;
                }
                accepted = self.listener.accept() => match accepted {
                    Ok((stream, _)) if connections.len() < MAX_CONNECTIONS => {
                        let host = expected_host.clone();
                        let path = self.path.clone();
                        connections.spawn(read_callback(stream, host, bare_host_ok, path));
                    }
                    // Over the cap: closed unread.
                    Ok(_) => {}
                    // Out of file descriptors and the like: back off a little.
                    Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                },
            }
        }
    }
}

fn check_path(path: &str) -> Result<(), LoopbackError> {
    let ok = path.starts_with('/')
        && path.len() <= MAX_PATH_LEN
        && path
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'?' && b != b'#');
    if ok {
        Ok(())
    } else {
        Err(LoopbackError::InvalidPath(path.escape_debug().to_string()))
    }
}

/// A listening socket on `127.0.0.1:port` (0 for any), without
/// `SO_REUSEADDR`, and with `SO_EXCLUSIVEADDRUSE` on Windows.
fn listen_on(port: u16) -> io::Result<TcpListener> {
    let socket = TcpSocket::new_v4()?;
    #[cfg(windows)]
    exclusive_address_use(&socket)?;
    socket.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
    socket.listen(LISTEN_BACKLOG)
}

/// Sets `SO_EXCLUSIVEADDRUSE`, which must happen before `bind`. Without it,
/// another process can bind the same address and port with `SO_REUSEADDR`
/// and receive the redirect, code included.
#[cfg(windows)]
fn exclusive_address_use(socket: &TcpSocket) -> io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        setsockopt, WSAGetLastError, SOCKET, SOCKET_ERROR, SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
    };

    let enable: i32 = 1;
    // SAFETY: `socket` owns an open socket for the whole call, `optval`
    // points at a live `i32`, and `optlen` is that `i32`'s size.
    let rc = unsafe {
        setsockopt(
            socket.as_raw_socket() as SOCKET,
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            (&enable as *const i32).cast::<u8>(),
            std::mem::size_of::<i32>() as i32,
        )
    };
    if rc == SOCKET_ERROR {
        // SAFETY: no preconditions; it reads this thread's last error.
        return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
    }
    Ok(())
}

/// Reads one request from `stream`. Returns the stream and the query when it
/// is a well-formed `GET` of the callback path for the right host, carrying
/// `code` or `error`. Anything else is answered (404 or 400) or, if it never
/// arrives in time, dropped, and gives `None`.
async fn read_callback(
    mut stream: TcpStream,
    expected_host: Arc<str>,
    bare_host_ok: bool,
    path: Arc<str>,
) -> Option<(TcpStream, BTreeMap<String, String>)> {
    let head = match tokio::time::timeout(CONNECTION_TIMEOUT, read_head(&mut stream)).await {
        Ok(Ok(head)) => head,
        Ok(Err(HeadError::TooLarge)) => {
            respond_plain(&mut stream, 431, "Request Header Fields Too Large").await;
            return None;
        }
        Ok(Err(HeadError::Closed)) | Err(_) => return None,
    };
    match parse_request(&head, &expected_host, bare_host_ok, &path) {
        Ok(params) => Some((stream, params)),
        Err(Rejection::NotFound) => {
            respond_plain(&mut stream, 404, "Not Found").await;
            None
        }
        Err(Rejection::BadRequest) => {
            respond_plain(&mut stream, 400, "Bad Request").await;
            None
        }
    }
}

enum HeadError {
    TooLarge,
    Closed,
}

/// Reads up to and including the blank line that ends the head.
async fn read_head(stream: &mut TcpStream) -> Result<Vec<u8>, HeadError> {
    let mut head = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream
            .read(&mut chunk)
            .await
            .map_err(|_| HeadError::Closed)?;
        if n == 0 {
            return Err(HeadError::Closed);
        }
        let searched_from = head.len().saturating_sub(3);
        head.extend_from_slice(&chunk[..n]);
        if let Some(end) = find(&head[searched_from..], b"\r\n\r\n") {
            let end = searched_from + end + 4;
            if end > MAX_REQUEST_HEAD {
                return Err(HeadError::TooLarge);
            }
            head.truncate(end);
            return Ok(head);
        }
        if head.len() > MAX_REQUEST_HEAD {
            return Err(HeadError::TooLarge);
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[derive(Debug, PartialEq, Eq)]
enum Rejection {
    NotFound,
    BadRequest,
}

/// Parses a request head strictly: `GET <origin-form> HTTP/1.x`, at most
/// [`MAX_HEADERS`] `name: value` headers with no folding or control
/// characters, exactly one `Host` naming the listener, no body.
fn parse_request(
    head: &[u8],
    expected_host: &str,
    bare_host_ok: bool,
    path: &str,
) -> Result<BTreeMap<String, String>, Rejection> {
    use Rejection::{BadRequest, NotFound};

    let text = std::str::from_utf8(head).map_err(|_| BadRequest)?;
    let text = text.strip_suffix("\r\n\r\n").ok_or(BadRequest)?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().ok_or(BadRequest)?;
    if request_line.bytes().any(|b| b.is_ascii_control()) {
        return Err(BadRequest);
    }
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(BadRequest);
    };
    if method != "GET" || !matches!(version, "HTTP/1.1" | "HTTP/1.0") {
        return Err(BadRequest);
    }
    if !target.starts_with('/') || target.contains('#') {
        return Err(BadRequest);
    }

    let mut host = None;
    for (count, line) in lines.enumerate() {
        if count >= MAX_HEADERS {
            return Err(BadRequest);
        }
        // Folded (obsolete) continuation lines are refused, not joined.
        if line.starts_with([' ', '\t']) {
            return Err(BadRequest);
        }
        let (name, value) = line.split_once(':').ok_or(BadRequest)?;
        if name.is_empty() || !name.bytes().all(is_token_byte) {
            return Err(BadRequest);
        }
        if value.bytes().any(|b| b.is_ascii_control() && b != b'\t') {
            return Err(BadRequest);
        }
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("host") {
            if host.replace(value).is_some() {
                return Err(BadRequest);
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && value != "0")
        {
            return Err(BadRequest);
        }
    }
    let host = host.ok_or(BadRequest)?;
    let host_ok = host.eq_ignore_ascii_case(expected_host)
        || (bare_host_ok
            && expected_host
                .strip_suffix(":80")
                .is_some_and(|bare| host.eq_ignore_ascii_case(bare)));
    if !host_ok {
        return Err(BadRequest);
    }

    let (request_path, query) = target.split_once('?').unwrap_or((target, ""));
    if request_path != path {
        return Err(NotFound);
    }
    let mut params = BTreeMap::new();
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        // RFC 6749 §3.1: parameters must not be repeated.
        if params
            .insert(name.into_owned(), value.into_owned())
            .is_some()
        {
            return Err(BadRequest);
        }
    }
    let present = |k: &str| params.get(k).is_some_and(|v| !v.is_empty());
    if !present("code") && !present("error") {
        return Err(BadRequest);
    }
    Ok(params)
}

/// RFC 9110 §5.6.2 `tchar`.
fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// Turns a callback that reached the right path into the attempt's result.
/// `state` is checked first: nothing else in a callback from someone else
/// is believed, its error included.
fn evaluate(
    mut params: BTreeMap<String, String>,
    expected_state: &State,
) -> Result<Callback, LoopbackError> {
    let state_ok = params
        .remove("state")
        .is_some_and(|s| expected_state.matches(&s));
    if !state_ok {
        return Err(LoopbackError::StateMismatch);
    }
    if let Some(error) = params.remove("error").filter(|e| !e.is_empty()) {
        let description = params
            .remove("error_description")
            .map(|d| printable(&d, MAX_DESCRIPTION_CHARS));
        return Err(if error == "access_denied" {
            LoopbackError::AccessDenied { description }
        } else {
            LoopbackError::Authorization {
                error: printable(&error, MAX_ERROR_CODE_CHARS),
                description,
            }
        });
    }
    let code = params.remove("code").unwrap_or_default();
    Ok(Callback {
        code: SecretString::new(code),
        params,
    })
}

/// `s` cut to `max` characters, control characters replaced by spaces.
fn printable(s: &str, max: usize) -> String {
    s.chars()
        .take(max)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

async fn respond_plain(stream: &mut TcpStream, status: u16, reason: &str) {
    respond(stream, status, reason, "text/plain; charset=utf-8", reason).await;
}

/// Writes a complete response and closes the connection. Errors are
/// ignored: the browser may already have gone.
async fn respond(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &str,
) {
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Pragma: no-cache\r\n\
         Referrer-Policy: no-referrer\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\n\
         X-Frame-Options: DENY\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );
    let sent = tokio::time::timeout(CONNECTION_TIMEOUT, async {
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await
    })
    .await;
    if !matches!(sent, Ok(Ok(()))) {
        return;
    }
    // A lingering close: read what the client is still sending before the
    // socket is dropped. Closing with unread input makes the OS send a
    // reset, which can destroy the response before the client reads it
    // (a 431 for an oversized request, say).
    let _ = tokio::time::timeout(LINGER, async {
        let mut sink = [0u8; 1024];
        let mut drained = 0;
        while drained < MAX_DRAIN {
            match stream.read(&mut sink).await {
                Ok(0) | Err(_) => break,
                Ok(n) => drained += n,
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &str = "127.0.0.1:1455";

    fn parse(head: &str) -> Result<BTreeMap<String, String>, Rejection> {
        parse_request(head.as_bytes(), HOST, false, "/cb")
    }

    #[tokio::test]
    async fn the_listener_does_not_set_so_reuseaddr() {
        // tokio's `TcpListener::bind` sets SO_REUSEADDR on Unix; the
        // listener must not, so it builds its socket itself.
        let listener =
            LoopbackListener::bind(LoopbackConfig::new(LoopbackHost::Ipv4Loopback, "/cb"))
                .await
                .unwrap();
        let sock = socket2::SockRef::from(&listener.listener);
        assert!(!sock.reuse_address().unwrap());
        let addr = listener.listener.local_addr().unwrap();
        assert_eq!(addr.ip(), Ipv4Addr::LOCALHOST);
    }

    #[test]
    fn parses_a_callback() {
        let params = parse(
            "GET /cb?code=a%20b&state=s HTTP/1.1\r\nHost: 127.0.0.1:1455\r\nUser-Agent: x\r\n\r\n",
        )
        .unwrap();
        assert_eq!(params.get("code").map(String::as_str), Some("a b"));
        assert_eq!(params.get("state").map(String::as_str), Some("s"));
    }

    #[test]
    fn header_names_and_host_are_case_insensitive_and_trimmed() {
        parse("GET /cb?code=a HTTP/1.1\r\nhOsT:\t127.0.0.1:1455 \r\n\r\n").unwrap();
    }

    #[test]
    fn refuses_what_it_should() {
        for head in [
            "GET /cb?code=a HTTP/1.1\r\nHost: 127.0.0.1:1455\r\n\r\n\r\n",
            "GET /cb?code=a HTTP/1.1\nHost: 127.0.0.1:1455\r\n\r\n",
            "GET  /cb?code=a HTTP/1.1\r\nHost: 127.0.0.1:1455\r\n\r\n",
            "GET /cb?code=a HTTP/1.1 x\r\nHost: 127.0.0.1:1455\r\n\r\n",
            "get /cb?code=a HTTP/1.1\r\nHost: 127.0.0.1:1455\r\n\r\n",
            "GET /cb?code=a#f HTTP/1.1\r\nHost: 127.0.0.1:1455\r\n\r\n",
            "GET /cb?code=a\x00 HTTP/1.1\r\nHost: 127.0.0.1:1455\r\n\r\n",
            "GET /cb?code=a HTTP/1.1\r\nHost: 127.0.0.1:1455\r\nBad Name: x\r\n\r\n",
            "GET /cb?code=a HTTP/1.1\r\nHost: 127.0.0.1:1455\r\nNoColon\r\n\r\n",
            "GET /cb?code=a HTTP/1.1\r\nHost: 127.0.0.1:1455\r\nX: a\x01b\r\n\r\n",
            "GET /cb?code=a HTTP/1.1\r\nHost: 127.0.0.1:1455\r\nContent-Length: 1\r\n\r\n",
            "GET /cb?code=&error= HTTP/1.1\r\nHost: 127.0.0.1:1455\r\n\r\n",
        ] {
            assert_eq!(parse(head), Err(Rejection::BadRequest), "{head:?}");
        }
        let many = format!(
            "GET /cb?code=a HTTP/1.1\r\nHost: 127.0.0.1:1455\r\n{}\r\n",
            "X: y\r\n".repeat(MAX_HEADERS)
        );
        assert_eq!(parse(&many), Err(Rejection::BadRequest));
        assert_eq!(
            parse("GET /CB?code=a HTTP/1.1\r\nHost: 127.0.0.1:1455\r\n\r\n"),
            Err(Rejection::NotFound)
        );
        assert_eq!(
            parse("GET /cb/?code=a HTTP/1.1\r\nHost: 127.0.0.1:1455\r\n\r\n"),
            Err(Rejection::NotFound)
        );
        assert_eq!(
            parse("GET /%63b?code=a HTTP/1.1\r\nHost: 127.0.0.1:1455\r\n\r\n"),
            Err(Rejection::NotFound)
        );
    }

    #[test]
    fn a_bare_host_is_accepted_only_on_port_80() {
        let head = "GET /cb?code=a HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(
            parse_request(head.as_bytes(), "127.0.0.1:80", true, "/cb").map(|_| ()),
            Ok(())
        );
        assert_eq!(
            parse_request(head.as_bytes(), HOST, false, "/cb").map(|_| ()),
            Err(Rejection::BadRequest)
        );
    }

    #[test]
    fn evaluation_checks_state_before_anything_else() {
        let state = State::generate().unwrap();
        let with = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        assert!(matches!(
            evaluate(with(&[("code", "c"), ("error", "access_denied")]), &state),
            Err(LoopbackError::StateMismatch)
        ));
        assert!(matches!(
            evaluate(
                with(&[
                    ("code", "c"),
                    ("error", "access_denied"),
                    ("state", state.as_str())
                ]),
                &state
            ),
            Err(LoopbackError::AccessDenied { .. })
        ));
        let err = evaluate(
            with(&[
                ("error", "bad\u{7}code"),
                ("error_description", &"\n".repeat(1000)),
                ("state", state.as_str()),
            ]),
            &state,
        )
        .unwrap_err();
        match err {
            LoopbackError::Authorization { error, description } => {
                assert_eq!(error, "bad code");
                let description = description.unwrap();
                assert_eq!(description.chars().count(), MAX_DESCRIPTION_CHARS);
                assert!(description.chars().all(|c| c == ' '));
            }
            other => panic!("{other:?}"),
        }
    }
}
