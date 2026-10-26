//! The OAuth loopback listener (`ai::oauth::loopback`), driven over real
//! TCP on 127.0.0.1.
//!
//! Each test binds a listener, then plays the browser from the `open_url`
//! closure it injects: a `reqwest` GET for an ordinary redirect, or raw
//! bytes when the test needs a request no HTTP client would send.

use std::net::{Ipv4Addr, SocketAddr, TcpListener as StdTcpListener};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ai::oauth::loopback::{
    LoopbackConfig, LoopbackError, LoopbackHost, LoopbackListener, DEFAULT_TIMEOUT,
};
use ai::oauth::pkce::State;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const PATH: &str = "/auth/callback";
const AUTHORIZE_URL: &str = "https://auth.example.com/authorize?client_id=c";

fn config() -> LoopbackConfig {
    LoopbackConfig::new(LoopbackHost::Ipv4Loopback, PATH).with_timeout(Duration::from_secs(20))
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

/// What the browser saw for one request.
#[derive(Debug)]
struct Seen {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

async fn get(url: String) -> Seen {
    let resp = http().get(url).send().await.expect("the listener answers");
    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
        .collect();
    let body = resp.text().await.unwrap_or_default();
    Seen {
        status,
        headers,
        body,
    }
}

/// [`get`], or `None` when nothing answers (the listener has gone).
async fn try_get(url: String) -> Option<Seen> {
    let resp = http().get(url).send().await.ok()?;
    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
        .collect();
    let body = resp.text().await.unwrap_or_default();
    Some(Seen {
        status,
        headers,
        body,
    })
}

/// Sends `raw` as-is and returns the status line's code (0 if the listener
/// closed without answering).
async fn raw(port: u16, raw: Vec<u8>) -> u16 {
    // 0 too when nothing listens any more.
    let Ok(mut stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await else {
        return 0;
    };
    // The listener may answer and close before reading everything.
    let _ = stream.write_all(&raw).await;
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf);
    text.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}

fn callback_url(port: u16, query: &str) -> String {
    format!("http://127.0.0.1:{port}{PATH}?{query}")
}

/// Runs `browser` as the injected opener (on its own task) and returns the
/// listener's result plus what the browser task returned.
async fn run<F, Fut, T>(
    listener: LoopbackListener,
    state: &State,
    browser: F,
) -> (Result<ai::oauth::loopback::Callback, LoopbackError>, T)
where
    F: FnOnce(u16) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let port = listener.port();
    let task: Arc<Mutex<Option<tokio::task::JoinHandle<T>>>> = Arc::new(Mutex::new(None));
    let slot = task.clone();
    let result = listener
        .authorize(
            AUTHORIZE_URL,
            move |_url: &str| -> Result<(), String> {
                *slot.lock().unwrap() = Some(tokio::spawn(browser(port)));
                Ok(())
            },
            state,
        )
        .await;
    let handle = task.lock().unwrap().take().expect("open_url was called");
    (result, handle.await.unwrap())
}

#[tokio::test]
async fn a_real_redirect_round_trip_returns_the_code() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let port = listener.port();
    assert_eq!(
        listener.redirect_uri(),
        format!("http://127.0.0.1:{port}{PATH}")
    );
    let opened = Arc::new(Mutex::new(None::<String>));
    let seen_url = opened.clone();
    let query = format!(
        "code=CODE-MARKER-123&state={}&client_id=issued-client&scope=openid+profile",
        state.as_str()
    );
    let task = Arc::new(Mutex::new(None));
    let slot = task.clone();

    let callback = listener
        .authorize(
            AUTHORIZE_URL,
            move |url: &str| -> Result<(), String> {
                *seen_url.lock().unwrap() = Some(url.to_string());
                *slot.lock().unwrap() = Some(tokio::spawn(get(callback_url(port, &query))));
                Ok(())
            },
            &state,
        )
        .await
        .expect("a callback with the right state returns its code");
    let handle = task.lock().unwrap().take().unwrap();
    let page = handle.await.unwrap();

    assert_eq!(opened.lock().unwrap().as_deref(), Some(AUTHORIZE_URL));
    assert_eq!(callback.code.expose_secret(), "CODE-MARKER-123");
    assert_eq!(
        callback.params.get("client_id").map(String::as_str),
        Some("issued-client")
    );
    assert_eq!(
        callback.params.get("scope").map(String::as_str),
        Some("openid profile")
    );
    assert!(!callback.params.contains_key("code"));
    assert!(!callback.params.contains_key("state"));

    // The page: static, no echo, not cached, no CORS.
    assert_eq!(page.status, 200);
    assert!(page
        .body
        .contains("You can close this tab and return to edytlab"));
    for echoed in ["CODE-MARKER-123", state.as_str(), "issued-client"] {
        assert!(!page.body.contains(echoed), "{echoed} echoed");
    }
    assert_eq!(page.header("cache-control"), Some("no-store"));
    assert!(page
        .headers
        .iter()
        .all(|(k, _)| !k.to_ascii_lowercase().starts_with("access-control-")));
    assert_eq!(page.header("referrer-policy"), Some("no-referrer"));
}

