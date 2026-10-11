//! ID-token verification (`ai::oauth::oidc`) against a mock issuer.
//!
//! Every token here is signed at test time by an RSA key generated at test
//! time (`oauth_support`), served from a `wiremock` JWKS behind a `wiremock`
//! discovery document. Each check of OIDC Core §3.1.3.7 that the client makes
//! has a token that fails only that check.

mod oauth_support;

use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ai::oauth::oidc::{IdTokenExpectations, OidcClient, OidcError};
use ai::oauth::SecretString;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use oauth_support::{jwt, jwt_raw, TestRsaKey};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CLIENT_ID: &str = "client-123";
const NONCE: &str = "nonce-abcdefghijklmnopqrstuvwxyz0123456789";

fn key() -> &'static TestRsaKey {
    static KEY: OnceLock<TestRsaKey> = OnceLock::new();
    KEY.get_or_init(TestRsaKey::generate)
}

fn other_key() -> &'static TestRsaKey {
    static KEY: OnceLock<TestRsaKey> = OnceLock::new();
    KEY.get_or_init(TestRsaKey::generate)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// A mock issuer whose discovery document names itself and `/jwks`, serving
/// `jwks` there.
async fn issuer_with(jwks: Value) -> MockServer {
    let server = MockServer::start().await;
    mount_discovery(&server, &server.uri()).await;
    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jwks))
        .mount(&server)
        .await;
    server
}

async fn mount_discovery(server: &MockServer, issuer: &str) {
    let uri = server.uri();
    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{uri}/authorize"),
            "token_endpoint": format!("{uri}/token"),
            "jwks_uri": format!("{uri}/jwks"),
            "revocation_endpoint": format!("{uri}/revoke"),
            "id_token_signing_alg_values_supported": ["RS256"],
            "response_types_supported": ["code"],
        })))
        .mount(server)
        .await;
}

fn good_claims(issuer: &str) -> Value {
    let now = now();
    json!({
        "iss": issuer,
        "sub": "user-subject-1",
        "aud": CLIENT_ID,
        "exp": now + 3600,
        "iat": now,
        "nonce": NONCE,
        "email": "person@example.com",
        "email_verified": true,
    })
}

fn rs256_header(kid: &str) -> Value {
    json!({ "alg": "RS256", "typ": "JWT", "kid": kid })
}

fn signed(kid: &str, claims: &Value) -> SecretString {
    SecretString::new(jwt(&rs256_header(kid), claims, |m| key().sign_rs256(m)))
}

fn expectations() -> IdTokenExpectations<'static> {
    IdTokenExpectations {
        client_id: CLIENT_ID,
        nonce: NONCE,
    }
}

async fn client_for(server: &MockServer) -> OidcClient {
    OidcClient::new(&server.uri()).expect("a loopback http issuer is allowed")
}

fn jwks_requests(requests: &[wiremock::Request]) -> usize {
    requests.iter().filter(|r| r.url.path() == "/jwks").count()
}

#[tokio::test]
async fn accepts_a_good_token_and_returns_its_claims() {
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;

    let claims = client
        .verify_id_token(&signed("k1", &good_claims(&server.uri())), &expectations())
        .await
        .expect("a token that passes every check verifies");

    assert_eq!(claims.subject, "user-subject-1");
    assert_eq!(claims.issuer, server.uri());
    assert_eq!(claims.audience, CLIENT_ID);
    assert_eq!(claims.email.as_deref(), Some("person@example.com"));
    assert_eq!(claims.email_verified, Some(true));
    assert_eq!(
        claims.claims.get("sub").and_then(Value::as_str),
        Some("user-subject-1")
    );
    // The email is personal data: not in Debug output.
    let debug = format!("{claims:?}");
    assert!(!debug.contains("person@example.com"), "{debug}");
}

#[tokio::test]
async fn rejects_a_token_from_another_issuer() {
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;
    let mut claims = good_claims(&server.uri());
    // A prefix or a trailing slash is not the same issuer: the match is exact.
    for wrong in [
        "https://evil.example".to_string(),
        format!("{}/", server.uri()),
        format!("{}/tenant", server.uri()),
    ] {
        claims["iss"] = json!(wrong);
        let err = client
            .verify_id_token(&signed("k1", &claims), &expectations())
            .await
            .unwrap_err();
        assert!(matches!(err, OidcError::WrongIssuer), "{wrong}: {err:?}");
    }
}

