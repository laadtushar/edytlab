//! PKCE, the authorization URL, the token endpoint and secret redaction
//! (`ai::oauth::{pkce, token}` and `ai::oauth`).
//!
//! The token-endpoint tests run against `wiremock` and read back exactly what
//! the client sent.

use std::collections::BTreeMap;

use ai::oauth::pkce::{Nonce, PkceChallenge, PkceVerifier, State, VerifierError};
use ai::oauth::token::{TokenClient, TokenError, TokenResponse, TokenTypeHint};
use ai::oauth::{
    authorization_url, AuthorizationRequest, AuthorizationUrlError, ParameterError, SecretString,
};
use serde_json::{json, Value};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const UNRESERVED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

// --- PKCE, state and nonce ------------------------------------------------

#[test]
fn s256_challenge_matches_rfc_7636_appendix_b() {
    let verifier = PkceVerifier::new(SecretString::new(
        "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
    ))
    .expect("the RFC's verifier is valid");
    assert_eq!(
        verifier.challenge().as_str(),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    assert_eq!(PkceChallenge::METHOD, "S256");
}

#[test]
fn generated_verifiers_are_long_unreserved_and_fresh() {
    let a = PkceVerifier::generate().unwrap();
    let b = PkceVerifier::generate().unwrap();
    for v in [&a, &b] {
        let s = v.expose_secret();
        assert!((43..=128).contains(&s.len()), "length {}", s.len());
        assert!(s.chars().all(|c| UNRESERVED.contains(c)), "{s}");
        // The challenge is unpadded base64url of a SHA-256 digest.
        assert_eq!(v.challenge().as_str().len(), 43);
    }
    assert_ne!(a.expose_secret(), b.expose_secret());
}

#[test]
fn verifier_rules_follow_rfc_7636() {
    let ok = "a".repeat(43);
    PkceVerifier::new(SecretString::new(ok)).unwrap();
    PkceVerifier::new(SecretString::new("~".repeat(128))).unwrap();
    assert!(matches!(
        PkceVerifier::new(SecretString::new("a".repeat(42))),
        Err(VerifierError::Length(42))
    ));
    assert!(matches!(
        PkceVerifier::new(SecretString::new("a".repeat(129))),
        Err(VerifierError::Length(129))
    ));
    for bad in ['+', '/', '=', ' ', 'é'] {
        let s = format!("{}{bad}", "a".repeat(50));
        assert!(
            matches!(
                PkceVerifier::new(SecretString::new(s)),
                Err(VerifierError::Charset)
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn state_and_nonce_carry_at_least_128_bits() {
    let states: Vec<State> = (0..32).map(|_| State::generate().unwrap()).collect();
    let nonces: Vec<Nonce> = (0..32).map(|_| Nonce::generate().unwrap()).collect();
    for s in states
        .iter()
        .map(State::as_str)
        .chain(nonces.iter().map(Nonce::as_str))
    {
        // base64url carries 6 bits a character: 128 bits need 22 characters.
        assert!(s.len() >= 22, "{s}");
        assert!(s.chars().all(|c| UNRESERVED.contains(c)), "{s}");
    }
    let mut all: Vec<&str> = states.iter().map(State::as_str).collect();
    all.extend(nonces.iter().map(Nonce::as_str));
    let distinct: std::collections::BTreeSet<&str> = all.iter().copied().collect();
    assert_eq!(distinct.len(), all.len());
}

#[test]
fn state_matches_only_itself() {
    let state = State::generate().unwrap();
    assert!(state.matches(state.as_str()));
    assert!(!state.matches(""));
    assert!(!state.matches(&state.as_str()[1..]));
    assert!(!state.matches(&format!("{}x", state.as_str())));
    let mut flipped = state.as_str().to_string();
    let last = flipped.pop().unwrap();
    flipped.push(if last == 'A' { 'B' } else { 'A' });
    assert!(!state.matches(&flipped));
}

// --- The authorization URL ------------------------------------------------

fn query_of(url: &Url) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, v) in url.query_pairs() {
        map.entry(k.into_owned()).or_default().push(v.into_owned());
    }
    map
}

#[test]
fn authorization_url_carries_the_code_flow_parameters() {
    let verifier = PkceVerifier::generate().unwrap();
    let challenge = verifier.challenge();
    let state = State::generate().unwrap();
    let nonce = Nonce::generate().unwrap();
    let endpoint = Url::parse("https://auth.example.com/authorize?tenant=t1").unwrap();
    let url = authorization_url(
        &endpoint,
        &AuthorizationRequest {
            client_id: "client 1",
            redirect_uri: "http://127.0.0.1:1455/auth/callback",
            scope: "openid profile",
            state: &state,
            nonce: Some(&nonce),
            code_challenge: &challenge,
        },
        &[
            ("resource", "https://api.example.com/v1"),
            ("login_hint", "a@b.c"),
        ],
    )
    .unwrap();

    assert_eq!(url.origin(), endpoint.origin());
    assert_eq!(url.path(), "/authorize");
    let q = query_of(&url);
    let one = |k: &str| -> &str {
        let v = &q[k];
        assert_eq!(v.len(), 1, "{k} appears once");
        &v[0]
    };
    assert_eq!(one("tenant"), "t1");
    assert_eq!(one("response_type"), "code");
    assert_eq!(one("client_id"), "client 1");
    assert_eq!(one("redirect_uri"), "http://127.0.0.1:1455/auth/callback");
    assert_eq!(one("scope"), "openid profile");
    assert_eq!(one("state"), state.as_str());
    assert_eq!(one("nonce"), nonce.as_str());
    assert_eq!(one("code_challenge"), challenge.as_str());
    assert_eq!(one("code_challenge_method"), "S256");
    assert_eq!(one("resource"), "https://api.example.com/v1");
    assert_eq!(one("login_hint"), "a@b.c");
    // The verifier itself never goes in the URL.
    assert!(!url.as_str().contains(verifier.expose_secret()));
}

#[test]
fn authorization_url_refuses_extras_that_override_its_own_parameters() {
    let challenge = PkceVerifier::generate().unwrap().challenge();
    let state = State::generate().unwrap();
    let request = AuthorizationRequest {
        client_id: "c",
        redirect_uri: "http://127.0.0.1:1455/cb",
        scope: "openid",
        state: &state,
        nonce: None,
        code_challenge: &challenge,
    };
    let endpoint = Url::parse("https://auth.example.com/authorize").unwrap();
    for name in [
        "state",
        "redirect_uri",
        "code_challenge",
        "code_challenge_method",
        "response_type",
        "client_id",
        "scope",
        "nonce",
    ] {
        let err = authorization_url(&endpoint, &request, &[(name, "x")]).unwrap_err();
        assert!(
            matches!(&err, AuthorizationUrlError::Parameter(ParameterError::Reserved(n)) if n == name),
            "{name}: {err:?}"
        );
    }
    let err =
        authorization_url(&endpoint, &request, &[("prompt", "a"), ("prompt", "b")]).unwrap_err();
    assert!(
        matches!(
            err,
            AuthorizationUrlError::Parameter(ParameterError::Duplicate(_))
        ),
        "{err:?}"
    );
    // An endpoint whose own query already sets a reserved name is refused too.
    let rigged =
        Url::parse("https://auth.example.com/authorize?redirect_uri=https://evil").unwrap();
    let err = authorization_url(&rigged, &request, &[]).unwrap_err();
    assert!(
        matches!(
            err,
            AuthorizationUrlError::Parameter(ParameterError::Reserved(_))
        ),
        "{err:?}"
    );
}

#[test]
fn authorization_url_refuses_an_endpoint_that_is_not_https() {
    // The URL goes to the OS opener: only https, or plain http on a loopback
    // address (tests), may reach it.
    let challenge = PkceVerifier::generate().unwrap().challenge();
    let state = State::generate().unwrap();
    let request = AuthorizationRequest {
        client_id: "c",
        redirect_uri: "http://127.0.0.1:1455/cb",
        scope: "openid",
        state: &state,
        nonce: None,
        code_challenge: &challenge,
    };
    for bad in [
        "http://auth.example.com/authorize",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "ftp://auth.example.com/authorize",
        "https://user:pass@auth.example.com/authorize",
        "https://auth.example.com/authorize#frag",
    ] {
        let endpoint = Url::parse(bad).unwrap();
        let result = authorization_url(&endpoint, &request, &[]);
        assert!(
            matches!(result, Err(AuthorizationUrlError::Endpoint(_))),
            "{bad}: {result:?}"
        );
    }
    for ok in [
        "https://auth.example.com/authorize",
        "http://127.0.0.1:8080/authorize",
    ] {
        authorization_url(&Url::parse(ok).unwrap(), &request, &[]).unwrap();
    }
}

// --- The token endpoint ---------------------------------------------------

fn form_of(req: &wiremock::Request) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, v) in url::form_urlencoded::parse(&req.body) {
        map.entry(k.into_owned()).or_default().push(v.into_owned());
    }
    map
}

fn single(form: &BTreeMap<String, Vec<String>>, key: &str) -> String {
    let v = form.get(key).unwrap_or_else(|| panic!("{key} missing"));
    assert_eq!(v.len(), 1, "{key} appears once");
    v[0].clone()
}

fn assert_form_post(req: &wiremock::Request) {
    assert_eq!(req.method.as_str(), "POST");
    let ct = req
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        ct.starts_with("application/x-www-form-urlencoded"),
        "content-type {ct}"
    );
    // A public client: no secret, in the body or as Basic auth.
    assert!(req.headers.get("authorization").is_none());
}