#[tokio::test]
async fn a_wrong_path_is_404_and_the_listener_keeps_waiting() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let query = format!("code=good&state={}", state.as_str());

    let (result, (wrong, prefix, favicon)) = run(listener, &state, move |port| async move {
        let wrong = get(format!("http://127.0.0.1:{port}/callback?{query}")).await;
        let prefix = get(format!("http://127.0.0.1:{port}{PATH}/x?{query}")).await;
        let favicon = get(format!("http://127.0.0.1:{port}/favicon.ico")).await;
        get(callback_url(port, &query)).await;
        (wrong.status, prefix.status, favicon.status)
    })
    .await;

    assert_eq!((wrong, prefix, favicon), (404, 404, 404));
    assert_eq!(result.unwrap().code.expose_secret(), "good");
}

#[tokio::test]
async fn a_wrong_or_missing_host_header_is_400_and_the_listener_keeps_waiting() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let target = format!("{PATH}?code=good&state={}", state.as_str());

    let (result, codes) = run(listener, &state, move |port| async move {
        let mut codes = Vec::new();
        for host in [
            // DNS rebinding: the page's own origin.
            "Host: evil.example\r\n".to_string(),
            format!("Host: evil.example:{port}\r\n"),
            // Another loopback name, or the right name on another port.
            format!("Host: localhost:{port}\r\n"),
            format!("Host: 127.0.0.1:{}\r\n", port.wrapping_add(1)),
            "Host: 127.0.0.1\r\n".to_string(),
            // Two Host headers, or none.
            format!("Host: 127.0.0.1:{port}\r\nHost: evil.example\r\n"),
            String::new(),
        ] {
            let req = format!("GET {target} HTTP/1.1\r\n{host}Connection: close\r\n\r\n");
            codes.push(raw(port, req.into_bytes()).await);
        }
        get(format!("http://127.0.0.1:{port}{target}")).await;
        codes
    })
    .await;

    assert!(codes.iter().all(|&c| c == 400), "{codes:?}");
    assert_eq!(result.unwrap().code.expose_secret(), "good");
}

