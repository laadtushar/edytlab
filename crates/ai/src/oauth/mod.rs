//! A generic OAuth 2.0 and OpenID Connect client for a native app (#474).
//!
//! It signs a user in through their system browser, the way RFC 8252 tells a
//! native app to: the authorization-code flow with PKCE (RFC 7636), for a
//! public client that has no secret, redirected to a short-lived listener on
//! the loopback interface (RFC 8252 §7.3), then the ID token checked (OpenID
//! Connect Core §3.1.3.7) and the code exchanged, refreshed and revoked at the
//! token endpoint (RFC 6749 §4.1.3 and §6, RFC 7009).
//!
//! Nothing here is specific to one provider. The issuer, endpoints, client id,
//! scopes, callback host and path, and any extra parameters (such as
//! `resource`) come from the caller or from the issuer's discovery document.
//! A provider's own rules (its registration flow, which scopes it needs, how
//! it classifies a failed refresh) live with that provider, on top of this.
//!
//! - [`pkce`]: the code verifier and its S256 challenge, `state` and `nonce`.
//! - [`loopback`]: the redirect listener.
//! - [`oidc`]: discovery, the JWKS and ID-token verification (RS256 only).
//! - [`token`]: the code exchange, refresh and revocation.
//!
//! **Secrets.** Every token, code and verifier is a [`SecretString`]: its
//! `Debug` prints `<redacted>`, and it has no `Display` and no `Serialize`, so
//! it cannot reach a log line, an error message or an IPC reply by accident.
//! Reading one takes an explicit [`SecretString::expose_secret`]. Nothing in
//! this module logs a token, a code or an email address. Text a server sends
//! back (an OAuth `error`, `error_description` or `error_uri`) is cut short
//! and has its control characters replaced before it reaches an error value.
//!
//! **HTTP.** Each client makes its own `reqwest` client that never follows a
//! redirect (a 307 from a token endpoint would resend the refresh token to
//! wherever it points), times out, and caps the size of what it reads. An
//! endpoint must be `https`, or plain `http` on a loopback address for tests.
//! The authorization URL handed to the browser is held to the same rule.
//!
//! **For the provider built on this (#474, PR 3).**
//!
//! - Verify the ID token of the code exchange with
//!   [`oidc::IdTokenExpectations::sign_in`] and the attempt's
//!   [`pkce::Nonce`], and an ID token in a refresh response with
//!   [`oidc::IdTokenExpectations::refresh`] and the claims of the original
//!   (OpenID Connect Core §12.2: no new nonce, same `iss`, `sub` and `aud`).
//! - Call [`loopback::Callback::check_issuer`] before exchanging the code
//!   (RFC 9207, mix-up attacks). It compares `Callback::params["iss"]` with
//!   the issuer whenever the response carries one, and refuses a response
//!   without one when `required`, which the caller sets from
//!   [`oidc::ProviderMetadata::authorization_response_iss_parameter_supported`].
//! - Use the port the listener reports, never the preferred one: see
//!   "Ports" in [`loopback`].

pub mod loopback;
pub mod oidc;
pub mod pkce;
pub mod token;

use std::collections::BTreeSet;
use std::fmt;
use std::net::IpAddr;
use std::time::Duration;

use serde::Deserialize;
use url::Url;

use pkce::{Nonce, PkceChallenge, State};

/// A secret string: an access, refresh or ID token, an authorization code
/// or a PKCE verifier.
///
/// `Debug` shows `SecretString(<redacted>)`. There is deliberately no
/// `Display` and no `Serialize`, and no `PartialEq` either, whose `==` stops
/// at the first byte that differs: compare secrets with
/// [`pkce::constant_time_eq`]. `tests/oauth_client.rs`
/// (`secrets_cannot_be_displayed_serialized_or_compared`) fails to compile if
/// this type, or any other that holds a secret, gains one of the three.
#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret itself. Call it only where the value is sent or stored.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(<redacted>)")
    }
}