fn token_body() -> serde_json::Value {
    json!({
        "access_token": "AT-secret-value",
        "refresh_token": "RT-secret-value",
        "id_token": "IDT-secret-value",
        "token_type": "Bearer",
        "expires_in": 3600,
        "scope": "openid profile offline_access",
        "earliest_refresh_at": 1790034332,
    })
}

async fn token_server(status: u16, body: serde_json::Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&server)
        .await;
    server
}

fn token_client(server: &MockServer) -> TokenClient {
    TokenClient::new(
        Url::parse(&format!("{}/token", server.uri())).unwrap(),
        "client-123",
    )
    .unwrap()
}

#[tokio::test]
async fn exchanges_a_code_with_the_verifier_and_reads_a_typed_response() {
    let server = token_server(200, token_body()).await;
    let verifier = PkceVerifier::generate().unwrap();

    let tokens = token_client(&server)
        .exchange_code(
            &SecretString::new("the-code"),
            &verifier,
            "http://127.0.0.1:1455/auth/callback",
            &[("resource", "https://api.example.com/v1")],
        )
        .await
        .expect("200 with a token body");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_form_post(&requests[0]);
    let form = form_of(&requests[0]);
    assert_eq!(single(&form, "grant_type"), "authorization_code");
    assert_eq!(single(&form, "client_id"), "client-123");
    assert_eq!(single(&form, "code"), "the-code");
    assert_eq!(single(&form, "code_verifier"), verifier.expose_secret());
    assert_eq!(
        single(&form, "redirect_uri"),
        "http://127.0.0.1:1455/auth/callback"
    );
    assert_eq!(single(&form, "resource"), "https://api.example.com/v1");
    assert_eq!(form.len(), 6, "nothing else is sent: {form:?}");

    assert_eq!(tokens.access_token.expose_secret(), "AT-secret-value");
    assert_eq!(
        tokens
            .refresh_token
            .as_ref()
            .map(SecretString::expose_secret),
        Some("RT-secret-value")
    );
    assert_eq!(
        tokens.id_token.as_ref().map(SecretString::expose_secret),
        Some("IDT-secret-value")
    );
    assert_eq!(tokens.token_type, "Bearer");
    assert_eq!(tokens.expires_in, Some(3600));
    assert_eq!(tokens.scopes(), vec!["openid", "profile", "offline_access"]);
    assert!(tokens.has_scope("offline_access"));
    assert!(!tokens.has_scope("offline"));
    assert_eq!(
        tokens.extra.get("earliest_refresh_at"),
        Some(&json!(1790034332))
    );
}