#[tokio::test]
async fn rejects_a_token_for_another_audience() {
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;
    let mut claims = good_claims(&server.uri());
    for wrong in [
        json!("someone-else"),
        json!([]),
        json!(["someone-else"]),
        // The right client plus an audience it does not trust (§3.1.3.7, 3).
        json!([CLIENT_ID, "someone-else"]),
    ] {
        claims["aud"] = wrong.clone();
        let err = client
            .verify_id_token(&signed("k1", &claims), &expectations())
            .await
            .unwrap_err();
        assert!(matches!(err, OidcError::WrongAudience), "{wrong}: {err:?}");
    }

    // A one-element array naming the client is the same audience.
    claims["aud"] = json!([CLIENT_ID]);
    client
        .verify_id_token(&signed("k1", &claims), &expectations())
        .await
        .expect("[client_id] is the client's audience");

    // An `azp` naming another party is refused.
    claims["azp"] = json!("someone-else");
    let err = client
        .verify_id_token(&signed("k1", &claims), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::WrongAudience), "{err:?}");
}

#[tokio::test]
async fn rejects_an_expired_token_beyond_the_skew() {
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;
    let mut claims = good_claims(&server.uri());
    let exp = now() - 3600;
    claims["exp"] = json!(exp);
    claims["iat"] = json!(exp - 3600);
    let token = signed("k1", &claims);
    let at = |secs: u64| UNIX_EPOCH + Duration::from_secs(secs);

    // Within the 60 s skew it still verifies; past it, it does not.
    client
        .verify_id_token_at(&token, &expectations(), at(exp + 60))
        .await
        .expect("60 s past exp is inside the skew");
    let err = client
        .verify_id_token_at(&token, &expectations(), at(exp + 61))
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::Expired), "{err:?}");
    let err = client
        .verify_id_token(&token, &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::Expired), "{err:?}");
}

#[tokio::test]
async fn rejects_a_token_that_is_not_valid_yet() {
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;

    let mut claims = good_claims(&server.uri());
    claims["nbf"] = json!(now() + 600);
    let err = client
        .verify_id_token(&signed("k1", &claims), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::NotYetValid), "{err:?}");

    let mut claims = good_claims(&server.uri());
    claims["iat"] = json!(now() + 600);
    let err = client
        .verify_id_token(&signed("k1", &claims), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::NotYetValid), "{err:?}");
}

#[tokio::test]
async fn rejects_a_wrong_or_missing_nonce() {
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;

    let mut claims = good_claims(&server.uri());
    claims["nonce"] = json!("another-attempt");
    let err = client
        .verify_id_token(&signed("k1", &claims), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::NonceMismatch), "{err:?}");

    claims.as_object_mut().unwrap().remove("nonce");
    let err = client
        .verify_id_token(&signed("k1", &claims), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::NonceMismatch), "{err:?}");
}

#[tokio::test]
async fn rejects_alg_none() {
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;
    let claims = good_claims(&server.uri());

    for header in [
        json!({ "alg": "none", "kid": "k1" }),
        json!({ "alg": "None", "kid": "k1" }),
    ] {
        // Unsigned: an empty signature part.
        let token = jwt(&header, &claims, |_| Vec::new());
        let err = client
            .verify_id_token(&SecretString::new(token), &expectations())
            .await
            .unwrap_err();
        assert!(
            matches!(err, OidcError::UnsupportedAlgorithm(_)),
            "{header}: {err:?}"
        );
    }
}

#[tokio::test]
async fn rejects_hs256_signed_with_the_public_key() {
    // The classic algorithm confusion: an attacker signs with HMAC, keyed
    // by the issuer's public key, hoping the verifier uses it as a secret.
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;
    let claims = good_claims(&server.uri());
    let mac_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &key().modulus_bytes());
    let token = jwt(&json!({ "alg": "HS256", "kid": "k1" }), &claims, |m| {
        ring::hmac::sign(&mac_key, m).as_ref().to_vec()
    });

    let err = client
        .verify_id_token(&SecretString::new(token), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::UnsupportedAlgorithm(_)), "{err:?}");
}

#[tokio::test]
async fn rejects_other_asymmetric_algorithms() {
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;
    let claims = good_claims(&server.uri());
    for alg in ["ES256", "PS256", "RS384", "RS512", "EdDSA", "rs256"] {
        let token = jwt(&json!({ "alg": alg, "kid": "k1" }), &claims, |m| {
            key().sign_rs256(m)
        });
        let err = client
            .verify_id_token(&SecretString::new(token), &expectations())
            .await
            .unwrap_err();
        assert!(
            matches!(err, OidcError::UnsupportedAlgorithm(_)),
            "{alg}: {err:?}"
        );
    }
}

