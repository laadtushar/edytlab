//! What a request is authenticated with, and how it is sent (#474, PR 4).
//!
//! Every provider before the ChatGPT plan has an API key that is the same on
//! every request. The plan has an OAuth session instead: an access token that
//! lives an hour and is renewed from a refresh token that rotates, so a
//! request cannot be built from a string held for the agent's lifetime. A
//! [`Credential`] is either, and [`LlmConfig::send_with`] is the one place a
//! request is authenticated and sent, for the streamed step, the plan, the
//! classifier, the model probe:
//!
//! * a key: [`LlmProvider::apply_auth`](crate::LlmProvider::apply_auth) with
//!   it, one send;
//! * a session: the session's bearer
//!   ([`ChatGptSession::bearer`](crate::chatgpt::ChatGptSession::bearer),
//!   renewed first when it is about to expire), and **one** renewal and resend
//!   if the API answers `401`
//!   ([`refresh_rejected`](crate::chatgpt::ChatGptSession::refresh_rejected)),
//!   since a token can be refused before its clock says it is old (the person
//!   disconnected the app in ChatGPT settings, or another window renewed).
//!   Only a bare `401`, or one that says the subscriber could not be
//!   validated, is worth that: a `401` that carries any other code of the plan
//!   route is that code's answer, and a renewal spends a request to hear it
//!   again ([`LlmProvider::renews_on_401`](crate::LlmProvider::renews_on_401)).
//!   A second `401` is the answer: the caller reads it as a session that
//!   ended. No other status is retried here.
//!
//! A session's bearer is only ever used with the provider that is made for it,
//! and only with one that ignores base-URL overrides, so it cannot be sent to
//! a host a setting names.

use std::fmt;
use std::sync::Arc;

use crate::chatgpt::{ChatGptSession, PlanUsageError};
use crate::provider::CredentialKind;
use crate::{Error, LlmConfig, Result};

/// What authenticates a request.
#[derive(Clone)]
pub enum Credential {
    /// An API key, from the keychain. Empty for a provider that needs none.
    ApiKey(String),
    /// The signed-in ChatGPT session: a fresh bearer for every request.
    ChatGpt(Arc<ChatGptSession>),
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Neither the key nor a token, nor the account's email, which the
        // session's own `Debug` already leaves out.
        match self {
            Self::ApiKey(_) => f.write_str("Credential::ApiKey(<redacted>)"),
            Self::ChatGpt(_) => f.write_str("Credential::ChatGpt(<session>)"),
        }
    }
}

impl LlmConfig {
    /// Authenticate and send the request `build` makes, with one renewal and
    /// resend on a `401` if the credential is a ChatGPT session. `build` is
    /// called once per attempt and returns the request with its URL and body
    /// and no authentication.
    pub(crate) async fn send_with(
        &self,
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        match &self.credential {
            Credential::ApiKey(key) => self
                .provider
                .apply_auth(build(), key)
                .send()
                .await
                .map_err(Error::Http),
            Credential::ChatGpt(session) => {
                // A session's token is for the provider made for it, which
                // is sent to one host. Anything else is a wiring mistake, and
                // the token must not go out on it.
                if self.provider.credential_kind() != CredentialKind::ChatGptOAuth
                    || self.provider.honours_base_url_override()
                {
                    return Err(Error::Protocol(
                        "a ChatGPT session can only be used with the chatgpt provider".into(),
                    ));
                }
                let bearer = session.bearer().await.map_err(plan_usage)?;
                let resp = self
                    .provider
                    .apply_auth(build(), bearer.expose_secret())
                    .send()
                    .await
                    .map_err(Error::Http)?;
                if resp.status() != reqwest::StatusCode::UNAUTHORIZED {
                    return Ok(resp);
                }
                // Read the body to see what the 401 says, and hand it on
                // intact if it is not renewed for.
                let (status, headers) = (resp.status(), resp.headers().clone());
                let bytes = resp.bytes().await.map_err(Error::Http)?;
                if !self
                    .provider
                    .renews_on_401(&String::from_utf8_lossy(&bytes))
                {
                    let mut rebuilt = http::Response::new(bytes);
                    *rebuilt.status_mut() = status;
                    *rebuilt.headers_mut() = headers;
                    return Ok(reqwest::Response::from(rebuilt));
                }
                let fresh = session
                    .refresh_rejected(&bearer)
                    .await
                    .map_err(plan_usage)?;
                self.provider
                    .apply_auth(build(), fresh.expose_secret())
                    .send()
                    .await
                    .map_err(Error::Http)
            }
        }
    }

    /// `POST` `body` to the provider's endpoint.
    pub(crate) async fn post_json(
        &self,
        http: &reqwest::Client,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response> {
        let url = format!("{}{}", self.base_url(), self.provider.endpoint_path());
        self.send_with(|| http.post(&url).json(body)).await
    }
}

fn plan_usage(error: crate::chatgpt::BearerError) -> Error {
    Error::PlanUsage(PlanUsageError::from(error))
}