#[tokio::test]
async fn refreshes_with_the_refresh_token_and_no_scope() {
    let server = token_server(200, token_body()).await;

    let tokens = token_client(&server)
        .refresh(
            &SecretString::new("RT-old"),
            &[("resource", "https://api.example.com/v1")],
        )
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_form_post(&requests[0]);
    let form = form_of(&requests[0]);
    assert_eq!(single(&form, "grant_type"), "refresh_token");
    assert_eq!(single(&form, "client_id"), "client-123");
    assert_eq!(single(&form, "refresh_token"), "RT-old");
    assert_eq!(single(&form, "resource"), "https://api.example.com/v1");
    assert_eq!(form.len(), 4, "no scope, no secret: {form:?}");
    assert_eq!(
        tokens
            .refresh_token
            .as_ref()
            .map(SecretString::expose_secret),
        Some("RT-secret-value")
    );
}

#[tokio::test]
async fn revokes_with_a_type_hint_and_the_client_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/revoke"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let revoke = Url::parse(&format!("{}/revoke", server.uri())).unwrap();

    token_client(&server)
        .revoke(
            &revoke,
            &SecretString::new("RT-1"),
            TokenTypeHint::RefreshToken,
        )
        .await
        .expect("an empty 200 is success");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_form_post(&requests[0]);
    let form = form_of(&requests[0]);
    assert_eq!(single(&form, "token"), "RT-1");
    assert_eq!(single(&form, "token_type_hint"), "refresh_token");
    assert_eq!(single(&form, "client_id"), "client-123");
    assert_eq!(form.len(), 3);
}

