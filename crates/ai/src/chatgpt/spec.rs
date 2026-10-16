//! The ChatGPT plan-usage protocol's parameters and rules, as OpenAI
//! documents them for open-source apps (spec §3; the pages are
//! <https://developers.openai.com/siwc/token-sharing-open-source/sign-in>,
//! `.../profiles-and-sessions`, `.../token-reference` and
//! `.../errors-and-recovery`).
//!
//! Everything here is a constant or a pure function, so each rule has a unit
//! test of its own; [`super::session`] applies them in order.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::oauth::loopback::Callback;
use crate::oauth::pkce::{Nonce, PkceChallenge, RandomnessError, State};
use crate::oauth::token::{TokenError, TokenResponse};
use crate::oauth::{authorization_url, AuthorizationRequest, AuthorizationUrlError};

use super::Endpoints;

/// The production issuer. ID tokens carry it as `iss`, and its discovery
/// document is at `/.well-known/openid-configuration` under it.
pub const ISSUER: &str = "https://auth.openai.com";
/// The documented authorization endpoint, under the issuer.
pub const AUTHORIZE_PATH: &str = "/api/accounts/authorize";
/// The documented token endpoint, under the issuer.
pub const TOKEN_PATH: &str = "/api/accounts/oauth/token";

/// The `resource` of every authorization request, code exchange and
/// refresh: the API the tokens are for. An identifier, never a URL this
/// client sends anything to.
pub const RESOURCE: &str = "https://api.openai.com/v1";

/// The scopes, exactly as documented: the identity scopes `openid profile
/// email`, then the plan-usage scopes `offline_access resource.invoke
/// chatgpt.tokens.use.direct`. No connector scope, nothing else.
pub const SCOPE: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

/// The scope that permits ChatGPT plan usage. "A valid ID token alone does
/// not authorize ChatGPT plan usage": the token response must grant this.
pub const PLAN_USAGE_SCOPE: &str = "chatgpt.tokens.use.direct";

/// The `client_id` of a first-time registration. It is an entry point, never
/// a client id to save or to exchange a code with.
pub const DYNAMIC_CLIENT_ID: &str = "dynamic_agent_client";

/// `agent_name_hint`, sent on first registration only. "Use it consistently
/// across installations."
pub const AGENT_NAME_HINT: &str = "edytlab";

/// The callback path. Only the port of the redirect URI may vary between
/// sign-ins; the scheme, host (`127.0.0.1`) and this path may not.
pub const CALLBACK_PATH: &str = "/auth/callback";

/// The port tried first for the callback listener. OpenAI's example uses
/// it, as does Codex CLI; when it is taken (by Codex, or by our own last
/// attempt's connections in TIME_WAIT) the listener takes any free port.
pub const PREFERRED_CALLBACK_PORT: u16 = 1455;

/// Refresh once the access token is this close to expiring.
pub const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);

/// Assumed when a token response leaves `expires_in` out: the documented
/// lifetime of an access token.
pub const DEFAULT_ACCESS_TOKEN_LIFETIME: Duration = Duration::from_secs(60 * 60);

// --- The host ID ------------------------------------------------------------

/// This installation's `ext_agent_host_id`: `urn:uuid:` and a random
/// (version 4) UUID.
///
/// "Use an opaque value, not an email, user ID, or other user-identifying
/// value. A host ID is not an authentication credential." It is chosen and
/// saved before the first sign-in and sent with every authorization request,
/// so ChatGPT can tell this host's usage apart. Of the documented forms
/// (`urn:ietf:params:oauth:jwk-thumbprint:`, `urn:uuid:`, `did:key:`) this is
/// the simplest; OpenAI does not verify possession of a key for the others.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HostId(String);

const HOST_ID_PREFIX: &str = "urn:uuid:";