#[tokio::test]
async fn malformed_or_oversized_requests_are_refused_and_the_listener_keeps_waiting() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let query = format!("code=good&state={}", state.as_str());

    let (result, codes) = run(listener, &state, move |port| async move {
        let host = format!("Host: 127.0.0.1:{port}\r\n");
        let target = format!("{PATH}?{query}");
        let requests: Vec<Vec<u8>> = vec![
            // Not a GET.
            format!("POST {target} HTTP/1.1\r\n{host}Content-Length: 0\r\n\r\n").into_bytes(),
            format!("HEAD {target} HTTP/1.1\r\n{host}\r\n").into_bytes(),
            // Not HTTP/1.x, or not a request line at all.
            format!("GET {target} HTTP/2.0\r\n{host}\r\n").into_bytes(),
            format!("GET {target}\r\n{host}\r\n").into_bytes(),
            b"\x16\x03\x01\x02\x00\x01\x00\x01\xfc\x03\x03\r\n\r\n".to_vec(),
            // An absolute-form target, a body, a folded header.
            format!("GET http://127.0.0.1:{port}{target} HTTP/1.1\r\n{host}\r\n").into_bytes(),
            format!("GET {target} HTTP/1.1\r\n{host}Content-Length: 5\r\n\r\nhello").into_bytes(),
            format!("GET {target} HTTP/1.1\r\n{host}Transfer-Encoding: chunked\r\n\r\n0\r\n\r\n")
                .into_bytes(),
            format!("GET {target} HTTP/1.1\r\n{host}X-A: 1\r\n  folded\r\n\r\n").into_bytes(),
            // Over the size caps: a long request line, a long header block.
            format!("GET {PATH}?pad={} HTTP/1.1\r\n{host}\r\n", "a".repeat(9000)).into_bytes(),
            format!(
                "GET {target} HTTP/1.1\r\n{host}{}\r\n",
                "X-Pad: aaaaaaaaaa\r\n".repeat(800)
            )
            .into_bytes(),
            // The callback path with neither a code nor an error.
            format!("GET {PATH} HTTP/1.1\r\n{host}\r\n").into_bytes(),
            format!("GET {PATH}?foo=bar HTTP/1.1\r\n{host}\r\n").into_bytes(),
            // A parameter sent twice (RFC 6749 §3.1 forbids it).
            format!("GET {target}&code=other HTTP/1.1\r\n{host}\r\n").into_bytes(),
        ];
        let mut codes = Vec::new();
        for req in requests {
            codes.push(raw(port, req).await);
        }
        get(callback_url(port, &query)).await;
        codes
    })
    .await;

    // Each is refused with a 400, except the two over the size cap, which
    // get a 431. None is a 200, and none ends the wait.
    let mut expected = vec![400; codes.len()];
    expected[9] = 431;
    expected[10] = 431;
    assert_eq!(codes, expected);
    assert_eq!(result.unwrap().code.expose_secret(), "good");
}

#[tokio::test]
async fn a_silent_connection_does_not_hold_up_the_callback() {
    // Browsers open speculative connections that may never send a byte.
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let query = format!("code=good&state={}", state.as_str());
    let started = Instant::now();

    let (result, _) = run(listener, &state, move |port| async move {
        let idle = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        get(callback_url(port, &query)).await;
        drop(idle);
    })
    .await;

    assert_eq!(result.unwrap().code.expose_secret(), "good");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn a_state_mismatch_is_400_and_the_listener_keeps_waiting() {
    // Any page can make the browser request the callback URL. A wrong
    // `state` is refused, but must not end the sign-in in progress.
    let state = State::generate().unwrap();
    let other = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let forged = format!("code=forged&state={}", other.as_str());
    let real = format!("code=good&state={}", state.as_str());

    let (result, (page, after)) = run(listener, &state, move |port| async move {
        let page = try_get(callback_url(port, &forged)).await;
        let after = try_get(callback_url(port, &real)).await;
        (page, after)
    })
    .await;

    assert_eq!(
        result
            .expect("the real callback still ends the wait")
            .code
            .expose_secret(),
        "good"
    );
    let page = page.expect("the forged callback is answered");
    assert_eq!(page.status, 400);
    assert!(!page.body.contains("forged"));
    assert_eq!(page.header("cache-control"), Some("no-store"));
    assert_eq!(after.map(|p| p.status), Some(200));
}

#[tokio::test]
async fn a_missing_state_is_400_and_the_listener_keeps_waiting() {
    // What `<img src="http://127.0.0.1:1455/auth/callback?code=x">` sends.
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let real = format!("code=good&state={}", state.as_str());
    let (result, (poke, after)) = run(listener, &state, move |port| async move {
        let poke = try_get(callback_url(port, "code=x")).await;
        let after = try_get(callback_url(port, &real)).await;
        (poke, after)
    })
    .await;
    assert_eq!(
        result
            .expect("the real callback still ends the wait")
            .code
            .expose_secret(),
        "good"
    );
    assert_eq!(poke.map(|p| p.status), Some(400));
    assert_eq!(after.map(|p| p.status), Some(200));
}

#[tokio::test]
async fn access_denied_is_its_own_error() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let query = format!(
        "error=access_denied&error_description=The+user+declined&state={}",
        state.as_str()
    );
    let (result, page) = run(listener, &state, move |port| async move {
        get(callback_url(port, &query)).await
    })
    .await;

    match result {
        Err(LoopbackError::AccessDenied { description }) => {
            assert_eq!(description.as_deref(), Some("The user declined"));
        }
        other => panic!("expected AccessDenied, got {other:?}"),
    }
    assert!(
        !page.body.contains("declined"),
        "the description is not echoed"
    );
}