#[tokio::test]
async fn an_oauth_error_body_is_typed() {
    let server = token_server(
        400,
        json!({ "error": "invalid_grant", "error_description": "code expired" }),
    )
    .await;

    let err = token_client(&server)
        .refresh(&SecretString::new("RT-old"), &[])
        .await
        .unwrap_err();
    match &err {
        TokenError::OAuth { status, body } => {
            assert_eq!(*status, 400);
            assert_eq!(body.error, "invalid_grant");
            assert_eq!(body.error_description.as_deref(), Some("code expired"));
        }
        other => panic!("expected an OAuth error, got {other:?}"),
    }
    assert_eq!(
        err.oauth_error().map(|b| b.error.as_str()),
        Some("invalid_grant")
    );
}

#[tokio::test]
async fn an_oauth_error_body_is_cut_short_and_printable() {
    // The endpoint's text ends up in error messages and logs: no terminal
    // escapes or line breaks, and no more than a few hundred characters.
    let server = token_server(
        400,
        json!({
            "error": format!("invalid_grant\u{1b}[2J\r\n{}", "e".repeat(500)),
            "error_description": format!("line one\nline two\u{7}{}", "d".repeat(10_000)),
            "error_uri": format!("https://example.com/\r\nX-Injected: 1{}", "u".repeat(10_000)),
        }),
    )
    .await;
    let err = token_client(&server)
        .refresh(&SecretString::new("RT-old"), &[])
        .await
        .unwrap_err();
    let body = err.oauth_error().expect("an RFC 6749 error body");
    assert!(body.error.starts_with("invalid_grant"), "{:?}", body.error);
    let description = body.error_description.as_deref().unwrap();
    let uri = body.error_uri.as_deref().unwrap();
    for (field, value, max) in [
        ("error", body.error.as_str(), 64),
        ("error_description", description, 300),
        ("error_uri", uri, 300),
    ] {
        assert!(
            value.chars().count() <= max,
            "{field}: {} characters",
            value.chars().count()
        );
        assert!(!value.chars().any(char::is_control), "{field}: {value:?}");
    }
    assert!(!err.to_string().chars().any(char::is_control), "{err}");
}

#[tokio::test]
async fn the_token_type_must_be_bearer() {
    // RFC 6749 §5.1 requires `token_type`, and this client sends the token
    // as a bearer (RFC 6750), whose name is case-insensitive.
    for token_type in ["Bearer", "bearer", "BEARER"] {
        let mut body = token_body();
        body["token_type"] = json!(token_type);
        let server = token_server(200, body).await;
        token_client(&server)
            .refresh(&SecretString::new("RT"), &[])
            .await
            .unwrap_or_else(|e| panic!("{token_type}: {e:?}"));
    }
    for token_type in [
        json!("DPoP"),
        json!("mac"),
        json!(""),
        json!("Bearer "),
        Value::Null,
    ] {
        let mut body = token_body();
        if token_type.is_null() {
            body.as_object_mut().unwrap().remove("token_type");
        } else {
            body["token_type"] = token_type.clone();
        }
        let server = token_server(200, body).await;
        let err = token_client(&server)
            .refresh(&SecretString::new("RT"), &[])
            .await
            .unwrap_err();
        assert!(
            matches!(err, TokenError::InvalidResponse(_)),
            "{token_type}: {err:?}"
        );
    }
}