impl HostId {
    /// A fresh one, from the OS random generator.
    pub fn generate() -> Result<Self, RandomnessError> {
        let mut b = [0u8; 16];
        SystemRandom::new()
            .fill(&mut b)
            .map_err(|_| RandomnessError)?;
        b[6] = (b[6] & 0x0f) | 0x40; // version 4
        b[8] = (b[8] & 0x3f) | 0x80; // RFC 9562 variant
        let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
        Ok(Self(format!(
            "{HOST_ID_PREFIX}{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A saved host ID that is not `urn:uuid:` and a lowercase UUID.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("not a urn:uuid host ID")]
pub struct InvalidHostId;

impl TryFrom<String> for HostId {
    type Error = InvalidHostId;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let uuid = value.strip_prefix(HOST_ID_PREFIX).ok_or(InvalidHostId)?;
        let groups: Vec<&str> = uuid.split('-').collect();
        let lengths_ok = groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12]);
        let hex_ok = groups
            .iter()
            .all(|g| g.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')));
        if lengths_ok && hex_ok {
            Ok(Self(value))
        } else {
            Err(InvalidHostId)
        }
    }
}

impl From<HostId> for String {
    fn from(id: HostId) -> Self {
        id.0
    }
}

// --- The authorization request ----------------------------------------------

/// Which kind of authorization request an attempt makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttemptKind {
    /// First registration: `client_id=dynamic_agent_client` and
    /// `agent_name_hint`. The callback must name the issued client.
    Register,
    /// A returning sign-in with the issued client id saved for this account.
    /// The callback may leave `client_id` out, and may not name another.
    Reauthorize { client_id: String },
}

impl AttemptKind {
    /// The `client_id` the authorization request carries.
    pub fn request_client_id(&self) -> &str {
        match self {
            Self::Register => DYNAMIC_CLIENT_ID,
            Self::Reauthorize { client_id } => client_id,
        }
    }
}

/// One attempt's authorization request, before it is turned into a URL.
pub struct AuthorizeAttempt<'a> {
    pub kind: &'a AttemptKind,
    pub host_id: &'a HostId,
    /// Exactly the listener's: `http://127.0.0.1:<port>/auth/callback`.
    pub redirect_uri: &'a str,
    pub state: &'a State,
    pub nonce: &'a Nonce,
    pub code_challenge: &'a PkceChallenge,
    /// The account's email, saved from its verified ID token, on a returning
    /// sign-in. `id_token_hint` is never sent: ID tokens are not kept, and
    /// one would put a token in the browser's history.
    pub login_hint: Option<&'a str>,
    /// `prompt=consent`: only when the user turns plan usage back on after
    /// declining it, never on an ordinary sign-in.
    pub consent: bool,
}

impl fmt::Debug for AuthorizeAttempt<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizeAttempt")
            .field("kind", self.kind)
            .field("redirect_uri", &self.redirect_uri)
            .field("login_hint", &self.login_hint.map(|_| "<redacted>"))
            .field("consent", &self.consent)
            .finish_non_exhaustive()
    }
}

/// The URL to open in the browser for `attempt` (spec §3.2).
pub fn authorize_url(
    endpoints: &Endpoints,
    attempt: &AuthorizeAttempt<'_>,
) -> Result<Url, AuthorizationUrlError> {
    let mut extra: Vec<(&str, &str)> = Vec::with_capacity(5);
    if attempt.kind == &AttemptKind::Register {
        extra.push(("agent_name_hint", AGENT_NAME_HINT));
    }
    extra.push(("ext_agent_host_id", attempt.host_id.as_str()));
    extra.push(("resource", RESOURCE));
    if let (AttemptKind::Reauthorize { .. }, Some(hint)) = (attempt.kind, attempt.login_hint) {
        extra.push(("login_hint", hint));
    }
    if attempt.consent {
        extra.push(("prompt", "consent"));
    }
    authorization_url(
        endpoints.authorization_endpoint(),
        &AuthorizationRequest {
            client_id: attempt.kind.request_client_id(),
            redirect_uri: attempt.redirect_uri,
            scope: SCOPE,
            state: attempt.state,
            nonce: Some(attempt.nonce),
            code_challenge: attempt.code_challenge,
        },
        &extra,
    )
}