#[tokio::test]
async fn rejects_a_signature_from_another_key_or_over_other_claims() {
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;
    let claims = good_claims(&server.uri());

    // Signed by a key the issuer does not publish, under its key id.
    let forged = jwt(&rs256_header("k1"), &claims, |m| other_key().sign_rs256(m));
    let err = client
        .verify_id_token(&SecretString::new(forged), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::BadSignature), "{err:?}");

    // A genuine signature moved onto edited claims.
    let genuine = signed("k1", &claims);
    let signature = genuine.expose_secret().rsplit('.').next().unwrap();
    let mut edited = claims.clone();
    edited["sub"] = json!("someone-else");
    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&rs256_header("k1")).unwrap());
    let claims_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&edited).unwrap());
    let spliced = format!("{header_b64}.{claims_b64}.{signature}");
    let err = client
        .verify_id_token(&SecretString::new(spliced), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::BadSignature), "{err:?}");
}

#[tokio::test]
async fn rejects_claims_of_the_wrong_type() {
    // CVE-2026-25537 (jsonwebtoken < 10.3.0) skipped the `exp`/`nbf` checks
    // when the claim had the wrong JSON type. Here a wrong type is an error.
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;
    for (claim, value) in [
        ("exp", json!("99999999999")),
        ("exp", Value::Null),
        ("nbf", json!("0")),
        ("iat", json!(true)),
        ("iss", json!(["x"])),
        ("aud", json!(7)),
        ("nonce", json!(1)),
        ("sub", json!(12)),
    ] {
        let mut claims = good_claims(&server.uri());
        claims[claim] = value.clone();
        let err = client
            .verify_id_token(&signed("k1", &claims), &expectations())
            .await
            .unwrap_err();
        assert!(
            matches!(err, OidcError::InvalidClaims(_)),
            "{claim}={value}: {err:?}"
        );
    }

    let mut claims = good_claims(&server.uri());
    claims.as_object_mut().unwrap().remove("exp");
    let err = client
        .verify_id_token(&signed("k1", &claims), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::InvalidClaims(_)), "{err:?}");
}

#[tokio::test]
async fn rejects_repeated_keys_rather_than_picking_one() {
    // Parsers disagree on which of two repeated keys wins; the verifier
    // refuses the token instead of choosing.
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;
    let now = now();
    let claims = format!(
        r#"{{"iss":"{iss}","iss":"https://evil.example","sub":"u","aud":"{CLIENT_ID}","exp":{exp},"iat":{now},"nonce":"{NONCE}"}}"#,
        iss = server.uri(),
        exp = now + 3600,
    );
    let token = jwt_raw(r#"{"alg":"RS256","kid":"k1"}"#, &claims, |m| {
        key().sign_rs256(m)
    });
    let err = client
        .verify_id_token(&SecretString::new(token), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::InvalidClaims(_)), "{err:?}");

    let good_claims = serde_json::to_string(&good_claims(&server.uri())).unwrap();
    let token = jwt_raw(
        r#"{"alg":"RS256","alg":"none","kid":"k1"}"#,
        &good_claims,
        |m| key().sign_rs256(m),
    );
    let err = client
        .verify_id_token(&SecretString::new(token), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::Malformed(_)), "{err:?}");
}

#[tokio::test]
async fn rejects_a_critical_header_and_malformed_tokens() {
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    let client = client_for(&server).await;
    let claims = good_claims(&server.uri());

    let crit = jwt(
        &json!({ "alg": "RS256", "kid": "k1", "crit": ["exp"] }),
        &claims,
        |m| key().sign_rs256(m),
    );
    let err = client
        .verify_id_token(&SecretString::new(crit), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::UnsupportedHeader), "{err:?}");

    let good = signed("k1", &claims);
    let good = good.expose_secret();
    for malformed in [
        String::new(),
        "a.b".to_string(),
        format!("{good}.extra.parts"),
        format!("{good}="),
        good.replacen('.', ".!", 1),
    ] {
        let err = client
            .verify_id_token(&SecretString::new(malformed.clone()), &expectations())
            .await
            .unwrap_err();
        assert!(matches!(err, OidcError::Malformed(_)), "{err:?}");
    }
}