#[tokio::test]
async fn expires_in_is_capped_at_a_day() {
    // `Instant::now() + Duration::from_secs(u64::MAX)` panics: a hostile or
    // broken endpoint must not be able to make the caller's expiry overflow.
    for (sent, kept) in [
        (json!(3600), 3600),
        (json!(86_400), 86_400),
        (json!(86_401), 86_400),
        (json!(u64::MAX), 86_400),
    ] {
        let mut body = token_body();
        body["expires_in"] = sent.clone();
        let server = token_server(200, body).await;
        let tokens = token_client(&server)
            .refresh(&SecretString::new("RT"), &[])
            .await
            .unwrap();
        assert_eq!(tokens.expires_in, Some(kept), "{sent}");
    }
}

#[tokio::test]
async fn a_revoke_error_body_is_typed_too() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/revoke"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({ "error": "unsupported_token_type" })),
        )
        .mount(&server)
        .await;
    let revoke = Url::parse(&format!("{}/revoke", server.uri())).unwrap();
    let err = token_client(&server)
        .revoke(
            &revoke,
            &SecretString::new("RT-1"),
            TokenTypeHint::RefreshToken,
        )
        .await
        .unwrap_err();
    assert_eq!(
        err.oauth_error().map(|b| b.error.as_str()),
        Some("unsupported_token_type")
    );
}

#[tokio::test]
async fn a_non_json_failure_is_its_status_alone() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(503).set_body_string("<html>busy</html>"))
        .mount(&server)
        .await;
    let err = token_client(&server)
        .refresh(&SecretString::new("RT"), &[])
        .await
        .unwrap_err();
    assert!(matches!(err, TokenError::Status { status: 503 }), "{err:?}");
}

#[tokio::test]
async fn a_success_without_an_access_token_is_an_error() {
    for body in [
        json!({ "token_type": "Bearer" }),
        json!({ "access_token": "" }),
    ] {
        let server = token_server(200, body).await;
        let err = token_client(&server)
            .refresh(&SecretString::new("RT"), &[])
            .await
            .unwrap_err();
        assert!(matches!(err, TokenError::InvalidResponse(_)), "{err:?}");
    }
}

#[tokio::test]
async fn an_unreadable_response_does_not_echo_its_contents() {
    // A token where a number belongs: serde's message would quote it.
    let server = token_server(
        200,
        json!({ "access_token": "AT-secret-value", "expires_in": "AT-secret-value" }),
    )
    .await;
    let err = token_client(&server)
        .refresh(&SecretString::new("RT"), &[])
        .await
        .unwrap_err();
    assert!(matches!(err, TokenError::InvalidResponse(_)), "{err:?}");
    assert!(!err.to_string().contains("AT-secret-value"), "{err}");
    assert!(!format!("{err:?}").contains("AT-secret-value"), "{err:?}");
}

#[tokio::test]
async fn does_not_follow_a_redirect_with_the_form() {
    // A 307 would resend the body, refresh token included, to the new place.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/elsewhere", server.uri())),
        )
        .mount(&server)
        .await;
    let err = token_client(&server)
        .refresh(&SecretString::new("RT"), &[])
        .await
        .unwrap_err();
    assert!(matches!(err, TokenError::Status { status: 307 }), "{err:?}");
    let requests = server.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| r.url.path() == "/token"));
}

#[tokio::test]
async fn refuses_an_oversized_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(2 * 1024 * 1024)))
        .mount(&server)
        .await;
    let err = token_client(&server)
        .refresh(&SecretString::new("RT"), &[])
        .await
        .unwrap_err();
    assert!(matches!(err, TokenError::TooLarge(_)), "{err:?}");
}