// --- The callback -------------------------------------------------------------

/// A callback that broke the registration rules (spec §3.4).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CallbackError {
    /// "If a new-registration callback does not include an issued ID, treat
    /// registration as incomplete." `dynamic_agent_client` is not an issued
    /// id, and neither is an empty or overlong one.
    #[error("registration did not complete: the sign-in did not return this app's client id")]
    RegistrationIncomplete,
    /// "If the callback supplies a different client ID, reject the result
    /// rather than replacing the selected account's registration."
    #[error("the sign-in returned a different client id than this account's registration")]
    ClientIdChanged,
}

/// The longest client id accepted from a callback. Issued ids are short
/// (`oaiapp_...`); this only keeps junk out of `registration.json`.
const MAX_CLIENT_ID_LEN: usize = 256;

/// The client id to exchange the code with, from a callback whose `state`
/// matched (the listener checks that first).
pub fn issued_client_id(kind: &AttemptKind, callback: &Callback) -> Result<String, CallbackError> {
    let returned = callback.params.get("client_id").map(String::as_str);
    match kind {
        AttemptKind::Register => match returned {
            Some(id) if is_issued_client_id(id) => Ok(id.to_string()),
            _ => Err(CallbackError::RegistrationIncomplete),
        },
        AttemptKind::Reauthorize { client_id } => match returned {
            None => Ok(client_id.clone()),
            Some(id) if id == client_id => Ok(client_id.clone()),
            Some(_) => Err(CallbackError::ClientIdChanged),
        },
    }
}

fn is_issued_client_id(id: &str) -> bool {
    !id.is_empty()
        && id != DYNAMIC_CLIENT_ID
        && id.len() <= MAX_CLIENT_ID_LEN
        && id.bytes().all(|b| b.is_ascii_graphic())
}

// --- The token response ---------------------------------------------------------

/// Whether a token response grants plan usage. Only the response's own
/// `scope` counts: a response that leaves it out grants nothing here, since
/// the docs say to check the granted scopes before any inference.
pub fn grants_plan_usage(scopes: &[String]) -> bool {
    scopes.iter().any(|s| s == PLAN_USAGE_SCOPE)
}

/// The granted scopes of a token response, if it names them.
pub fn granted_scopes(response: &TokenResponse) -> Option<Vec<String>> {
    response.scope.as_ref()?;
    Some(response.scopes().into_iter().map(str::to_string).collect())
}

/// `earliest_refresh_at` of a token response, read as Unix seconds. OpenAI
/// returns it without defining it further; the session treats it as the
/// earliest time to refresh a token that is still valid. A value that is
/// not a non-negative number is ignored.
pub fn earliest_refresh_at(response: &TokenResponse) -> Option<SystemTime> {
    let secs = response.extra.get("earliest_refresh_at")?.as_f64()?;
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    // Capped at 2106, so `UNIX_EPOCH + huge` cannot overflow and panic.
    let secs = secs.min(u32::MAX as f64);
    UNIX_EPOCH.checked_add(Duration::from_secs_f64(secs))
}

// --- Refresh failures -------------------------------------------------------------

/// What a failed refresh means for the session (spec §3.6, and the "Refresh
/// errors" and "Disconnection" sections of the docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefreshFailure {
    /// The refresh token cannot be used again: `invalid_grant`,
    /// `invalid_refresh_token`, `token_expired`, `refresh_token_expired`,
    /// `refresh_token_invalidated` or `refresh_token_reused`. "Clear
    /// unusable tokens and repeat OAuth with the saved issued client ID."
    /// The registration and host ID are kept.
    Unusable { code: String },
    /// Nothing is known to be wrong with the credentials: the network, a
    /// timeout, a 5xx, a 429, an unreadable reply. "Do not erase credentials
    /// solely because of a temporary network or infrastructure failure";
    /// retry later with bounded backoff.
    Transient { reason: String },
    /// Refused for another reason (`invalid_client`, which the docs answer
    /// with "fix the client configuration", or any other code or 4xx). The
    /// credentials are kept, since nothing says they are unusable, and the
    /// failure is reported.
    Refused { status: u16, code: Option<String> },
}