/// An endpoint URL this client will not send anything to.
#[derive(Debug, thiserror::Error)]
pub enum EndpointError {
    #[error("not a valid URL: {0}")]
    Invalid(#[from] url::ParseError),
    #[error("{0} must use https (plain http is allowed only on a loopback address)")]
    NotHttps(String),
    #[error("{0} must not carry a user name or password")]
    Credentials(String),
    #[error("{0} must not have a fragment")]
    Fragment(String),
}

/// Checks that `url` is somewhere this client may send a token, code or
/// verifier: `https`, or `http` to a loopback address (tests, and a local
/// issuer), with no credentials or fragment in it.
pub(crate) fn check_endpoint(url: &Url) -> Result<(), EndpointError> {
    let shown = || format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));
    match url.scheme() {
        "https" => {}
        "http" if is_loopback_host(url) => {}
        _ => return Err(EndpointError::NotHttps(shown())),
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(EndpointError::NotHttps(shown()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(EndpointError::Credentials(shown()));
    }
    if url.fragment().is_some() {
        return Err(EndpointError::Fragment(shown()));
    }
    Ok(())
}

pub(crate) fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

/// An OAuth `error` code from a server is kept to this many characters.
pub(crate) const MAX_ERROR_CODE_CHARS: usize = 64;
/// An `error_description` or `error_uri` from a server is kept to this many
/// characters.
pub(crate) const MAX_DESCRIPTION_CHARS: usize = 300;

/// `s` cut to `max` characters, control characters replaced by spaces: what
/// is left of a server's text once it is fit for an error message or a log.
pub(crate) fn printable(s: &str, max: usize) -> String {
    s.chars()
        .take(max)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// How long one HTTP call to an issuer may take, connection included.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// The `reqwest` client every OAuth call uses: no redirects, a timeout.
pub(crate) fn http_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(HTTP_TIMEOUT)
        .build()
}

/// Why a body could not be read.
#[derive(Debug)]
pub(crate) enum BodyError {
    TooLarge,
    Network(reqwest::Error),
}

/// Reads a response body, refusing one larger than `limit` bytes.
pub(crate) async fn read_limited(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, BodyError> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(BodyError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(BodyError::Network)? {
        if body.len() + chunk.len() > limit {
            return Err(BodyError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// A request parameter that cannot be sent.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParameterError {
    /// The caller's extra parameter would replace one the client sets.
    #[error("the parameter {0:?} is set by the OAuth client and cannot be passed as an extra")]
    Reserved(String),
    /// The same extra parameter twice (RFC 6749 §3.1 forbids repeats).
    #[error("the parameter {0:?} is given more than once")]
    Duplicate(String),
}

/// Checks that `extra` repeats nothing and sets none of `reserved`.
pub(crate) fn check_extra_params(
    extra: &[(&str, &str)],
    reserved: &[&str],
) -> Result<(), ParameterError> {
    let mut seen = BTreeSet::new();
    for (name, _) in extra {
        if reserved.contains(name) {
            return Err(ParameterError::Reserved((*name).to_string()));
        }
        if !seen.insert(*name) {
            return Err(ParameterError::Duplicate((*name).to_string()));
        }
    }
    Ok(())
}

/// The standard parameters of an authorization request (RFC 6749 §4.1.1,
/// RFC 7636 §4.3, OpenID Connect Core §3.1.2.1).
#[derive(Debug)]
pub struct AuthorizationRequest<'a> {
    pub client_id: &'a str,
    /// Exactly as it will be sent to the token endpoint later.
    pub redirect_uri: &'a str,
    /// Space-separated.
    pub scope: &'a str,
    pub state: &'a State,
    /// Set for an OpenID Connect sign-in, whose ID token must carry it back.
    pub nonce: Option<&'a Nonce>,
    pub code_challenge: &'a PkceChallenge,
}

const AUTHORIZE_RESERVED: &[&str] = &[
    "response_type",
    "client_id",
    "redirect_uri",
    "scope",
    "state",
    "nonce",
    "code_challenge",
    "code_challenge_method",
];

/// Why [`authorization_url`] would not build a URL.
#[derive(Debug, thiserror::Error)]
pub enum AuthorizationUrlError {
    /// The endpoint is not one to hand to the browser.
    #[error("the authorization endpoint is not usable: {0}")]
    Endpoint(#[from] EndpointError),
    #[error(transparent)]
    Parameter(#[from] ParameterError),
}

/// The URL to open in the browser: `endpoint` with the request's parameters
/// and then `extra`, all percent-encoded. `response_type` is `code` and
/// `code_challenge_method` is `S256`.
///
/// `endpoint` must pass the same check as every other endpoint (`https`, or
/// plain `http` on a loopback address, with no credentials or fragment), so
/// only such a URL reaches the OS opener. An extra parameter, or a query
/// parameter already on `endpoint`, that would set one of the standard ones
/// is refused, as is a repeated extra.
pub fn authorization_url(
    endpoint: &Url,
    request: &AuthorizationRequest<'_>,
    extra: &[(&str, &str)],
) -> Result<Url, AuthorizationUrlError> {
    check_endpoint(endpoint)?;
    check_extra_params(extra, AUTHORIZE_RESERVED)?;
    for (name, _) in endpoint.query_pairs() {
        if AUTHORIZE_RESERVED.contains(&name.as_ref()) {
            return Err(ParameterError::Reserved(name.into_owned()).into());
        }
        if extra.iter().any(|(n, _)| *n == name) {
            return Err(ParameterError::Duplicate(name.into_owned()).into());
        }
    }
    let mut url = endpoint.clone();
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", request.client_id)
            .append_pair("redirect_uri", request.redirect_uri)
            .append_pair("scope", request.scope)
            .append_pair("state", request.state.as_str());
        if let Some(nonce) = request.nonce {
            q.append_pair("nonce", nonce.as_str());
        }
        q.append_pair("code_challenge", request.code_challenge.as_str())
            .append_pair("code_challenge_method", PkceChallenge::METHOD);
        for (name, value) in extra {
            q.append_pair(name, value);
        }
    }
    Ok(url)
}
