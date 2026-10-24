//! OpenID Connect: the issuer's discovery document, its signing keys (JWKS)
//! and ID-token verification.
//!
//! Verification follows OpenID Connect Core §3.1.3.7, narrowed:
//!
//! - **RS256 only.** The header's `alg` must be exactly `RS256`; `none`,
//!   every HMAC algorithm, ECDSA, RSA-PSS and anything else are refused
//!   before a key is looked up. Only RSA keys published for signing (`use`
//!   absent or `sig`, `alg` absent or `RS256`) are candidates, and the check
//!   is `ring`'s RSASSA-PKCS1-v1_5 with SHA-256 over 2048- to 8192-bit
//!   moduli, so a key under 2048 bits never verifies. A header with a
//!   `crit` member is refused, whatever its value (`null` included); `jku`,
//!   `jwk`, `x5u` and the like are never used, so a token cannot name its own
//!   key.
//! - **Claims are typed strictly**, and only read after the signature
//!   verifies. A missing `iss`, `sub`, `aud`, `exp` or `iat`, or any claim
//!   of the wrong JSON type (such as `exp` as a string), is an error rather
//!   than a skipped check.
//! - `iss` must equal the configured issuer exactly. `aud` must be exactly
//!   the client id (a string, or an array holding only it), and `azp`, if
//!   present, the client id too. `exp`, `nbf` and `iat` are checked with
//!   [`CLOCK_SKEW`].
//! - **Two kinds of ID token** ([`IdTokenExpectations`]). The one from a
//!   sign-in's code exchange must carry the attempt's `nonce`, compared in
//!   constant time; the expectation takes a [`Nonce`], which only
//!   [`Nonce::generate`] makes, so it can never be empty. One from a refresh
//!   response (OpenID Connect Core §12.2) needs no nonce, but its `iss`,
//!   `sub` and `aud` must equal the original's, as must its `azp` (present
//!   or absent alike), a `nonce` if it has one, and an `auth_time` if both
//!   have one.
//!
//! The discovery document is fetched once and cached; its `issuer` must
//! equal the configured issuer (OpenID Connect Discovery §4.3). The endpoints
//! it names must be `https`, unless the issuer itself is on a loopback
//! address (tests), when plain `http` to a loopback address is allowed too:
//! an issuer reached over `https` cannot send the client to a local port. The
//! JWKS is
//! cached for an hour. A token whose `kid` is not in the cache makes one
//! refetch, unless the set was fetched less than
//! [`DEFAULT_JWKS_REFETCH_INTERVAL`] ago, so a stream of bad tokens cannot
//! make the client hammer the issuer.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use tokio::sync::{Mutex, OnceCell};
use url::Url;

use super::pkce::{constant_time_eq, Nonce};
use super::{
    check_endpoint, http_client, is_loopback_host, read_limited, BodyError, EndpointError,
    SecretString,
};

