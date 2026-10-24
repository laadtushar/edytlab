//! The token endpoint: the authorization-code exchange (RFC 6749 §4.1.3,
//! with the PKCE verifier of RFC 7636 §4.5), refresh (RFC 6749 §6) and
//! revocation (RFC 7009).
//!
//! The client is public: it identifies itself with `client_id` in the form
//! and never sends a secret. Every call is a form-encoded `POST`. The
//! endpoint URLs come from the caller or from discovery
//! ([`super::oidc::ProviderMetadata`]); none is built in here.
//!
//! Responses are typed. A success body without an `access_token` is an
//! error, and so is one that does not parse; that error says where parsing
//! failed but never quotes the body, which holds tokens. A success must also
//! name its `token_type` as `Bearer` (in any case), the only kind this client
//! knows how to send, and an `expires_in` above [`MAX_EXPIRES_IN`] is read as
//! that ([`TokenResponse::expires_in`]). A failure with an RFC 6749 §5.2
//! error body is [`TokenError::OAuth`], so a caller can act on its `error`
//! code (`invalid_grant` and the like), whether the server sends it as the
//! RFC's string or wrapped in an object; its text is cut short and stripped
//! of control characters as it is read. Any other failure is its HTTP status
//! alone.

use std::fmt;

use serde::Deserialize;
use serde_json::{Map, Value};
use url::Url;

use super::pkce::PkceVerifier;
use super::{
    check_endpoint, check_extra_params, http_client, printable, read_limited, BodyError,
    EndpointError, ParameterError, SecretString, MAX_DESCRIPTION_CHARS, MAX_ERROR_CODE_CHARS,
};

/// A token endpoint's reply is read up to this size.
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// The shortest `expires_in` believed, in seconds. See [`capped_expires_in`].
pub const MIN_EXPIRES_IN: u64 = 60;

/// The longest `expires_in` kept, in seconds: one day. Access tokens live
/// for minutes or hours; a larger value is read as this, so a caller's
/// `Instant::now() + Duration::from_secs(expires_in)` cannot overflow (with
/// `u64::MAX` it panics).
pub const MAX_EXPIRES_IN: u64 = 24 * 60 * 60;