#[tokio::test]
async fn another_oauth_error_is_reported_apart_from_access_denied() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let query = format!("error=server_error&state={}", state.as_str());
    let (result, _) = run(listener, &state, move |port| async move {
        get(callback_url(port, &query)).await
    })
    .await;
    match result {
        Err(LoopbackError::Authorization { error, description }) => {
            assert_eq!(error, "server_error");
            assert_eq!(description, None);
        }
        other => panic!("expected Authorization, got {other:?}"),
    }
}

#[tokio::test]
async fn an_error_with_the_wrong_state_is_400_and_the_listener_keeps_waiting() {
    // A forged denial is not believed: only one with the right `state` ends
    // the attempt (`access_denied_is_its_own_error`).
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let real = format!("code=good&state={}", state.as_str());
    let (result, codes) = run(listener, &state, move |port| async move {
        let mut codes = Vec::new();
        for forged in ["error=access_denied&state=forged", "error=access_denied"] {
            codes.push(try_get(callback_url(port, forged)).await.map(|p| p.status));
        }
        codes.push(try_get(callback_url(port, &real)).await.map(|p| p.status));
        codes
    })
    .await;
    assert_eq!(
        result
            .expect("the real callback still ends the wait")
            .code
            .expose_secret(),
        "good"
    );
    assert_eq!(codes, [Some(400), Some(400), Some(200)]);
}

#[tokio::test]
async fn a_background_request_is_400_and_the_listener_keeps_waiting() {
    // A redirect is a top-level navigation. A request a page makes in the
    // background (an image, a fetch, a frame's subresource) says otherwise in
    // `Sec-Fetch-Mode`, and is refused before its `state` is looked at.
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let target = format!("{PATH}?code=from-background&state={}", state.as_str());
    let real = format!("{PATH}?code=good&state={}", state.as_str());

    let (result, (codes, navigated)) = run(listener, &state, move |port| async move {
        let mut codes = Vec::new();
        for mode in [
            "Sec-Fetch-Mode: no-cors\r\n".to_string(),
            "Sec-Fetch-Mode: cors\r\n".to_string(),
            "Sec-Fetch-Mode: same-origin\r\n".to_string(),
            "Sec-Fetch-Mode: websocket\r\n".to_string(),
            "Sec-Fetch-Mode: \r\n".to_string(),
            "Sec-Fetch-Mode: navigate\r\nSec-Fetch-Mode: no-cors\r\n".to_string(),
        ] {
            let req = format!(
                "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{mode}Connection: close\r\n\r\n"
            );
            codes.push(raw(port, req.into_bytes()).await);
        }
        let req = format!(
            "GET {real} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nSec-Fetch-Mode: navigate\r\n\
             Sec-Fetch-Dest: document\r\nConnection: close\r\n\r\n"
        );
        let navigated = raw(port, req.into_bytes()).await;
        (codes, navigated)
    })
    .await;

    assert_eq!(
        result
            .expect("the navigation ends the wait")
            .code
            .expose_secret(),
        "good"
    );
    assert!(codes.iter().all(|&c| c == 400), "{codes:?}");
    assert_eq!(navigated, 200);
}

#[tokio::test]
async fn it_times_out() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config().with_timeout(Duration::from_millis(300)))
        .await
        .unwrap();
    let started = Instant::now();
    let result = listener
        .authorize(AUTHORIZE_URL, |_: &str| Ok::<(), String>(()), &state)
        .await;
    assert!(matches!(result, Err(LoopbackError::TimedOut)), "{result:?}");
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(300), "{waited:?}");
    assert!(waited < Duration::from_secs(5), "{waited:?}");
}

#[test]
fn the_default_timeout_is_about_five_minutes() {
    assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(300));
    assert_eq!(
        LoopbackConfig::new(LoopbackHost::Ipv4Loopback, PATH).timeout,
        DEFAULT_TIMEOUT
    );
}