/// Leeway for `exp`, `nbf` and `iat` against this machine's clock.
pub const CLOCK_SKEW: Duration = Duration::from_secs(60);
/// The least time between two JWKS fetches made for an unknown `kid`.
pub const DEFAULT_JWKS_REFETCH_INTERVAL: Duration = Duration::from_secs(60);
/// A cached JWKS older than this is fetched again before use.
const JWKS_MAX_AGE: Duration = Duration::from_secs(60 * 60);
const MAX_DISCOVERY_BYTES: usize = 64 * 1024;
const MAX_JWKS_BYTES: usize = 256 * 1024;
const MAX_ID_TOKEN_BYTES: usize = 16 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    #[error("the issuer URL is not usable: {0}")]
    Endpoint(#[from] EndpointError),
    #[error("the issuer URL must not have a query")]
    InvalidIssuer,
    #[error("could not set up the HTTP client: {0}")]
    Client(#[source] reqwest::Error),
    #[error("could not reach the issuer: {0}")]
    Network(#[source] reqwest::Error),
    #[error("{url} answered HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("{0} sent more than this client reads")]
    TooLarge(String),
    #[error("the issuer's discovery document is invalid: {0}")]
    InvalidDiscovery(String),
    #[error("the issuer's discovery document names a different issuer")]
    IssuerMismatch,
    #[error("the issuer's JWKS is invalid")]
    InvalidJwks,
    #[error("the ID token is malformed: {0}")]
    Malformed(&'static str),
    #[error("the ID token is signed with {0:?}; only RS256 is accepted")]
    UnsupportedAlgorithm(String),
    #[error("the ID token has a critical header this client does not support")]
    UnsupportedHeader,
    #[error("no signing key the issuer publishes matches the ID token")]
    UnknownKey,
    #[error("the ID token's signature is invalid")]
    BadSignature,
    #[error("the ID token's claims are invalid: {0}")]
    InvalidClaims(&'static str),
    #[error("the ID token was issued by a different issuer")]
    WrongIssuer,
    #[error("the ID token was issued for a different client")]
    WrongAudience,
    #[error("the ID token has expired")]
    Expired,
    #[error("the ID token is not valid yet")]
    NotYetValid,
    #[error("the ID token does not belong to this sign-in (nonce mismatch)")]
    NonceMismatch,
    /// A refreshed ID token names another account than the original.
    #[error("the refreshed ID token is for a different account than the original sign-in")]
    SubjectChanged,
}

/// The parts of an issuer's discovery document this client uses.
#[derive(Clone, Debug)]
pub struct ProviderMetadata {
    pub issuer: String,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
    pub jwks_uri: Url,
    pub revocation_endpoint: Option<Url>,
    pub id_token_signing_alg_values_supported: Vec<String>,
    /// RFC 9207: the issuer promises an `iss` parameter in every
    /// authorization response, so one without it is to be refused
    /// ([`super::loopback::Callback::check_issuer`]). False when absent.
    pub authorization_response_iss_parameter_supported: bool,
    /// The whole document, the fields above included.
    pub document: Map<String, Value>,
}

#[derive(Deserialize)]
struct RawMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    revocation_endpoint: Option<String>,
    #[serde(default)]
    id_token_signing_alg_values_supported: Vec<String>,
    #[serde(default)]
    authorization_response_iss_parameter_supported: bool,
}

/// What a verified ID token must have been issued for.
#[derive(Clone, Copy, Debug)]
pub enum IdTokenExpectations<'a> {
    /// The ID token from a sign-in's code exchange (OpenID Connect Core
    /// §3.1.3.7): its `aud` (and `azp`, if present) is `client_id`, and its
    /// `nonce` is the one sent in this attempt's authorization request.
    SignIn {
        client_id: &'a str,
        nonce: &'a Nonce,
    },
    /// An ID token from a refresh response (OpenID Connect Core §12.2): its
    /// `aud` (and `azp`, if present) is `client_id`, it needs no `nonce`, and
    /// its `iss`, `sub`, `aud` and `azp` are those of `original`, the claims
    /// of the ID token verified at sign-in. A `nonce`, if present, must be
    /// the original's, and so must an `auth_time` if both have one.
    Refresh {
        client_id: &'a str,
        original: &'a IdTokenClaims,
    },
}

impl<'a> IdTokenExpectations<'a> {
    /// For the ID token of a sign-in's code exchange.
    pub fn sign_in(client_id: &'a str, nonce: &'a Nonce) -> Self {
        Self::SignIn { client_id, nonce }
    }

    /// For the ID token of a refresh response, checked against `original`.
    pub fn refresh(client_id: &'a str, original: &'a IdTokenClaims) -> Self {
        Self::Refresh {
            client_id,
            original,
        }
    }

    fn client_id(&self) -> &'a str {
        match self {
            Self::SignIn { client_id, .. } | Self::Refresh { client_id, .. } => client_id,
        }
    }
}

/// The claims of a verified ID token.
#[derive(Clone)]
pub struct IdTokenClaims {
    pub issuer: String,
    /// The account's stable identifier at this issuer.
    pub subject: String,
    pub audience: String,
    /// Unix seconds.
    pub expires_at: u64,
    /// Unix seconds.
    pub issued_at: u64,
    /// Personal data: keep it out of logs.
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    /// Every claim in the token, the ones above included.
    pub claims: Map<String, Value>,
}

impl fmt::Debug for IdTokenClaims {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdTokenClaims")
            .field("issuer", &self.issuer)
            .field("subject", &self.subject)
            .field("audience", &self.audience)
            .field("expires_at", &self.expires_at)
            .field("issued_at", &self.issued_at)
            .field("email", &self.email.as_ref().map(|_| "<redacted>"))
            .field("email_verified", &self.email_verified)
            .field("claims", &self.claims.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Clone)]
struct RsaKey {
    kid: Option<String>,
    n: Vec<u8>,
    e: Vec<u8>,
}

#[derive(Default)]
struct JwksCache {
    keys: Vec<RsaKey>,
    fetched_at: Option<Instant>,
}

impl JwksCache {
    fn find(&self, kid: Option<&str>) -> Option<RsaKey> {
        match kid {
            Some(kid) => self
                .keys
                .iter()
                .find(|k| k.kid.as_deref() == Some(kid))
                .cloned(),
            // No `kid`: usable only when the issuer publishes one key.
            None => match self.keys.as_slice() {
                [only] => Some(only.clone()),
                _ => None,
            },
        }
    }
}

/// A client for one OpenID Connect issuer.
pub struct OidcClient {
    http: reqwest::Client,
    issuer: String,
    issuer_url: Url,
    discovery_url: Url,
    metadata: OnceCell<Arc<ProviderMetadata>>,
    jwks: Mutex<JwksCache>,
    jwks_refetch_interval: Duration,
}

impl fmt::Debug for OidcClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OidcClient")
            .field("issuer", &self.issuer)
            .finish_non_exhaustive()
    }
}

