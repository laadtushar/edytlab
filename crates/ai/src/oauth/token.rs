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
//! code (`invalid_grant` and the like); its text is cut short and stripped of
//! control characters as it is read. Any other failure is its HTTP status
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
    error: String,
    #[serde(default)]
    error_description: Option<String>,
    #[serde(default)]
    error_uri: Option<String>,
}

impl From<RawOAuthErrorBody> for OAuthErrorBody {
    fn from(raw: RawOAuthErrorBody) -> Self {
        Self {
            error: printable(&raw.error, MAX_ERROR_CODE_CHARS),
            error_description: raw
                .error_description
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
    #[serde(default)]
    pub refresh_token: Option<SecretString>,
    #[serde(default)]
    pub id_token: Option<SecretString>,
    /// `Bearer`, in some case: a response with any other type, or none, is
    /// refused (RFC 6749 §5.1 requires it, and RFC 6750 bearer use is all
    /// this client does with a token).
    #[serde(default)]
    pub token_type: String,
    /// Seconds from now, at most [`MAX_EXPIRES_IN`]: a larger value is read
    /// as that.
    #[serde(default, deserialize_with = "capped_expires_in")]
    pub expires_in: Option<u64>,
    /// The granted scopes, space-separated.
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn capped_expires_in<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Ok(Option::<u64>::deserialize(d)?.map(|secs| secs.min(MAX_EXPIRES_IN)))
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
        self.token_request(&form).await
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
        self.token_request(&form).await
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

    async fn token_request(&self, form: &[(&str, &str)]) -> Result<TokenResponse, TokenError> {
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
        if response.access_token.is_empty() {
            return Err(TokenError::InvalidResponse(
                "the access token is empty".into(),
            ));
        }
        if !response.token_type.eq_ignore_ascii_case("bearer") {
            return Err(TokenError::InvalidResponse(format!(
                "the token type is {:?}, not Bearer",
                printable(&response.token_type, 32)
            )));
        }
        Ok(response)
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

/// A non-2xx reply: its RFC 6749 §5.2 body if it has one, else its status.
fn failure(status: u16, body: &[u8]) -> TokenError {
    match serde_json::from_slice::<OAuthErrorBody>(body) {
        Ok(body) if !body.error.is_empty() => TokenError::OAuth { status, body },
        _ => TokenError::Status { status },
    }
}