/// The parameters the client sets itself, which an extra may not repeat.
const TOKEN_RESERVED: &[&str] = &[
    "grant_type",
    "client_id",
    "client_secret",
    "client_assertion",
    "client_assertion_type",
    "code",
    "code_verifier",
    "redirect_uri",
    "refresh_token",
];

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("the token endpoint is not usable: {0}")]
    Endpoint(#[from] EndpointError),
    #[error("the client id is empty")]
    EmptyClientId,
    #[error(transparent)]
    Parameter(#[from] ParameterError),
    #[error("could not set up the HTTP client: {0}")]
    Client(#[source] reqwest::Error),
    #[error("could not reach the token endpoint: {0}")]
    Network(#[source] reqwest::Error),
    #[error("the token endpoint sent more than {0} bytes")]
    TooLarge(usize),
    #[error("the token endpoint's reply could not be read: {0}")]
    InvalidResponse(String),
    #[error("the token endpoint refused the request (HTTP {status}): {}", .body.error)]
    OAuth { status: u16, body: OAuthErrorBody },
    #[error("the token endpoint answered HTTP {status}")]
    Status { status: u16 },
}

impl TokenError {
    /// The RFC 6749 §5.2 error body, when the endpoint sent one.
    pub fn oauth_error(&self) -> Option<&OAuthErrorBody> {
        match self {
            Self::OAuth { body, .. } => Some(body),
            _ => None,
        }
    }
}

/// An RFC 6749 §5.2 error response.
///
/// RFC 6749 puts the code in `error` as a string. Some issuers wrap it in an
/// object instead, `{"error": {"code": "...", "message": "..."}}`. Codex
/// CLI, a client of the same OpenAI auth server the ChatGPT session uses,
/// reads its refresh errors in both shapes. Both are read into this one
/// type, the object's `message` standing in for a missing
/// `error_description`.
///
/// The server's text ends up in error messages and perhaps logs, so each
/// field is cut to a few hundred characters (the code to 64) and has its
/// control characters replaced by spaces as it is read: no terminal escapes,
/// no forged log lines.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(from = "RawOAuthErrorBody")]
pub struct OAuthErrorBody {
    /// The error code, such as `invalid_grant`.
    pub error: String,
    pub error_description: Option<String>,
    pub error_uri: Option<String>,
}

#[derive(Deserialize)]
struct RawOAuthErrorBody {
    error: RawErrorField,
    #[serde(default)]
    error_description: Option<String>,
    #[serde(default)]
    error_uri: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawErrorField {
    /// RFC 6749 §5.2.
    Code(String),
    /// The wrapped form.
    Object {
        code: String,
        #[serde(default)]
        message: Option<String>,
    },
}

impl From<RawOAuthErrorBody> for OAuthErrorBody {
    fn from(raw: RawOAuthErrorBody) -> Self {
        let (code, message) = match raw.error {
            RawErrorField::Code(code) => (code, None),
            RawErrorField::Object { code, message } => (code, message),
        };
        Self {
            error: printable(&code, MAX_ERROR_CODE_CHARS),
            error_description: raw
                .error_description
                .or(message)
                .map(|d| printable(&d, MAX_DESCRIPTION_CHARS)),
            error_uri: raw.error_uri.map(|u| printable(&u, MAX_DESCRIPTION_CHARS)),
        }
    }
}

/// A successful token response (RFC 6749 §5.1), with any fields beyond the
/// standard ones (such as `earliest_refresh_at`) kept in [`Self::extra`].
#[derive(Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: SecretString,
    #[serde(default, deserialize_with = "lenient")]
    pub refresh_token: Option<SecretString>,
    #[serde(default, deserialize_with = "lenient")]
    pub id_token: Option<SecretString>,
    /// `Bearer`, in some case: a response with any other type, or none, is
    /// refused (RFC 6749 §5.1 requires it, and RFC 6750 bearer use is all
    /// this client does with a token).
    #[serde(default, deserialize_with = "lenient")]
    pub token_type: String,
    /// Seconds from now, at least [`MIN_EXPIRES_IN`] (a smaller one is read
    /// as absent) and at most [`MAX_EXPIRES_IN`]: a larger value is read as
    /// that.
    #[serde(default, deserialize_with = "capped_expires_in")]
    pub expires_in: Option<u64>,
    /// The granted scopes, space-separated.
    #[serde(default, deserialize_with = "lenient")]
    pub scope: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `expires_in` as an integer, a float, a numeric string or null; anything
/// else is "not given". A success response is parsed **leniently**: the
/// server has already rotated the refresh token by the time it answers, so a
/// malformed optional field that failed the whole response would lose a token
/// that cannot be had again (the next attempt spends the old one and the
/// session ends). A field that is wrong is read as absent, and the caller
/// falls back to the documented default.
fn capped_expires_in<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    let value = Value::deserialize(d)?;
    // Under a minute is a server saying nothing useful, not a token that
    // lives that long: "0", `0` and `0.4` would have the caller refresh on
    // every request. Absent, the caller uses the documented lifetime.
    Ok(seconds_of(&value)
        .filter(|secs| *secs >= MIN_EXPIRES_IN)
        .map(|secs| secs.min(MAX_EXPIRES_IN)))
}

/// A non-negative number of seconds in a JSON value: an integer, a float
/// (truncated), or a string holding either. Not a negative, not `NaN`, not
/// infinite.
pub fn seconds_of(value: &Value) -> Option<u64> {
    let secs = match value {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    (secs.is_finite() && secs >= 0.0).then_some(secs.min(u64::MAX as f64) as u64)
}

/// The field if it parses as `T`, else `T::default()`: a field of the wrong
/// type is read as absent. See [`capped_expires_in`] for why.
fn lenient<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = Value::deserialize(d)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

impl TokenResponse {
    /// The granted scopes.
    pub fn scopes(&self) -> Vec<&str> {
        self.scope
            .as_deref()
            .map(|s| s.split_ascii_whitespace().collect())
            .unwrap_or_default()
    }

    /// Whether `scope` was granted.
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes().contains(&scope)
    }
}

impl fmt::Debug for TokenResponse {
    // Tokens are redacted by `SecretString`. Extra fields show their names
    // only: an unknown field could hold another credential.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &self.access_token)
            .field("refresh_token", &self.refresh_token)
            .field("id_token", &self.id_token)
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .field("extra", &self.extra.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// RFC 7009 §2.1 `token_type_hint`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenTypeHint {
    RefreshToken,
    AccessToken,
}

impl TokenTypeHint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RefreshToken => "refresh_token",
            Self::AccessToken => "access_token",
        }
    }
}

/// A public client of one token endpoint.
pub struct TokenClient {
    http: reqwest::Client,
    token_endpoint: Url,
    client_id: String,
}

impl fmt::Debug for TokenClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenClient")
            .field("token_endpoint", &self.token_endpoint.as_str())
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

impl TokenClient {
    /// `token_endpoint` must be `https` (or `http` on a loopback address).
    pub fn new(token_endpoint: Url, client_id: impl Into<String>) -> Result<Self, TokenError> {
        check_endpoint(&token_endpoint)?;
        let client_id = client_id.into();
        if client_id.is_empty() {
            return Err(TokenError::EmptyClientId);
        }
        Ok(Self {
            http: http_client().map_err(TokenError::Client)?,
            token_endpoint,
            client_id,
        })
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Exchanges an authorization code. `redirect_uri` must be exactly the
    /// one sent in the authorization request. `extra` adds parameters such
    /// as `resource`; it may not set any the client sets.
    pub async fn exchange_code(
        &self,
        code: &SecretString,
        verifier: &PkceVerifier,
        redirect_uri: &str,
        extra: &[(&str, &str)],
    ) -> Result<TokenResponse, TokenError> {
        check_extra_params(extra, TOKEN_RESERVED)?;
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("client_id", self.client_id.as_str()),
            ("code", code.expose_secret()),
            ("code_verifier", verifier.expose_secret()),
            ("redirect_uri", redirect_uri),
        ];
        form.extend_from_slice(extra);
        self.token_request(&form, true).await
    }

    /// Uses a refresh token. No `scope` is sent, so the grant is kept as it
    /// was (RFC 6749 §6). A rotating server returns a new refresh token in
    /// the response; the caller must keep it in place of the old one.
    pub async fn refresh(
        &self,
        refresh_token: &SecretString,
        extra: &[(&str, &str)],
    ) -> Result<TokenResponse, TokenError> {
        check_extra_params(extra, TOKEN_RESERVED)?;
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("client_id", self.client_id.as_str()),
            ("refresh_token", refresh_token.expose_secret()),
        ];
        form.extend_from_slice(extra);
        // The server has rotated the refresh token by the time it answers:
        // see `validate` for why a missing `token_type` does not fail this.
        self.token_request(&form, false).await
    }

    /// Revokes `token` at `revocation_endpoint` (RFC 7009 §2.1). Any 2xx is
    /// success; RFC 7009 answers 200 for a token that was already invalid.
    pub async fn revoke(
        &self,
        revocation_endpoint: &Url,
        token: &SecretString,
        hint: TokenTypeHint,
    ) -> Result<(), TokenError> {
        check_endpoint(revocation_endpoint)?;
        let form = [
            ("token", token.expose_secret()),
            ("token_type_hint", hint.as_str()),
            ("client_id", self.client_id.as_str()),
        ];
        let (status, body) = self.post(revocation_endpoint, &form).await?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(failure(status, &body))
        }
    }

    async fn token_request(
        &self,
        form: &[(&str, &str)],
        require_token_type: bool,
    ) -> Result<TokenResponse, TokenError> {
        let (status, body) = self.post(&self.token_endpoint, form).await?;
        if !(200..300).contains(&status) {
            return Err(failure(status, &body));
        }
        let response: TokenResponse = serde_json::from_slice(&body).map_err(|e| {
            // serde's message can quote the offending value, which may be a
            // token: keep only the category and the position.
            TokenError::InvalidResponse(format!(
                "{:?} error at line {}, column {}",
                e.classify(),
                e.line(),
                e.column()
            ))
        })?;
        validate(response, require_token_type)
    }

    async fn post(&self, url: &Url, form: &[(&str, &str)]) -> Result<(u16, Vec<u8>), TokenError> {
        let response = self
            .http
            .post(url.clone())
            .header(reqwest::header::ACCEPT, "application/json")
            .form(form)
            .send()
            .await
            .map_err(TokenError::Network)?;
        let status = response.status().as_u16();
        let body = read_limited(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|e| match e {
                BodyError::TooLarge => TokenError::TooLarge(MAX_RESPONSE_BYTES),
                BodyError::Network(e) => TokenError::Network(e),
            })?;
        Ok((status, body))
    }
}