impl OidcClient {
    /// A client for `issuer`, such as `https://auth.example.com`. It must be
    /// `https` (or `http` on a loopback address). Nothing is fetched yet.
    pub fn new(issuer: &str) -> Result<Self, OidcError> {
        let parsed = Url::parse(issuer).map_err(EndpointError::from)?;
        check_endpoint(&parsed)?;
        if parsed.query().is_some() {
            return Err(OidcError::InvalidIssuer);
        }
        // OpenID Connect Discovery §4: drop a trailing '/', then append.
        let discovery_url = Url::parse(&format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        ))
        .map_err(EndpointError::from)?;
        Ok(Self {
            http: http_client().map_err(OidcError::Client)?,
            issuer: issuer.to_string(),
            issuer_url: parsed,
            discovery_url,
            metadata: OnceCell::new(),
            jwks: Mutex::new(JwksCache::default()),
            jwks_refetch_interval: DEFAULT_JWKS_REFETCH_INTERVAL,
        })
    }

    /// Sets the least time between JWKS refetches for an unknown `kid`.
    pub fn with_jwks_refetch_interval(mut self, interval: Duration) -> Self {
        self.jwks_refetch_interval = interval;
        self
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// The discovery document, fetched on first use and then cached.
    pub async fn metadata(&self) -> Result<Arc<ProviderMetadata>, OidcError> {
        self.metadata
            .get_or_try_init(|| self.fetch_metadata())
            .await
            .cloned()
    }

    async fn fetch_metadata(&self) -> Result<Arc<ProviderMetadata>, OidcError> {
        let body = self.get(&self.discovery_url, MAX_DISCOVERY_BYTES).await?;
        let document: Map<String, Value> = serde_json::from_slice(&body)
            .map_err(|_| OidcError::InvalidDiscovery("not a JSON object".into()))?;
        let raw: RawMetadata = serde_json::from_value(Value::Object(document.clone()))
            .map_err(|e| OidcError::InvalidDiscovery(e.to_string()))?;
        if raw.issuer != self.issuer {
            return Err(OidcError::IssuerMismatch);
        }
        let endpoint = |name: &str, value: &str| discovered_endpoint(&self.issuer_url, name, value);
        Ok(Arc::new(ProviderMetadata {
            authorization_endpoint: endpoint(
                "authorization_endpoint",
                &raw.authorization_endpoint,
            )?,
            token_endpoint: endpoint("token_endpoint", &raw.token_endpoint)?,
            jwks_uri: endpoint("jwks_uri", &raw.jwks_uri)?,
            revocation_endpoint: raw
                .revocation_endpoint
                .as_deref()
                .map(|v| endpoint("revocation_endpoint", v))
                .transpose()?,
            id_token_signing_alg_values_supported: raw.id_token_signing_alg_values_supported,
            authorization_response_iss_parameter_supported: raw
                .authorization_response_iss_parameter_supported,
            issuer: raw.issuer,
            document,
        }))
    }

    async fn get(&self, url: &Url, limit: usize) -> Result<Vec<u8>, OidcError> {
        let response = self
            .http
            .get(url.clone())
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(OidcError::Network)?;
        let status = response.status();
        if !status.is_success() {
            return Err(OidcError::Status {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }
        read_limited(response, limit).await.map_err(|e| match e {
            BodyError::TooLarge => OidcError::TooLarge(url.to_string()),
            BodyError::Network(e) => OidcError::Network(e),
        })
    }

    async fn refresh_jwks(&self, cache: &mut JwksCache) -> Result<(), OidcError> {
        let jwks_uri = self.metadata().await?.jwks_uri.clone();
        let body = self.get(&jwks_uri, MAX_JWKS_BYTES).await?;
        #[derive(Deserialize)]
        struct RawJwks {
            keys: Vec<Value>,
        }
        let raw: RawJwks = serde_json::from_slice(&body).map_err(|_| OidcError::InvalidJwks)?;
        cache.keys = raw.keys.into_iter().filter_map(usable_rsa_key).collect();
        cache.fetched_at = Some(Instant::now());
        Ok(())
    }

    async fn key_for(&self, kid: Option<&str>) -> Result<RsaKey, OidcError> {
        let mut cache = self.jwks.lock().await;
        let stale = cache
            .fetched_at
            .is_none_or(|at| at.elapsed() >= JWKS_MAX_AGE);
        if stale {
            self.refresh_jwks(&mut cache).await?;
        }
        if let Some(key) = cache.find(kid) {
            return Ok(key);
        }
        // An unknown key may be a rotation: refetch once, unless the set
        // was just fetched.
        let fresh = cache
            .fetched_at
            .is_some_and(|at| at.elapsed() < self.jwks_refetch_interval);
        if stale || fresh {
            return Err(OidcError::UnknownKey);
        }
        self.refresh_jwks(&mut cache).await?;
        cache.find(kid).ok_or(OidcError::UnknownKey)
    }

    /// Verifies `id_token` now. See the module documentation for the checks.
    pub async fn verify_id_token(
        &self,
        id_token: &SecretString,
        expected: &IdTokenExpectations<'_>,
    ) -> Result<IdTokenClaims, OidcError> {
        self.verify_id_token_at(id_token, expected, SystemTime::now())
            .await
    }

    /// [`Self::verify_id_token`] with the clock read as `now`.
    pub async fn verify_id_token_at(
        &self,
        id_token: &SecretString,
        expected: &IdTokenExpectations<'_>,
        now: SystemTime,
    ) -> Result<IdTokenClaims, OidcError> {
        let jws = split_compact_jws(id_token.expose_secret())?;
        let header: Header = serde_json::from_slice(&jws.header)
            .map_err(|_| OidcError::Malformed("the header is not a valid JOSE header"))?;
        if header.alg != "RS256" {
            return Err(OidcError::UnsupportedAlgorithm(
                header
                    .alg
                    .chars()
                    .take(32)
                    .filter(|c| !c.is_control())
                    .collect(),
            ));
        }
        if header.crit {
            return Err(OidcError::UnsupportedHeader);
        }
        let key = self.key_for(header.kid.as_deref()).await?;
        ring::signature::RsaPublicKeyComponents {
            n: key.n.as_slice(),
            e: key.e.as_slice(),
        }
        .verify(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            jws.signing_input.as_bytes(),
            &jws.signature,
        )
        .map_err(|_| OidcError::BadSignature)?;
        check_claims(&jws.payload, &self.issuer, expected, now)
    }
}

/// An endpoint URL from `issuer`'s discovery document, checked like any
/// endpoint. Plain `http` (which [`check_endpoint`] allows only to a
/// loopback address) is allowed only when the issuer itself is on a loopback
/// address: an issuer reached over `https` cannot send a token, a code or a
/// verifier to a local port.
fn discovered_endpoint(issuer: &Url, name: &str, value: &str) -> Result<Url, OidcError> {
    let invalid = |why: &dyn fmt::Display| OidcError::InvalidDiscovery(format!("{name}: {why}"));
    let url = Url::parse(value).map_err(|e| invalid(&e))?;
    check_endpoint(&url).map_err(|e| invalid(&e))?;
    if url.scheme() != "https" && !is_loopback_host(issuer) {
        return Err(invalid(&"plain http is allowed only for a loopback issuer"));
    }
    Ok(url)
}

/// The JOSE header fields this client reads. Others (`typ`, `jku`, `jwk`,
/// `x5u`, ...) are ignored, never followed.
#[derive(Deserialize)]
struct Header {
    alg: String,
    #[serde(default)]
    kid: Option<String>,
    /// Whether the header has a `crit` member at all. Its value is not
    /// read: `"crit": null`, which RFC 7515 §4.1.11 does not allow, is
    /// refused like any other.
    #[serde(default, deserialize_with = "present")]
    crit: bool,
}

/// True for a member that is there, whatever its value (`null` included).
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    serde::de::IgnoredAny::deserialize(d)?;
    Ok(true)
}

struct CompactJws<'a> {
    header: Vec<u8>,
    payload: Vec<u8>,
    signature: Vec<u8>,
    signing_input: &'a str,
}