/// A one-shot HTTP server that answers any request with `body` in chunks of
/// 4 KiB, `Transfer-Encoding: chunked` and no `Content-Length`, so the client
/// cannot tell the size from the headers.
async fn chunked_server(body: Vec<u8>) -> Url {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        // Read the request: its head, then the form (Content-Length bytes).
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        let body_len = loop {
            let n = stream.read(&mut buf).await.unwrap();
            assert!(n > 0, "the client closed before sending its request");
            request.extend_from_slice(&buf[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                let length: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .map(|v| v.trim().parse().unwrap())
                    .unwrap_or(0);
                break end + 4 + length;
            }
        };
        while request.len() < body_len {
            let n = stream.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
        }
        let head = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                    Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
        if stream.write_all(head.as_bytes()).await.is_err() {
            return;
        }
        for chunk in body.chunks(4096) {
            let mut framed = format!("{:x}\r\n", chunk.len()).into_bytes();
            framed.extend_from_slice(chunk);
            framed.extend_from_slice(b"\r\n");
            // The client hangs up once it has read too much.
            if stream.write_all(&framed).await.is_err() {
                return;
            }
        }
        let _ = stream.write_all(b"0\r\n\r\n").await;
        let _ = stream.shutdown().await;
    });
    Url::parse(&format!("http://127.0.0.1:{port}/token")).unwrap()
}