/// The checks a parsed success response must pass.
///
/// `require_token_type` is whether the `token_type` field must be there:
/// RFC 6749 §5.1 requires it, so a code exchange does. A refresh response is
/// the case where it is not worth failing for: the server has already rotated
/// the refresh token by the time it answers, so refusing the response loses a
/// token that cannot be had again, and a missing or unreadable type can only
/// mean the one kind this client uses. A type that is there and is not Bearer
/// is refused either way.
fn validate(
    response: TokenResponse,
    require_token_type: bool,
) -> Result<TokenResponse, TokenError> {
    if response.access_token.is_empty() {
        return Err(TokenError::InvalidResponse(
            "the access token is empty".into(),
        ));
    }
    let typeless = response.token_type.is_empty() && !require_token_type;
    if !typeless && !response.token_type.eq_ignore_ascii_case("bearer") {
        return Err(TokenError::InvalidResponse(format!(
            "the token type is {:?}, not Bearer",
            printable(&response.token_type, 32)
        )));
    }
    Ok(response)
}

/// A non-2xx reply: its RFC 6749 §5.2 body (in either shape
/// [`OAuthErrorBody`] reads) if it has one, else its status.
fn failure(status: u16, body: &[u8]) -> TokenError {
    match serde_json::from_slice::<OAuthErrorBody>(body) {
        Ok(body) if !body.error.is_empty() => TokenError::OAuth { status, body },
        _ => TokenError::Status { status },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(extra: Value) -> TokenResponse {
        let mut body =
            json!({ "access_token": "a", "token_type": "Bearer", "refresh_token": "rt" });
        for (k, v) in extra.as_object().unwrap() {
            body[k] = v.clone();
        }
        serde_json::from_value(body).expect("a success response always parses")
    }

    /// `expires_in` as the integer, float, numeric string, null, or something
    /// else a server writes. None of it fails the response, which would lose
    /// the rotated refresh token that came with it.
    #[test]
    fn expires_in_is_read_leniently_and_never_fails_the_response() {
        for (given, wanted) in [
            (json!(3600), Some(3600)),
            (json!(3600.9), Some(3600)),
            (json!("3600"), Some(3600)),
            (json!(" 1800.0 "), Some(1800)),
            (json!(1e12), Some(MAX_EXPIRES_IN)),
            // Under a minute is not a lifetime: it would refresh on every
            // request.
            (json!(0), None),
            (json!("0"), None),
            (json!(0.4), None),
            (json!(59), None),
            (json!(60), Some(60)),
            (json!(null), None),
            (json!("an hour"), None),
            (json!(-1), None),
            (json!("-1"), None),
            (json!({ "secs": 1 }), None),
            (json!([3600]), None),
            (json!(true), None),
        ] {
            let response = parse(json!({ "expires_in": given }));
            assert_eq!(response.expires_in, wanted, "{given}");
            assert_eq!(
                response.refresh_token.as_ref().map(|t| t.expose_secret()),
                Some("rt"),
                "{given}: the rotated token is still there"
            );
        }
        assert_eq!(parse(json!({})).expires_in, None, "absent");
    }

    /// Fields of the wrong type are absent, not an error, and the response
    /// then goes through the same checks a received one does: a refresh
    /// accepts one with no usable `token_type` (the rotated refresh token
    /// must not be lost), a code exchange does not.
    #[test]
    fn the_other_optional_fields_are_lenient_too() {
        let response = parse(json!({ "scope": 5, "id_token": ["x"], "token_type": null }));
        assert_eq!(response.scope, None);
        assert!(response.id_token.is_none());
        assert_eq!(response.token_type, "");
        assert_eq!(response.access_token.expose_secret(), "a");

        let kept = validate(response.clone(), false).expect("a refresh keeps it");
        assert_eq!(
            kept.refresh_token.as_ref().map(|t| t.expose_secret()),
            Some("rt"),
            "the rotated refresh token is still there"
        );
        assert!(
            validate(response, true).is_err(),
            "a code exchange needs the type"
        );
        // A type that is there and is not Bearer is refused by both.
        let mac = parse(json!({ "token_type": "mac" }));
        assert!(validate(mac.clone(), false).is_err());
        assert!(validate(mac, true).is_err());
    }
}