/// Splits `header.payload.signature` and decodes each part as unpadded,
/// canonical base64url (RFC 7515 §7.1). A JWE (five parts) is malformed here.
fn split_compact_jws(token: &str) -> Result<CompactJws<'_>, OidcError> {
    if token.len() > MAX_ID_TOKEN_BYTES {
        return Err(OidcError::Malformed("too long"));
    }
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(OidcError::Malformed("not three dot-separated parts"));
    };
    if header.is_empty() || payload.is_empty() {
        return Err(OidcError::Malformed("an empty part"));
    }
    let decode = |part: &str| {
        URL_SAFE_NO_PAD
            .decode(part)
            .map_err(|_| OidcError::Malformed("a part is not base64url"))
    };
    Ok(CompactJws {
        header: decode(header)?,
        payload: decode(payload)?,
        signature: decode(signature)?,
        signing_input: &token[..header.len() + 1 + payload.len()],
    })
}

/// A JWK, if it is an RSA key published for RS256 signatures.
fn usable_rsa_key(jwk: Value) -> Option<RsaKey> {
    #[derive(Deserialize)]
    struct RawJwk {
        kty: String,
        #[serde(default)]
        kid: Option<String>,
        #[serde(default, rename = "use")]
        use_: Option<String>,
        #[serde(default)]
        alg: Option<String>,
        #[serde(default)]
        key_ops: Option<Vec<String>>,
        n: String,
        e: String,
    }
    let raw: RawJwk = serde_json::from_value(jwk).ok()?;
    if raw.kty != "RSA"
        || raw.use_.as_deref().is_some_and(|u| u != "sig")
        || raw.alg.as_deref().is_some_and(|a| a != "RS256")
        || raw
            .key_ops
            .as_ref()
            .is_some_and(|ops| !ops.iter().any(|op| op == "verify"))
    {
        return None;
    }
    let decode = |s: &str| -> Option<Vec<u8>> {
        let bytes = URL_SAFE_NO_PAD.decode(s).ok()?;
        let start = bytes.iter().position(|&b| b != 0)?;
        Some(bytes[start..].to_vec())
    };
    Some(RsaKey {
        kid: raw.kid,
        n: decode(&raw.n)?,
        e: decode(&raw.e)?,
    })
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

/// The claims the checks read, strictly typed. `serde` refuses a duplicated
/// field, a missing required one and a wrong type alike.
#[derive(Deserialize)]
struct RawClaims {
    iss: String,
    sub: String,
    aud: Audience,
    #[serde(deserialize_with = "numeric_date")]
    exp: f64,
    #[serde(deserialize_with = "numeric_date")]
    iat: f64,
    #[serde(default, deserialize_with = "optional_numeric_date")]
    nbf: Option<f64>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    azp: Option<String>,
    #[serde(default, deserialize_with = "optional_numeric_date")]
    auth_time: Option<f64>,
}

/// A JSON number of seconds since the epoch (RFC 7519 §2): not a string,
/// not null, not negative.
fn numeric_date<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    let n = serde_json::Number::deserialize(d)?;
    n.as_f64()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .ok_or_else(|| D::Error::custom("not a NumericDate"))
}

