//! Sign in with ChatGPT, for ChatGPT plan usage (#474): the session that
//! signs a user in, keeps their tokens fresh and signs them out.
//!
//! It is built on the generic OAuth client in [`crate::oauth`] (PKCE, the
//! loopback listener, discovery and ID-token checks, the token endpoint) and
//! adds what is particular to OpenAI's open-source "ChatGPT plan usage"
//! flow, as documented at
//! <https://developers.openai.com/siwc/token-sharing-open-source> and
//! summarised in `docs/specs/2026-10-10-chatgpt-subscription-sign-in.md` §3:
//!
//! - [`spec`]: the protocol's parameters and rules. The authorization
//!   request, dynamic registration and the callback rules, the scope check,
//!   the host ID, and how a failed refresh is classified.
//! - [`store`]: what is kept where. The non-secret registration (host ID,
//!   issued client id, verified account) in `registration.json`, written
//!   atomically and owner-only; the refresh token behind
//!   [`store::RefreshTokenStore`], which the desktop app implements with the
//!   keychain slot [`store::REFRESH_TOKEN_SLOT`]. Access tokens stay in
//!   memory.
//! - [`session`]: [`ChatGptSession`]. Sign-in, the bearer for each request
//!   with single-flight refresh inside the process and a file lock across
//!   processes, and sign-out with revocation.
//!
//! - [`provider`]: [`ChatGptProvider`], the `chatgpt` provider that sends the
//!   agent's requests to the Responses API with the session's bearer, and
//!   [`errors`]: [`PlanUsageError`], what each way the route can fail is told
//!   as. The agent loop reads the bearer through [`crate::Credential`].
//!
//! Nothing here depends on Tauri. The desktop's side of it (the keychain store,
//! the commands, the status event) is `apps/desktop/src-tauri/src/chatgpt.rs`
//! (#474 PR 5).
//!
//! # Endpoints
//!
//! Tokens go only to OpenAI's issuer (spec §4.9, the exfiltration guard).
//! [`Endpoints`] has private fields and one constructor in every build of the
//! app, [`Endpoints::production`], whose issuer is
//! [`spec::ISSUER`] (`https://auth.openai.com`) and whose authorization and
//! token endpoints are the documented ones under it. No base-URL override,
//! keychain slot, environment variable or IPC argument reaches it.
//!
//! Tests need a mock issuer, so there is a second constructor,
//! [`Endpoints::loopback_for_tests`], and it is compiled only with the
//! crate's `test-endpoints` feature. Nothing in the app enables that feature;
//! only `ai`'s own dev-dependency on itself does, so it exists in this
//! crate's test builds and in no build of the desktop app or the CLI, debug
//! or release. Even there it accepts only an issuer on a loopback address.
//! The revocation endpoint comes from discovery and must be on the issuer's
//! own origin ([`spec::same_origin`]), so a discovery document cannot send
//! the refresh token elsewhere either.

pub mod errors;
pub mod provider;
pub mod session;
pub mod spec;
pub mod store;

pub use errors::{PlanUsageError, PlanUsageKind};
pub use provider::{ChatGptProvider, ReasoningReplay, CHATGPT_ID};

pub use session::{
    BearerError, ChatGptSession, ChatGptStatus, Clock, EndReason, ForgetError, ForgetReport,
    Revocation, SessionConfig, SessionState, SignInError, SignInIntent, SignOutReport, SystemClock,
    TokenStorage,
};

use url::Url;

/// Where the session sends its requests: the issuer, and the authorization
/// and token endpoints under it. See "Endpoints" above for why it cannot be
/// pointed anywhere else in the app.
#[derive(Clone, Debug)]
pub struct Endpoints {
    issuer: String,
    authorize: Url,
    token: Url,
}

impl Endpoints {
    /// OpenAI's: `https://auth.openai.com`, with the documented
    /// `/api/accounts/authorize` and `/api/accounts/oauth/token`.
    pub fn production() -> Self {
        Self::under(spec::ISSUER).expect("the production issuer is a valid URL")
    }

    /// A mock issuer on this machine, for tests: `issuer` must be `http` or
    /// `https` on a loopback address (`127.0.0.1`, `[::1]`, `localhost`),
    /// with no path, query or credentials. Compiled only with the
    /// `test-endpoints` feature, which no build of the app enables.
    #[cfg(feature = "test-endpoints")]
    pub fn loopback_for_tests(issuer: &str) -> Result<Self, crate::oauth::EndpointError> {
        use crate::oauth::EndpointError;
        let url = Url::parse(issuer)?;
        if !crate::oauth::is_loopback_host(&url)
            || !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(EndpointError::NotHttps(format!(
                "{} (the test issuer must be a bare loopback origin)",
                url.origin().ascii_serialization()
            )));
        }
        Self::under(issuer.trim_end_matches('/'))
    }

    fn under(issuer: &str) -> Result<Self, crate::oauth::EndpointError> {
        let authorize = Url::parse(&format!("{issuer}{}", spec::AUTHORIZE_PATH))?;
        let token = Url::parse(&format!("{issuer}{}", spec::TOKEN_PATH))?;
        crate::oauth::check_endpoint(&authorize)?;
        crate::oauth::check_endpoint(&token)?;
        Ok(Self {
            issuer: issuer.to_string(),
            authorize,
            token,
        })
    }

    /// The issuer, exactly as ID tokens and the discovery document name it.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn authorization_endpoint(&self) -> &Url {
        &self.authorize
    }

    pub fn token_endpoint(&self) -> &Url {
        &self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_endpoints_are_openais_documented_ones() {
        let e = Endpoints::production();
        assert_eq!(e.issuer(), "https://auth.openai.com");
        assert_eq!(
            e.authorization_endpoint().as_str(),
            "https://auth.openai.com/api/accounts/authorize"
        );
        assert_eq!(
            e.token_endpoint().as_str(),
            "https://auth.openai.com/api/accounts/oauth/token"
        );
    }

    #[test]
    #[cfg(feature = "test-endpoints")]
    fn the_test_issuer_must_be_a_bare_loopback_origin() {
        for ok in [
            "http://127.0.0.1:8080",
            "http://127.0.0.1:8080/",
            "http://localhost:9",
            "http://[::1]:9",
        ] {
            let e = Endpoints::loopback_for_tests(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
            assert!(e.token_endpoint().as_str().ends_with(spec::TOKEN_PATH));
            assert!(!e.issuer().ends_with('/'), "{}", e.issuer());
        }
        for bad in [
            "https://auth.openai.com",
            "https://evil.example",
            "http://10.0.0.1:80",
            "http://127.0.0.1.evil.example",
            "http://user:pw@127.0.0.1:80",
            "http://127.0.0.1:80/prefix",
            "http://127.0.0.1:80/?q=1",
            "file:///tmp/x",
        ] {
            assert!(Endpoints::loopback_for_tests(bad).is_err(), "{bad}");
        }
    }
}