#[tokio::test]
async fn it_can_be_cancelled_while_waiting() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let cancel = listener.cancel_handle();
    let started = Instant::now();
    let result = listener
        .authorize(
            AUTHORIZE_URL,
            move |_: &str| -> Result<(), String> {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    cancel.cancel();
                });
                Ok(())
            },
            &state,
        )
        .await;
    assert!(
        matches!(result, Err(LoopbackError::Cancelled)),
        "{result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn the_wait_can_run_on_a_task_of_its_own() {
    // A Tauri async command, or `tokio::spawn`, needs a `Send` future: the
    // wait must hold nothing that is not `Send` (such as the read guard a
    // `watch::Receiver` hands out) across an await.
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let cancel = listener.cancel_handle();
    let waiting = tokio::spawn(async move {
        listener
            .authorize(AUTHORIZE_URL, |_: &str| Ok::<(), String>(()), &state)
            .await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    cancel.cancel();
    let result = waiting.await.unwrap();
    assert!(
        matches!(result, Err(LoopbackError::Cancelled)),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_cancel_before_waiting_still_counts() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    listener.cancel_handle().cancel();
    let result = listener
        .authorize(AUTHORIZE_URL, |_: &str| Ok::<(), String>(()), &state)
        .await;
    assert!(
        matches!(result, Err(LoopbackError::Cancelled)),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_failed_browser_open_is_reported() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let result = listener
        .authorize(
            AUTHORIZE_URL,
            |_: &str| Err::<(), _>("no browser here"),
            &state,
        )
        .await;
    match result {
        Err(LoopbackError::OpenBrowser(msg)) => assert!(msg.contains("no browser here")),
        other => panic!("expected OpenBrowser, got {other:?}"),
    }
}

#[tokio::test]
async fn binds_the_preferred_port_or_falls_back_to_an_ephemeral_one() {
    // A free port: take it, release it, then ask for it.
    let free = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let wanted = free.local_addr().unwrap().port();
    drop(free);
    let listener = LoopbackListener::bind(config().with_preferred_port(wanted))
        .await
        .unwrap();
    assert_eq!(listener.port(), wanted);

    // A taken port: another listener holds it, so we get another one.
    let busy = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let taken = busy.local_addr().unwrap().port();
    let fallback = LoopbackListener::bind(config().with_preferred_port(taken))
        .await
        .unwrap();
    assert_ne!(fallback.port(), taken);
    assert_ne!(fallback.port(), 0);
    assert_eq!(
        fallback.redirect_uri(),
        format!("http://127.0.0.1:{}{PATH}", fallback.port())
    );
    drop(busy);
}

#[tokio::test]
async fn another_socket_cannot_take_over_the_port() {
    // SO_REUSEADDR on a second socket must not let it bind our address.
    // This checks the guard on Windows only: there SO_REUSEADDR alone lets
    // a second socket bind a listening one's address, and the listener's
    // SO_EXCLUSIVEADDRUSE is what stops it. On Linux and macOS a socket
    // cannot bind over a listening one even with SO_REUSEADDR set on both
    // (only SO_REUSEPORT, which neither sets, would allow it), so this passes
    // there whatever the listener does. The guard on Unix, no SO_REUSEADDR
    // on the listener, is the unit test `the_listener_does_not_set_so_reuseaddr`
    // in `src/oauth/loopback.rs`.
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, listener.port()));
    let squatter = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .unwrap();
    squatter.set_reuse_address(true).unwrap();
    let bound = squatter.bind(&addr.into());
    assert!(bound.is_err(), "a second socket bound {addr}");
    drop(listener);
}

#[tokio::test]
async fn the_callback_issuer_is_checked_as_rfc_9207_says() {
    const ISSUER: &str = "https://auth.example.com";
    let callback_with = |query: String| async move {
        let state = State::generate().unwrap();
        let listener = LoopbackListener::bind(config()).await.unwrap();
        let query = format!("{query}&state={}", state.as_str());
        let (result, _) = run(listener, &state, move |port| async move {
            get(callback_url(port, &query)).await
        })
        .await;
        result.unwrap()
    };

    // The issuer's own `iss`, percent-encoded as a form value.
    let right = callback_with(format!("code=c&iss={}", "https%3A%2F%2Fauth.example.com")).await;
    assert_eq!(right.params.get("iss").map(String::as_str), Some(ISSUER));
    right.check_issuer(ISSUER, false).unwrap();
    right.check_issuer(ISSUER, true).unwrap();

    // Another issuer's, or ours with a trailing slash: a mix-up, refused
    // whether or not the issuer promised `iss`.
    for other in [
        "https%3A%2F%2Fevil.example",
        "https%3A%2F%2Fauth.example.com%2F",
    ] {
        let wrong = callback_with(format!("code=c&iss={other}")).await;
        for required in [false, true] {
            assert!(
                matches!(
                    wrong.check_issuer(ISSUER, required),
                    Err(LoopbackError::IssuerMismatch)
                ),
                "{other} required={required}"
            );
        }
    }

    // No `iss`: fine unless the issuer's metadata promises one.
    let none = callback_with("code=c".to_string()).await;
    none.check_issuer(ISSUER, false).unwrap();
    assert!(matches!(
        none.check_issuer(ISSUER, true),
        Err(LoopbackError::IssuerMismatch)
    ));
}

#[tokio::test]
async fn refuses_a_callback_path_it_could_not_match_exactly() {
    for bad in ["", "auth/callback", "/a?b", "/a#b", "/a b", "/a\r\nX: y"] {
        let err = LoopbackListener::bind(LoopbackConfig::new(LoopbackHost::Ipv4Loopback, bad))
            .await
            .err()
            .unwrap_or_else(|| panic!("{bad:?} was accepted"));
        assert!(
            matches!(err, LoopbackError::InvalidPath(_)),
            "{bad:?}: {err:?}"
        );
    }
}

#[tokio::test]
async fn the_callback_does_not_show_its_code_in_debug_output() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let query = format!(
        "code=CODE-MARKER-456&state={}&id_token=IDT-MARKER",
        state.as_str()
    );
    let (result, _) = run(listener, &state, move |port| async move {
        get(callback_url(port, &query)).await
    })
    .await;
    let callback = result.unwrap();
    let debug = format!("{callback:?}");
    assert!(!debug.contains("CODE-MARKER-456"), "{debug}");
    assert!(!debug.contains("IDT-MARKER"), "{debug}");
    assert!(
        debug.contains("id_token"),
        "parameter names stay visible: {debug}"
    );
}

/// What a browser must be able to rely on in either page: a heading, light
/// and dark styling that the CSP lets through, and nothing that loads or runs.
fn assert_styled_static_page(page: &Seen) {
    let body = page.body.to_ascii_lowercase();
    assert!(body.contains("<h1>"), "no heading");
    assert!(body.contains("<style>"), "no inline style");
    assert!(body.contains("prefers-color-scheme: dark"), "no dark mode");
    assert!(body.contains("color-scheme"), "no color-scheme");
    assert!(!body.contains("<script"), "script");
    for external in ["http://", "https://", "src=", "<link", "@import", "url("] {
        assert!(!body.contains(external), "external resource: {external}");
    }
    let csp = page.header("content-security-policy").expect("a CSP");
    assert!(csp.contains("default-src 'none'"), "{csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert!(!csp.contains("script-src"), "{csp}");
    assert!(csp.contains("style-src 'unsafe-inline'"), "{csp}");
}

#[tokio::test]
async fn the_success_page_has_a_heading_and_light_and_dark_styling() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let query = format!("code=c&state={}", state.as_str());
    let (result, page) = run(listener, &state, move |port| async move {
        get(callback_url(port, &query)).await
    })
    .await;
    result.unwrap();
    assert_eq!(page.status, 200);
    assert!(page
        .body
        .contains("<h1>edytlab is connected to ChatGPT</h1>"));
    assert!(page
        .body
        .contains("You can close this tab and return to edytlab"));
    assert_styled_static_page(&page);
}

#[tokio::test]
async fn the_failure_page_is_styled_the_same_way_and_keeps_its_sentence() {
    let state = State::generate().unwrap();
    let listener = LoopbackListener::bind(config()).await.unwrap();
    let query = format!("error=access_denied&state={}", state.as_str());
    let (_, page) = run(listener, &state, move |port| async move {
        get(callback_url(port, &query)).await
    })
    .await;
    assert_eq!(page.status, 400);
    assert!(page.body.contains("Sign-in did not finish"));
    assert!(page
        .body
        .contains("You can close this tab and return to edytlab"));
    assert_styled_static_page(&page);
}