fn optional_numeric_date<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    numeric_date(d).map(Some)
}

fn check_claims(
    payload: &[u8],
    issuer: &str,
    expected: &IdTokenExpectations<'_>,
    now: SystemTime,
) -> Result<IdTokenClaims, OidcError> {
    let raw: RawClaims = serde_json::from_slice(payload).map_err(|_| {
        OidcError::InvalidClaims("a required claim is missing, repeated or of the wrong type")
    })?;
    let claims: Map<String, Value> = serde_json::from_slice(payload)
        .map_err(|_| OidcError::InvalidClaims("not a JSON object"))?;

    let client_id = expected.client_id();
    if raw.iss != issuer {
        return Err(OidcError::WrongIssuer);
    }
    // The token is signed by the issuer and names it. Whose account it is
    // comes before anything else about it: another account's token is that
    // whatever else is wrong with it (an `aud`, an expiry), because a caller
    // ends the session on it and ignores every other fault.
    if let IdTokenExpectations::Refresh { original, .. } = expected {
        if raw.sub != original.subject {
            return Err(OidcError::SubjectChanged);
        }
    }
    let audience_ok = match &raw.aud {
        Audience::One(aud) => aud == client_id,
        Audience::Many(auds) => matches!(auds.as_slice(), [aud] if aud == client_id),
    };
    if !audience_ok || raw.azp.as_deref().is_some_and(|azp| azp != client_id) {
        return Err(OidcError::WrongAudience);
    }
    let now = now
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let skew = CLOCK_SKEW.as_secs_f64();
    if now > raw.exp + skew {
        return Err(OidcError::Expired);
    }
    if raw.nbf.is_some_and(|nbf| nbf > now + skew) || raw.iat > now + skew {
        return Err(OidcError::NotYetValid);
    }
    match expected {
        IdTokenExpectations::SignIn { nonce, .. } => {
            if !raw.nonce.as_deref().is_some_and(|n| nonce.matches(n)) {
                return Err(OidcError::NonceMismatch);
            }
        }
        IdTokenExpectations::Refresh { original, .. } => {
            check_refreshed(&raw, original)?;
        }
    }
    if raw.sub.is_empty() {
        return Err(OidcError::InvalidClaims("sub is empty"));
    }
    Ok(IdTokenClaims {
        issuer: raw.iss,
        subject: raw.sub,
        audience: client_id.to_string(),
        expires_at: raw.exp as u64,
        issued_at: raw.iat as u64,
        // Profile claims, read leniently: they decide nothing here.
        email: claims
            .get("email")
            .and_then(Value::as_str)
            .map(str::to_string),
        email_verified: claims.get("email_verified").and_then(Value::as_bool),
        claims,
    })
}