/// The codes that make a refresh token unusable, from the docs.
pub const UNUSABLE_REFRESH_TOKEN_CODES: &[&str] = &[
    "invalid_grant",
    "invalid_refresh_token",
    "token_expired",
    "refresh_token_expired",
    "refresh_token_invalidated",
    "refresh_token_reused",
];

/// The code a raced refresh answers with: two refreshes sent the same
/// rotating token.
pub const REFRESH_TOKEN_REUSED: &str = "refresh_token_reused";

/// Whether an HTTP status says nothing about the request's credentials: a
/// server error, a rate limit or a timeout. Such a reply is temporary
/// whatever error code its body carries.
fn temporary_status(status: u16) -> bool {
    status >= 500 || status == 429 || status == 408
}

/// Classifies a refresh's [`TokenError`].
///
/// The status comes first: a 5xx, 429 or 408 is temporary even when its body
/// names one of the unusable-token codes, since a server that is failing
/// cannot be trusted to have judged the token. Only a 4xx can make a refresh
/// token unusable.
pub fn classify_refresh_error(error: &TokenError) -> RefreshFailure {
    match error {
        TokenError::OAuth { status, body } => {
            let code = body.error.to_ascii_lowercase();
            if temporary_status(*status)
                || matches!(code.as_str(), "temporarily_unavailable" | "server_error")
            {
                RefreshFailure::Transient {
                    reason: format!("the token endpoint answered {status} ({code})"),
                }
            } else if (400..500).contains(status)
                && UNUSABLE_REFRESH_TOKEN_CODES.contains(&code.as_str())
            {
                RefreshFailure::Unusable { code }
            } else {
                RefreshFailure::Refused {
                    status: *status,
                    code: Some(code),
                }
            }
        }
        TokenError::Status { status } if temporary_status(*status) => RefreshFailure::Transient {
            reason: format!("the token endpoint answered HTTP {status}"),
        },
        TokenError::Status { status } => RefreshFailure::Refused {
            status: *status,
            code: None,
        },
        TokenError::Network(_)
        | TokenError::Client(_)
        | TokenError::TooLarge(_)
        | TokenError::InvalidResponse(_) => RefreshFailure::Transient {
            reason: error.to_string(),
        },
        // Ours to get right, not the server's to refuse: kept, reported.
        TokenError::Endpoint(_) | TokenError::EmptyClientId | TokenError::Parameter(_) => {
            RefreshFailure::Refused {
                status: 0,
                code: None,
            }
        }
    }
}

// --- Endpoints from discovery -------------------------------------------------------