#[tokio::test]
async fn refetches_the_jwks_once_for_an_unknown_key_id() {
    let server = MockServer::start().await;
    mount_discovery(&server, &server.uri()).await;
    // Before the rotation the set holds k1; after it, k2.
    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "keys": [key().jwk("k1")] })),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "keys": [key().jwk("k2")] })),
        )
        .mount(&server)
        .await;
    let client = client_for(&server)
        .await
        .with_jwks_refetch_interval(Duration::ZERO);
    let claims = good_claims(&server.uri());

    client
        .verify_id_token(&signed("k1", &claims), &expectations())
        .await
        .expect("k1 is in the first set");
    // Cached: no new fetch for a known key.
    client
        .verify_id_token(&signed("k1", &claims), &expectations())
        .await
        .expect("k1 is still cached");
    assert_eq!(jwks_requests(&server.received_requests().await.unwrap()), 1);

    client
        .verify_id_token(&signed("k2", &claims), &expectations())
        .await
        .expect("an unknown kid refetches the set, which now holds k2");
    assert_eq!(jwks_requests(&server.received_requests().await.unwrap()), 2);

    // A kid that is in neither set: one refetch, then a refusal, not a loop.
    let err = client
        .verify_id_token(&signed("k9", &claims), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::UnknownKey), "{err:?}");
    assert_eq!(jwks_requests(&server.received_requests().await.unwrap()), 3);

    // Discovery was read once for all of it.
    let discovery = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/.well-known/openid-configuration")
        .count();
    assert_eq!(discovery, 1);
}

#[tokio::test]
async fn rate_limits_the_jwks_refetch() {
    let server = issuer_with(json!({ "keys": [key().jwk("k1")] })).await;
    // The default interval: a set fetched moments ago is not fetched again.
    let client = client_for(&server).await;
    let claims = good_claims(&server.uri());

    client
        .verify_id_token(&signed("k1", &claims), &expectations())
        .await
        .unwrap();
    for _ in 0..3 {
        let err = client
            .verify_id_token(&signed("unknown", &claims), &expectations())
            .await
            .unwrap_err();
        assert!(matches!(err, OidcError::UnknownKey), "{err:?}");
    }
    assert_eq!(jwks_requests(&server.received_requests().await.unwrap()), 1);
}

#[tokio::test]
async fn skips_keys_it_cannot_use() {
    // An EC key, an encryption key and an RS512 key share the set with the
    // signing key; only the RS256 signing key may verify.
    let mut enc = key().jwk("k1");
    enc["use"] = json!("enc");
    let mut rs512 = key().jwk("k1");
    rs512["alg"] = json!("RS512");
    let server = issuer_with(json!({ "keys": [
        { "kty": "EC", "kid": "k1", "crv": "P-256", "x": "AA", "y": "AA" },
        enc,
        rs512,
    ] }))
    .await;
    let client = client_for(&server).await;
    let err = client
        .verify_id_token(&signed("k1", &good_claims(&server.uri())), &expectations())
        .await
        .unwrap_err();
    assert!(matches!(err, OidcError::UnknownKey), "{err:?}");
}

#[tokio::test]
async fn refuses_a_discovery_document_for_another_issuer() {
    let server = MockServer::start().await;
    mount_discovery(&server, "https://evil.example").await;
    let client = client_for(&server).await;

    let err = client.metadata().await.unwrap_err();
    assert!(matches!(err, OidcError::IssuerMismatch), "{err:?}");
}

#[tokio::test]
async fn reads_the_endpoints_from_discovery() {
    let server = issuer_with(json!({ "keys": [] })).await;
    let client = client_for(&server).await;
    let meta = client.metadata().await.unwrap();

    assert_eq!(meta.issuer, server.uri());
    assert_eq!(
        meta.token_endpoint.as_str(),
        format!("{}/token", server.uri())
    );
    assert_eq!(
        meta.authorization_endpoint.as_str(),
        format!("{}/authorize", server.uri())
    );
    assert_eq!(meta.jwks_uri.as_str(), format!("{}/jwks", server.uri()));
    assert_eq!(
        meta.revocation_endpoint
            .as_ref()
            .map(|u| u.as_str().to_string()),
        Some(format!("{}/revoke", server.uri()))
    );
}

#[test]
fn refuses_a_plain_http_issuer_off_this_machine() {
    let err = OidcClient::new("http://auth.example.com").unwrap_err();
    assert!(matches!(err, OidcError::Endpoint(_)), "{err:?}");
    OidcClient::new("https://auth.example.com").expect("https is allowed");
}