/// OpenID Connect Core §12.2: a refreshed ID token is for the same issuer,
/// account and client as the original.
fn check_refreshed(raw: &RawClaims, original: &IdTokenClaims) -> Result<(), OidcError> {
    let claim = |name: &str| original.claims.get(name);
    if raw.iss != original.issuer {
        return Err(OidcError::WrongIssuer);
    }
    if raw.sub != original.subject {
        return Err(OidcError::SubjectChanged);
    }
    let audience_ok = match &raw.aud {
        Audience::One(aud) => *aud == original.audience,
        Audience::Many(auds) => matches!(auds.as_slice(), [aud] if *aud == original.audience),
    };
    // `azp` present in both and equal, or absent from both.
    if !audience_ok || raw.azp.as_deref() != claim("azp").and_then(Value::as_str) {
        return Err(OidcError::WrongAudience);
    }
    // It should have no nonce; one it has must be the original's.
    if let Some(nonce) = raw.nonce.as_deref() {
        let original_nonce = claim("nonce").and_then(Value::as_str);
        if !original_nonce.is_some_and(|o| constant_time_eq(nonce, o)) {
            return Err(OidcError::NonceMismatch);
        }
    }
    // `auth_time` is still the time of the original sign-in.
    if let (Some(new), Some(old)) = (raw.auth_time, claim("auth_time").and_then(Value::as_f64)) {
        if new != old {
            return Err(OidcError::InvalidClaims(
                "auth_time is not the original sign-in's",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_endpoints_are_allowed_only_for_a_loopback_issuer() {
        let remote = Url::parse("https://auth.example.com").unwrap();
        let local = Url::parse("http://127.0.0.1:8080").unwrap();
        for value in [
            "http://127.0.0.1:9/token",
            "http://localhost:9/token",
            "http://[::1]:9/token",
        ] {
            let result = discovered_endpoint(&remote, "token_endpoint", value);
            assert!(
                matches!(result, Err(OidcError::InvalidDiscovery(_))),
                "an https issuer named {value}: {result:?}"
            );
            discovered_endpoint(&local, "token_endpoint", value).unwrap();
        }
        for issuer in [&remote, &local] {
            discovered_endpoint(issuer, "token_endpoint", "https://auth.example.com/token")
                .unwrap();
            assert!(discovered_endpoint(issuer, "jwks_uri", "http://auth.example.com/j").is_err());
        }
    }
}