/// Whether `url` is on `issuer`'s origin (scheme, host and port). The
/// revocation endpoint is read from discovery, and the refresh token is sent
/// there: it may not be anywhere but the issuer itself.
pub fn same_origin(url: &Url, issuer: &str) -> bool {
    Url::parse(issuer).is_ok_and(|issuer| issuer.origin() == url.origin())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::pkce::PkceVerifier;
    use crate::oauth::token::OAuthErrorBody;
    use crate::oauth::SecretString;
    use std::collections::BTreeMap;

    fn callback(params: &[(&str, &str)]) -> Callback {
        Callback {
            code: SecretString::new("code"),
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    #[test]
    fn host_ids_are_fresh_urn_uuids_of_version_4() {
        let a = HostId::generate().unwrap();
        let b = HostId::generate().unwrap();
        assert_ne!(a, b);
        for id in [&a, &b] {
            let s = id.as_str();
            assert!(s.starts_with("urn:uuid:"), "{s}");
            assert_eq!(s.len(), "urn:uuid:".len() + 36, "{s}");
            let uuid = &s["urn:uuid:".len()..];
            assert_eq!(&uuid[14..15], "4", "version nibble: {s}");
            assert!(
                matches!(&uuid[19..20], "8" | "9" | "a" | "b"),
                "variant: {s}"
            );
            // It survives a save and a load.
            assert_eq!(HostId::try_from(s.to_string()).unwrap(), *id);
        }
    }

    #[test]
    fn a_saved_host_id_must_be_a_urn_uuid() {
        for bad in [
            "",
            "urn:uuid:",
            "person@example.com",
            "uuid:0e2a9a52-3a4b-4c5d-8e6f-7a8b9c0d1e2f",
            "urn:uuid:0E2A9A52-3A4B-4C5D-8E6F-7A8B9C0D1E2F",
            "urn:uuid:0e2a9a52-3a4b-4c5d-8e6f-7a8b9c0d1e2",
            "urn:uuid:0e2a9a52-3a4b-4c5d-8e6f7a8b9c0d1e2f0",
            "urn:uuid:0e2a9a52-3a4b-4c5d-8e6f-7a8b9c0d1e2g",
        ] {
            assert_eq!(
                HostId::try_from(bad.to_string()),
                Err(InvalidHostId),
                "{bad}"
            );
        }
    }

    fn query(url: &Url) -> BTreeMap<String, Vec<String>> {
        let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (k, v) in url.query_pairs() {
            m.entry(k.into_owned()).or_default().push(v.into_owned());
        }
        m
    }

    fn url_for(kind: &AttemptKind, login_hint: Option<&str>, consent: bool) -> Url {
        let host_id = HostId::generate().unwrap();
        let state = State::generate().unwrap();
        let nonce = Nonce::generate().unwrap();
        let challenge = PkceVerifier::generate().unwrap().challenge();
        authorize_url(
            &Endpoints::production(),
            &AuthorizeAttempt {
                kind,
                host_id: &host_id,
                redirect_uri: "http://127.0.0.1:1455/auth/callback",
                state: &state,
                nonce: &nonce,
                code_challenge: &challenge,
                login_hint,
                consent,
            },
        )
        .unwrap()
    }

    #[test]
    fn a_first_registration_names_the_app_and_the_host() {
        let url = url_for(&AttemptKind::Register, Some("person@example.com"), false);
        assert_eq!(
            url.as_str().split('?').next(),
            Some("https://auth.openai.com/api/accounts/authorize")
        );
        let q = query(&url);
        let one = |k: &str| q.get(k).map(|v| v.join("|"));
        assert_eq!(one("client_id").as_deref(), Some("dynamic_agent_client"));
        assert_eq!(one("agent_name_hint").as_deref(), Some("edytlab"));
        assert!(one("ext_agent_host_id").unwrap().starts_with("urn:uuid:"));
        assert_eq!(
            one("scope").as_deref(),
            Some("openid profile email offline_access resource.invoke chatgpt.tokens.use.direct")
        );
        assert_eq!(
            one("resource").as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(one("response_type").as_deref(), Some("code"));
        assert_eq!(one("code_challenge_method").as_deref(), Some("S256"));
        assert_eq!(
            one("redirect_uri").as_deref(),
            Some("http://127.0.0.1:1455/auth/callback")
        );
        for k in ["state", "nonce", "code_challenge"] {
            assert!(one(k).is_some_and(|v| v.len() >= 43), "{k}");
        }
        // No login hint on a registration, no consent prompt, no ID token.
        for absent in ["login_hint", "prompt", "id_token_hint"] {
            assert!(!q.contains_key(absent), "{absent} in {url}");
        }
        assert!(
            q.values().all(|v| v.len() == 1),
            "a repeated parameter: {q:?}"
        );
    }

    #[test]
    fn a_returning_sign_in_uses_the_issued_client_and_no_name_hint() {
        let kind = AttemptKind::Reauthorize {
            client_id: "oaiapp_123".into(),
        };
        let q = query(&url_for(&kind, Some("person@example.com"), false));
        assert_eq!(q["client_id"], ["oaiapp_123"]);
        assert_eq!(q["login_hint"], ["person@example.com"]);
        assert!(q["ext_agent_host_id"][0].starts_with("urn:uuid:"));
        assert!(!q.contains_key("agent_name_hint"));
        assert!(!q.contains_key("prompt"));

        // Turning plan usage back on asks for consent, with the full scopes.
        let q = query(&url_for(&kind, None, true));
        assert_eq!(q["prompt"], ["consent"]);
        assert_eq!(q["scope"], [SCOPE]);
        assert!(!q.contains_key("login_hint"));
    }

    #[test]
    fn registration_needs_an_issued_client_id_from_the_callback() {
        let register = AttemptKind::Register;
        assert_eq!(
            issued_client_id(&register, &callback(&[("client_id", "oaiapp_9")])),
            Ok("oaiapp_9".into())
        );
        let long = "x".repeat(MAX_CLIENT_ID_LEN + 1);
        for bad in [
            vec![],
            vec![("client_id", "")],
            vec![("client_id", "dynamic_agent_client")],
            vec![("client_id", "has space")],
            vec![("client_id", long.as_str())],
        ] {
            assert_eq!(
                issued_client_id(&register, &callback(&bad)),
                Err(CallbackError::RegistrationIncomplete),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn reauthorization_keeps_the_saved_client_and_refuses_another() {
        let kind = AttemptKind::Reauthorize {
            client_id: "oaiapp_1".into(),
        };
        assert_eq!(
            issued_client_id(&kind, &callback(&[])),
            Ok("oaiapp_1".into())
        );
        assert_eq!(
            issued_client_id(&kind, &callback(&[("client_id", "oaiapp_1")])),
            Ok("oaiapp_1".into())
        );
        for other in ["oaiapp_2", "", "dynamic_agent_client"] {
            assert_eq!(
                issued_client_id(&kind, &callback(&[("client_id", other)])),
                Err(CallbackError::ClientIdChanged),
                "{other:?}"
            );
        }
    }

    #[test]
    fn plan_usage_needs_its_scope_granted() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(grants_plan_usage(&s(&[
            "openid",
            "chatgpt.tokens.use.direct"
        ])));
        assert!(!grants_plan_usage(&s(&[
            "openid",
            "offline_access",
            "resource.invoke"
        ])));
        assert!(!grants_plan_usage(&s(&["chatgpt.tokens.use"])));
        assert!(!grants_plan_usage(&[]));

        let response = |scope: Option<&str>| -> TokenResponse {
            let mut v = serde_json::json!({ "access_token": "a", "token_type": "Bearer" });
            if let Some(scope) = scope {
                v["scope"] = scope.into();
            }
            serde_json::from_value(v).unwrap()
        };
        assert_eq!(granted_scopes(&response(None)), None);
        assert_eq!(
            granted_scopes(&response(Some("openid chatgpt.tokens.use.direct"))),
            Some(s(&["openid", "chatgpt.tokens.use.direct"]))
        );
    }

    #[test]
    fn earliest_refresh_at_is_unix_seconds() {
        let at = |v: serde_json::Value| {
            let r: TokenResponse = serde_json::from_value(serde_json::json!({
                "access_token": "a", "token_type": "Bearer", "earliest_refresh_at": v,
            }))
            .unwrap();
            earliest_refresh_at(&r)
        };
        assert_eq!(
            at(1_790_034_332u64.into()),
            Some(UNIX_EPOCH + Duration::from_secs(1_790_034_332))
        );
        assert_eq!(
            at(1.5.into()),
            Some(UNIX_EPOCH + Duration::from_millis(1500))
        );
        for ignored in [
            serde_json::json!(-1),
            serde_json::json!("2026-10-01T00:00:00Z"),
            serde_json::json!(null),
        ] {
            assert_eq!(at(ignored.clone()), None, "{ignored}");
        }
        assert!(
            at(u64::MAX.into()).is_some(),
            "a huge value is capped, not a panic"
        );
    }

    fn oauth(status: u16, code: &str) -> TokenError {
        TokenError::OAuth {
            status,
            body: serde_json::from_value::<OAuthErrorBody>(serde_json::json!({ "error": code }))
                .unwrap(),
        }
    }

    #[test]
    fn refresh_errors_are_classified_as_the_docs_say() {
        for code in UNUSABLE_REFRESH_TOKEN_CODES {
            for status in [400, 401, 403] {
                assert_eq!(
                    classify_refresh_error(&oauth(status, code)),
                    RefreshFailure::Unusable {
                        code: code.to_string()
                    },
                    "{code} {status}"
                );
            }
        }
        // Codes are matched without regard to case.
        assert!(matches!(
            classify_refresh_error(&oauth(400, "Refresh_Token_Reused")),
            RefreshFailure::Unusable { .. }
        ));
        // A failing server decides nothing about the token, whatever code it
        // sends: temporary, never unusable.
        let wrapped = |status: u16, code: &str| TokenError::OAuth {
            status,
            body: serde_json::from_value::<OAuthErrorBody>(
                serde_json::json!({ "error": { "code": code, "message": "x" } }),
            )
            .unwrap(),
        };
        for e in [
            oauth(503, "invalid_grant"),
            wrapped(502, "refresh_token_expired"),
            oauth(500, "refresh_token_reused"),
            oauth(429, "refresh_token_invalidated"),
            oauth(408, "token_expired"),
        ] {
            assert!(
                matches!(classify_refresh_error(&e), RefreshFailure::Transient { .. }),
                "{e:?}"
            );
        }
        // Temporary: kept, retried later.
        for e in [
            oauth(503, "temporarily_unavailable"),
            oauth(500, "server_error"),
            oauth(502, "upstream_unavailable"),
            oauth(400, "temporarily_unavailable"),
            oauth(429, "rate_limited"),
            TokenError::Status { status: 502 },
            TokenError::Status { status: 429 },
            TokenError::Status { status: 408 },
            TokenError::TooLarge(1),
            TokenError::InvalidResponse("x".into()),
        ] {
            assert!(
                matches!(classify_refresh_error(&e), RefreshFailure::Transient { .. }),
                "{e:?}"
            );
        }
        // Refused, with the credentials kept.
        assert_eq!(
            classify_refresh_error(&oauth(401, "invalid_client")),
            RefreshFailure::Refused {
                status: 401,
                code: Some("invalid_client".into())
            }
        );
        for e in [
            oauth(400, "invalid_request"),
            oauth(400, "invalid_scope"),
            TokenError::Status { status: 401 },
            TokenError::Status { status: 404 },
        ] {
            assert!(
                matches!(classify_refresh_error(&e), RefreshFailure::Refused { .. }),
                "{e:?}"
            );
        }
    }

    #[test]
    fn the_revocation_endpoint_must_be_on_the_issuers_origin() {
        let issuer = "https://auth.openai.com";
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(same_origin(
            &u("https://auth.openai.com/api/accounts/oauth/revoke"),
            issuer
        ));
        for other in [
            "https://auth.openai.com.evil.example/revoke",
            "https://evil.example/revoke",
            "http://auth.openai.com/revoke",
            "https://auth.openai.com:8443/revoke",
            "https://api.openai.com/revoke",
        ] {
            assert!(!same_origin(&u(other), issuer), "{other}");
        }
    }
}