/// A token response of exactly `len` bytes.
fn token_json_of_len(len: usize) -> Vec<u8> {
    let frame = r#"{"access_token":"AT","token_type":"Bearer","pad":""}"#;
    let pad = "p".repeat(len - frame.len());
    let body = format!(r#"{{"access_token":"AT","token_type":"Bearer","pad":"{pad}"}}"#);
    assert_eq!(body.len(), len);
    body.into_bytes()
}

#[tokio::test]
async fn refuses_an_oversized_chunked_response() {
    // With no Content-Length, only the count kept while reading can stop it.
    const LIMIT: usize = 64 * 1024;
    let at_limit = TokenClient::new(chunked_server(token_json_of_len(LIMIT)).await, "c")
        .unwrap()
        .refresh(&SecretString::new("RT"), &[])
        .await;
    assert!(at_limit.is_ok(), "{at_limit:?}");

    let over = TokenClient::new(chunked_server(token_json_of_len(LIMIT + 1)).await, "c")
        .unwrap()
        .refresh(&SecretString::new("RT"), &[])
        .await
        .unwrap_err();
    assert!(matches!(over, TokenError::TooLarge(_)), "{over:?}");

    let far_over = TokenClient::new(chunked_server(vec![b' '; 2 * 1024 * 1024]).await, "c")
        .unwrap()
        .refresh(&SecretString::new("RT"), &[])
        .await
        .unwrap_err();
    assert!(matches!(far_over, TokenError::TooLarge(_)), "{far_over:?}");
}

#[tokio::test]
async fn refuses_extras_that_override_its_own_parameters() {
    let server = token_server(200, token_body()).await;
    let client = token_client(&server);
    let verifier = PkceVerifier::generate().unwrap();
    for name in [
        "grant_type",
        "client_id",
        "client_secret",
        "code",
        "code_verifier",
        "redirect_uri",
        "refresh_token",
    ] {
        let err = client
            .exchange_code(
                &SecretString::new("c"),
                &verifier,
                "http://127.0.0.1/cb",
                &[(name, "x")],
            )
            .await
            .unwrap_err();
        assert!(
            matches!(&err, TokenError::Parameter(ParameterError::Reserved(n)) if n == name),
            "{name}: {err:?}"
        );
        let err = client
            .refresh(&SecretString::new("RT"), &[(name, "x")])
            .await
            .unwrap_err();
        assert!(
            matches!(&err, TokenError::Parameter(ParameterError::Reserved(_))),
            "{name}: {err:?}"
        );
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn refuses_a_plain_http_endpoint_off_this_machine() {
    for bad in [
        "http://auth.example.com/token",
        "https://user:pass@auth.example.com/token",
        "ftp://auth.example.com/token",
    ] {
        let err = TokenClient::new(Url::parse(bad).unwrap(), "c").unwrap_err();
        assert!(matches!(err, TokenError::Endpoint(_)), "{bad}: {err:?}");
    }
    for ok in [
        "https://auth.example.com/token",
        "http://127.0.0.1:8080/token",
        "http://localhost:8080/token",
        "http://[::1]:8080/token",
    ] {
        TokenClient::new(Url::parse(ok).unwrap(), "c").unwrap();
    }
}

// --- Secrets stay out of Debug --------------------------------------------

/// Fails to compile if `$ty` implements any of the traits.
///
/// `Probe<M>` is implemented for every type with `M = ()`, and once more,
/// with a marker type of its own, for every type that implements each trait.
/// `<$ty as Probe<_>>` leaves `M` to inference: with one impl it is `()`;
/// with two the compiler cannot choose and stops (E0283, "type annotations
/// needed"). This is the trick `static_assertions::assert_not_impl_any!`
/// uses. Unlike a `compile_fail` doc test, it cannot pass because of some
/// other error.
macro_rules! assert_not_impl {
    ($ty:ty: $($trait:path),+ $(,)?) => {
        const _: fn() = || {
            trait Probe<M> {
                fn probe() {}
            }
            impl<T: ?Sized> Probe<()> for T {}
            $({
                #[allow(dead_code)]
                struct Implements;
                impl<T: ?Sized + $trait> Probe<Implements> for T {}
            })+
            let _ = <$ty as Probe<_>>::probe;
        };
    };
}

#[test]
fn secrets_cannot_be_displayed_serialized_or_compared() {
    // Checked when this file compiles. No `Display` and no `Serialize`, so a
    // secret cannot reach a log line, an error message or an IPC reply by
    // accident; no `PartialEq`, whose `==` stops at the first differing
    // byte (compare with `constant_time_eq` or `State::matches`).
    use ai::oauth::loopback::Callback;
    use ai::oauth::oidc::{IdTokenClaims, IdTokenExpectations};
    use std::fmt::Display;
    assert_not_impl!(SecretString: Display, serde::Serialize, PartialEq);
    assert_not_impl!(PkceVerifier: Display, serde::Serialize, PartialEq);
    assert_not_impl!(State: Display, serde::Serialize, PartialEq);
    assert_not_impl!(Nonce: Display, serde::Serialize, PartialEq);
    assert_not_impl!(TokenResponse: Display, serde::Serialize, PartialEq);
    assert_not_impl!(Callback: Display, serde::Serialize, PartialEq);
    // The ID token's claims hold the email address, and the expectations
    // the nonce.
    assert_not_impl!(IdTokenClaims: Display, serde::Serialize, PartialEq);
    assert_not_impl!(IdTokenExpectations<'static>: Display, serde::Serialize, PartialEq);
}

#[test]
fn secrets_are_redacted_in_debug_output() {
    let secret = SecretString::new("super-secret-token-value");
    let debug = format!("{secret:?}");
    assert!(!debug.contains("super-secret-token-value"), "{debug}");
    assert!(debug.contains("redacted"), "{debug}");
    assert!(!format!("{:#?}", Some(&secret)).contains("super-secret"));

    let verifier = PkceVerifier::generate().unwrap();
    assert!(!format!("{verifier:?}").contains(verifier.expose_secret()));
    let state = State::generate().unwrap();
    assert!(!format!("{state:?}").contains(state.as_str()));
    let nonce = Nonce::generate().unwrap();
    assert!(!format!("{nonce:?}").contains(nonce.as_str()));

    let response: TokenResponse = serde_json::from_value(json!({
        "access_token": "AT-secret-value",
        "refresh_token": "RT-secret-value",
        "id_token": "IDT-secret-value",
        "token_type": "Bearer",
        "expires_in": 3600,
        "scope": "openid",
        "earliest_refresh_at": 1790034332,
        "unexpected_secret": "XT-secret-value",
    }))
    .unwrap();
    for debug in [format!("{response:?}"), format!("{response:#?}")] {
        for value in [
            "AT-secret-value",
            "RT-secret-value",
            "IDT-secret-value",
            "XT-secret-value",
        ] {
            assert!(!debug.contains(value), "{value} in {debug}");
        }
        // Field names stay visible, so the output is still useful.
        assert!(debug.contains("earliest_refresh_at"), "{debug}");
        assert!(debug.contains("expires_in"), "{debug}");
    }
}
