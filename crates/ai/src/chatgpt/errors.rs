//! What goes wrong on the ChatGPT plan route, as errors the app can tell apart
//! (#474, PR 4).
//!
//! The errors page of OpenAI's plan-usage docs
//! (<https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery>)
//! gives each failure its own recovery, and says plainly that "ChatGPT plan
//! usage errors stop inference. OpenAI does not silently switch the request to
//! another billing path." So a failure here is never retried on another
//! provider, never reduced to one generic "the provider returned an error",
//! and never reads as a problem with a key:
//!
//! | Where it arrives | Code or status | [`PlanUsageError`] |
//! |---|---|---|
//! | stream or body | `subscription_sharing_usage_limit_exceeded`, `subscription_sharing_workplace_individual_usage_limit_exceeded` (429) | [`UsageLimit`](PlanUsageKind::UsageLimit) |
//! | stream or body | `subscription_sharing_user_not_eligible` (403) | [`NotEligible`](PlanUsageKind::NotEligible) |
//! | the session | scope `chatgpt.tokens.use.direct` not granted | [`PlanDisabled`](PlanUsageKind::PlanDisabled) |
//! | the session, or 401 after a refresh, or `subscription_sharing_invalid_user` | | [`SessionEnded`](PlanUsageKind::SessionEnded) |
//! | stream or body | `subscription_sharing_usage_unavailable`, `..._user_unavailable`, `server_error`, `rate_limit_exceeded`, 503, a code-less 429 | [`Unavailable`](PlanUsageKind::Unavailable) |
//! | stream or body | `subscription_sharing_unsupported_capability` (400), `..._route_not_supported` | [`UnsupportedRequest`](PlanUsageKind::UnsupportedRequest) |
//! | stream or body | any other code of the `subscription_sharing_*` or `chatpass_*` family, a 403 | [`Refused`](PlanUsageKind::Refused) |
//!
//! The two families are mapped as families, because a new code appeared
//! within twelve days of the preview's launch (the Business one above): an
//! unknown code in either shows OpenAI's own message and is not retried.
//! Every error carries the request ID of the response it came from
//! (`x-request-id`, or `openai-request-id`), which the errors page asks to be
//! preserved and which is what OpenAI is asked about.
//!
//! Each variant's [`Display`](std::fmt::Display) is the text the person reads,
//! with what to do. Its [`code`](PlanUsageError::code) is a stable word for the
//! window to switch on (the usage-limit modal with **Manage usage**, the
//! "session ended" card), so no screen has to read the sentence back. The text
//! avoids the words the chat reads as a refused key (`chat-errors.ts`), because
//! no key is involved and Settings would be the wrong place to send the
//! person.

use serde_json::Value;

use super::session::BearerError;

/// ChatGPT settings → Usage, where a person reviews and changes what each app
/// may spend. The one URL the app opens for this, fixed.
pub const USAGE_URL: &str = "https://chatgpt.com/settings/usage";

/// What kind of failure it is, with what it needs to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanUsageKind {
    /// The plan's, or this app's, usage limit is reached
    /// (`subscription_sharing_usage_limit_exceeded`, and the Business variant
    /// `subscription_sharing_workplace_individual_usage_limit_exceeded`, 429).
    /// Requests that use the plan pause. No reset time is inferred: "an
    /// app-specific limit can also apply".
    UsageLimit,
    /// Plan usage is unavailable for this user, workspace or policy
    /// (`subscription_sharing_user_not_eligible`, 403). Not retried, and no
    /// loop through sign-in.
    NotEligible,
    /// Signed in, but the grant lacks `chatgpt.tokens.use.direct`. The sign-in
    /// is kept; the person can turn plan usage on or choose another provider.
    PlanDisabled,
    /// There is no usable session: never signed in, the refresh token was
    /// refused (devkit#5 makes that likely after about an hour), the app was
    /// disconnected in ChatGPT settings, or the API rejected the token again
    /// after a refresh. `detail` says which, with no secret in it. A distinct
    /// kind so that a turn can stop on it and the window can offer the
    /// sign-in.
    SessionEnded { detail: Option<String> },
    /// Temporary: the credentials are fine and the service is not, or it asked
    /// to slow down. Retry later.
    Unavailable { detail: String },
    /// A field, tool, model or route this app sent is not allowed
    /// (`subscription_sharing_unsupported_capability`, 400, with `param`).
    /// The same request is not sent again.
    UnsupportedRequest {
        param: Option<String>,
        detail: String,
    },
    /// OpenAI refused the request for a reason that is not the person's plan
    /// or session: a policy such as the serving region (403), a grant that
    /// does not authorise the operation (`chatpass_*`), or a code of the
    /// `subscription_sharing_*` family this app does not know. `code` is
    /// OpenAI's own, when it sent one, and `detail` its message.
    Refused {
        code: Option<String>,
        detail: String,
    },
}

/// A failure of ChatGPT plan usage that stops the request: its [`kind`] and the
/// request ID of the response it came from, when the response had one.
///
/// [`kind`]: PlanUsageError::kind
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanUsageError {
    kind: PlanUsageKind,
    request_id: Option<String>,
}

impl From<PlanUsageKind> for PlanUsageError {
    fn from(kind: PlanUsageKind) -> Self {
        Self::new(kind)
    }
}

impl PlanUsageError {
    pub fn new(kind: PlanUsageKind) -> Self {
        Self {
            kind,
            request_id: None,
        }
    }

    /// The same failure, naming the request it came from. An empty or absent
    /// ID leaves it as it was; a long one is cut short.
    pub fn with_request_id(mut self, request_id: Option<&str>) -> Self {
        self.request_id = request_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| id.chars().filter(|c| !c.is_control()).take(100).collect());
        self
    }

    pub fn kind(&self) -> &PlanUsageKind {
        &self.kind
    }

    /// The `x-request-id` (or `openai-request-id`) of the response.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// A stable word for each kind, for the window to switch on.
    pub fn code(&self) -> &'static str {
        match self.kind {
            PlanUsageKind::UsageLimit => "usage_limit",
            PlanUsageKind::NotEligible => "not_eligible",
            PlanUsageKind::PlanDisabled => "plan_disabled",
            PlanUsageKind::SessionEnded { .. } => "session_ended",
            PlanUsageKind::Unavailable { .. } => "unavailable",
            PlanUsageKind::UnsupportedRequest { .. } => "unsupported_request",
            PlanUsageKind::Refused { .. } => "refused",
        }
    }

    /// Whether the same request may be sent again later without anything
    /// changing: only a temporary failure. A usage limit waits for the
    /// person; the rest need a change first.
    pub fn is_temporary(&self) -> bool {
        matches!(self.kind, PlanUsageKind::Unavailable { .. })
    }

    /// What OpenAI's `code` for a failure means, or `None` when the code is
    /// not of this route (not in the `subscription_sharing_*` or `chatpass_*`
    /// families, and not one of the two Responses codes the route's transient
    /// failures use). `param` is the request field the error is about;
    /// `message` is OpenAI's own words, kept as a diagnostic.
    pub fn from_code(
        code: &str,
        param: Option<&str>,
        message: &str,
        request_id: Option<&str>,
    ) -> Option<Self> {
        let detail = cleaned(message);
        let unavailable = |fallback: &str| PlanUsageKind::Unavailable {
            detail: if detail.is_empty() {
                fallback.to_string()
            } else {
                detail.clone()
            },
        };
        let kind = match code {
            "subscription_sharing_usage_limit_exceeded"
            | "subscription_sharing_workplace_individual_usage_limit_exceeded" => {
                PlanUsageKind::UsageLimit
            }
            "subscription_sharing_user_not_eligible" => PlanUsageKind::NotEligible,
            "subscription_sharing_usage_unavailable" | "subscription_sharing_user_unavailable" => {
                unavailable("OpenAI could not check your plan's usage")
            }
            // The Responses API's own transient codes, which the plan route
            // also uses mid-stream.
            "server_error" | "rate_limit_exceeded" => {
                unavailable("OpenAI asked to try again later")
            }
            "subscription_sharing_unsupported_capability" => PlanUsageKind::UnsupportedRequest {
                param: param.filter(|p| !p.is_empty()).map(str::to_string),
                detail,
            },
            "subscription_sharing_route_not_supported" => PlanUsageKind::UnsupportedRequest {
                param: None,
                detail: if detail.is_empty() {
                    "the route is not supported for ChatGPT plan usage".to_string()
                } else {
                    detail
                },
            },
            // "Ask the user to sign in again after confirmed revocation or a
            // terminal refresh error": the refresh was already tried.
            "subscription_sharing_invalid_user" => PlanUsageKind::SessionEnded {
                detail: Some("OpenAI could not validate the account".to_string()),
            },
            other
                if other.starts_with("subscription_sharing_") || other.starts_with("chatpass_") =>
            {
                PlanUsageKind::Refused {
                    code: Some(other.to_string()),
                    detail,
                }
            }
            _ => return None,
        };
        Some(Self::new(kind).with_request_id(request_id))
    }

    /// The failure a non-2xx answer to a request is, from its status and body
    /// (and the request ID of the response); `None` when it is none of this
    /// route's.
    ///
    /// The body is `{"error": {"code", "message", "param"}}` when the
    /// Responses layer answered, and anything (`{"detail": "..."}`) when
    /// admission did. "Treat the message as diagnostic text, not a stable
    /// machine-readable code": only a documented `code` is acted on, and
    /// otherwise the status decides, with the text kept as the diagnostic.
    pub fn from_http(status: u16, body: &str, request_id: Option<&str>) -> Option<Self> {
        let json: Option<Value> = serde_json::from_str(body).ok();
        let error = json.as_ref().and_then(|j| j.get("error"));
        let code = error
            .and_then(|e| e.get("code"))
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty());
        let param = error.and_then(|e| e.get("param")).and_then(Value::as_str);
        let message = error
            .and_then(|e| e.get("message").and_then(Value::as_str))
            .map(str::to_string)
            .or_else(|| error.and_then(Value::as_str).map(str::to_string))
            .or_else(|| {
                // `detail` is a string, or whatever else admission sent.
                json.as_ref().and_then(|j| j.get("detail")).map(|d| {
                    d.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| d.to_string())
                })
            })
            .unwrap_or_default();

        if let Some(known) = code.and_then(|c| Self::from_code(c, param, &message, request_id)) {
            return Some(known);
        }
        let detail = cleaned(&message);
        let kind = match status {
            401 => PlanUsageKind::SessionEnded {
                detail: Some("OpenAI did not accept the sign-in".to_string()),
            },
            403 => PlanUsageKind::Refused {
                code: code.map(str::to_string),
                detail,
            },
            429 => PlanUsageKind::Unavailable {
                detail: if detail.is_empty() {
                    "OpenAI asked to slow down".to_string()
                } else {
                    detail
                },
            },
            503 => PlanUsageKind::Unavailable {
                detail: if detail.is_empty() {
                    "OpenAI's plan usage route is unavailable".to_string()
                } else {
                    detail
                },
            },
            _ => return None,
        };
        Some(Self::new(kind).with_request_id(request_id))
    }
}

/// Whether an in-stream failure with this code is one to try the request
/// again for, once. Only the transient ones: every try spends the person's
/// shared allowance, and a code that says the plan, the account or the
/// request is the problem will say it again.
pub fn is_retryable_stream_code(code: &str) -> bool {
    matches!(
        code,
        "subscription_sharing_usage_unavailable"
            | "subscription_sharing_user_unavailable"
            | "server_error"
            | "rate_limit_exceeded"
    )
}

/// Whether a `401` with this body is renewed and sent again. It is when the
/// body is bare or says the subscriber could not be validated
/// (`subscription_sharing_invalid_user`: "refresh once"); a `401` that carries
/// any other code of the plan route is that code's answer, and a renewal
/// would not change it.
pub fn renews_on_401(body: &str) -> bool {
    let code = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|j| j.get("error")?.get("code")?.as_str().map(str::to_string))
        .unwrap_or_default();
    let of_the_route = code.starts_with("subscription_sharing_") || code.starts_with("chatpass_");
    !of_the_route || code == "subscription_sharing_invalid_user"
}

/// OpenAI's own words, cut short and with control characters replaced, so a
/// long or odd body cannot flood the chat. A diagnostic: nothing reads it.
fn cleaned(message: &str) -> String {
    const MAX: usize = 200;
    let flat: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    match flat.char_indices().nth(MAX) {
        Some((end, _)) => format!("{}…", &flat[..end]),
        None => flat.to_string(),
    }
}

impl std::fmt::Display for PlanUsageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UsageLimit => write!(
                f,
                "ChatGPT plan usage limit reached. edytlab has paused requests that use your \
                 ChatGPT plan, and has not switched to another provider. Open ChatGPT settings \
                 → Usage ({USAGE_URL}) to review this app's limit, or choose another provider \
                 in Settings."
            ),
            Self::NotEligible => write!(
                f,
                "Your ChatGPT account cannot use its plan in edytlab. Plan usage needs a \
                 personal Plus or Pro plan, and OpenAI says the selected user, workspace or \
                 policy does not allow it. edytlab will not try again; choose another provider \
                 in Settings."
            ),
            Self::PlanDisabled => write!(
                f,
                "You are signed in to ChatGPT, but edytlab is not allowed to use your ChatGPT \
                 plan. Turn on ChatGPT plan usage in Settings, or choose another provider."
            ),
            Self::SessionEnded { detail } => {
                write!(f, "Your ChatGPT session ended")?;
                if let Some(detail) = detail {
                    write!(f, " ({detail})")?;
                }
                write!(f, ". Continue with ChatGPT in Settings to sign in again.")
            }
            Self::Unavailable { detail } => write!(
                f,
                "ChatGPT plan usage is unavailable right now ({detail}). You are still signed \
                 in; try again in a moment."
            ),
            Self::UnsupportedRequest { param, detail } => {
                if param.as_deref() == Some("model") {
                    return write!(
                        f,
                        "The selected model is not available to your ChatGPT account. Choose \
                         another model in Settings."
                    );
                }
                write!(
                    f,
                    "OpenAI's ChatGPT plan preview does not accept part of this request"
                )?;
                if let Some(param) = param {
                    write!(f, " (`{param}`)")?;
                }
                if !detail.is_empty() {
                    write!(f, ": {detail}")?;
                }
                write!(
                    f,
                    ". That is edytlab's to fix, not yours, and it does not send the same \
                     request again. Please report it."
                )
            }
            Self::Refused { code, detail } => {
                write!(f, "OpenAI refused this ChatGPT plan request")?;
                if let Some(code) = code {
                    write!(f, " ({code})")?;
                }
                if !detail.is_empty() {
                    write!(f, ": {detail}")?;
                }
                write!(
                    f,
                    ". Check that your account and region can use ChatGPT plan usage, or \
                     choose another provider in Settings. edytlab does not retry or switch \
                     provider by itself."
                )
            }
        }
    }
}

impl std::fmt::Display for PlanUsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(id) = &self.request_id {
            write!(f, " (request ID: {id})")?;
        }
        Ok(())
    }
}

impl std::error::Error for PlanUsageError {}

impl From<BearerError> for PlanUsageError {
    /// Why the session could not hand over a bearer, as the failure the
    /// request is. Holds no token: a [`BearerError`] has none.
    fn from(error: BearerError) -> Self {
        Self::new(match error {
            BearerError::NotSignedIn => PlanUsageKind::SessionEnded {
                detail: Some("not signed in".to_string()),
            },
            BearerError::PlanDisabled => PlanUsageKind::PlanDisabled,
            BearerError::SessionEnded(reason) => PlanUsageKind::SessionEnded {
                detail: Some(reason.to_string()),
            },
            BearerError::Unavailable { reason } => PlanUsageKind::Unavailable { detail: reason },
            // The status is left out of the words: a stated 401 or 403 is
            // what the chat reads as a refused key, and this is the renewal
            // of a session, not a key.
            BearerError::Refused { status, code } => PlanUsageKind::Refused {
                code,
                detail: if status == 0 {
                    String::new()
                } else {
                    "OpenAI refused to renew the ChatGPT session".to_string()
                },
            },
            BearerError::Store(_) => PlanUsageKind::Unavailable {
                detail: "the saved ChatGPT sign-in could not be read from the keychain".to_string(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(status: u16, body: Value) -> Option<PlanUsageError> {
        PlanUsageError::from_http(status, &body.to_string(), None)
    }

    fn coded(status: u16, code: &str) -> Option<PlanUsageError> {
        http(
            status,
            serde_json::json!({ "error": { "code": code, "message": "m", "param": "tools" } }),
        )
    }

    fn kind(error: Option<PlanUsageError>) -> PlanUsageKind {
        error.expect("an error").kind().clone()
    }

    /// Every row of the errors page, by its code and status.
    #[test]
    fn every_documented_code_has_its_own_error() {
        assert_eq!(
            kind(coded(429, "subscription_sharing_usage_limit_exceeded")),
            PlanUsageKind::UsageLimit
        );
        // The Business one, which appeared within days of the launch.
        assert_eq!(
            kind(coded(
                429,
                "subscription_sharing_workplace_individual_usage_limit_exceeded"
            )),
            PlanUsageKind::UsageLimit
        );
        assert_eq!(
            kind(coded(403, "subscription_sharing_user_not_eligible")),
            PlanUsageKind::NotEligible
        );
        for code in [
            "subscription_sharing_usage_unavailable",
            "subscription_sharing_user_unavailable",
            "server_error",
            "rate_limit_exceeded",
        ] {
            assert!(
                matches!(kind(coded(503, code)), PlanUsageKind::Unavailable { .. }),
                "{code}"
            );
        }
        assert_eq!(
            kind(coded(400, "subscription_sharing_unsupported_capability")),
            PlanUsageKind::UnsupportedRequest {
                param: Some("tools".into()),
                detail: "m".into()
            }
        );
        assert!(matches!(
            kind(coded(403, "subscription_sharing_route_not_supported")),
            PlanUsageKind::UnsupportedRequest { param: None, .. }
        ));
        assert!(matches!(
            kind(coded(401, "subscription_sharing_invalid_user")),
            PlanUsageKind::SessionEnded { .. }
        ));
        for code in [
            "chatpass_v2_scope_not_authorized",
            "chatpass_v2_invalid_authorization_context",
        ] {
            assert!(
                matches!(kind(coded(403, code)), PlanUsageKind::Refused { code: Some(c), .. } if c == code),
                "{code}"
            );
        }
    }

    /// The families are mapped as families: a code the page does not list is
    /// still this route's failure, with OpenAI's own message, and it is not
    /// retried (`Refused` is not temporary).
    #[test]
    fn an_unknown_code_in_either_family_shows_the_servers_message_and_is_not_retried() {
        for code in [
            "subscription_sharing_something_new",
            "chatpass_v3_something",
            "chatpass_x",
        ] {
            let error = coded(400, code).expect(code);
            assert!(
                matches!(error.kind(), PlanUsageKind::Refused { code: Some(c), detail } if c == code && detail == "m"),
                "{code}: {error:?}"
            );
            assert!(!error.is_temporary(), "{code}");
            assert!(error.to_string().contains("m"), "{error}");
            assert!(!is_retryable_stream_code(code), "{code}");
        }
        // And a code of neither family is not this route's.
        assert_eq!(coded(400, "invalid_request_error"), None);
    }

    /// The code decides, not the status: a usage limit that arrives with an
    /// odd status is still a usage limit.
    #[test]
    fn the_code_wins_over_the_status() {
        assert_eq!(
            kind(coded(500, "subscription_sharing_usage_limit_exceeded")),
            PlanUsageKind::UsageLimit
        );
    }

    /// Admission failures before a stream carry no standard error object, and
    /// their text is not a code.
    #[test]
    fn an_admission_failure_is_read_by_its_status() {
        let detail = serde_json::json!({ "detail": "region not permitted" });
        assert!(matches!(
            kind(http(401, detail.clone())),
            PlanUsageKind::SessionEnded { .. }
        ));
        assert!(
            matches!(kind(http(403, detail.clone())), PlanUsageKind::Refused { code: None, detail } if detail == "region not permitted")
        );
        assert!(matches!(
            kind(http(503, detail.clone())),
            PlanUsageKind::Unavailable { .. }
        ));
        assert!(matches!(
            kind(http(429, serde_json::json!({}))),
            PlanUsageKind::Unavailable { .. }
        ));
        // A `detail` that is not a string is text too.
        let list = serde_json::json!({ "detail": [{ "msg": "bad" }] });
        assert!(
            matches!(kind(http(403, list)), PlanUsageKind::Refused { detail, .. } if detail.contains("bad"))
        );
        // A body that is not JSON at all.
        assert!(matches!(
            kind(PlanUsageError::from_http(
                503,
                "upstream connect error",
                None
            )),
            PlanUsageKind::Unavailable { .. }
        ));
        // Anything else is not this route's to name.
        assert_eq!(http(400, detail.clone()), None);
        assert_eq!(http(404, detail.clone()), None);
        assert_eq!(http(500, detail), None);
    }

    #[test]
    fn the_request_id_is_kept_and_shown() {
        let body =
            serde_json::json!({ "error": { "code": "subscription_sharing_user_not_eligible" } });
        let error = PlanUsageError::from_http(403, &body.to_string(), Some("req_abc123")).unwrap();
        assert_eq!(error.request_id(), Some("req_abc123"));
        assert!(
            error.to_string().ends_with("(request ID: req_abc123)"),
            "{error}"
        );
        // None, empty, odd and long ones.
        let bare = PlanUsageError::from_http(403, &body.to_string(), None).unwrap();
        assert_eq!(bare.request_id(), None);
        assert!(!bare.to_string().contains("request ID"));
        assert_eq!(bare.clone().with_request_id(Some("  ")).request_id(), None);
        let long = "x".repeat(300);
        assert_eq!(
            bare.clone()
                .with_request_id(Some(&long))
                .request_id()
                .unwrap()
                .len(),
            100
        );
        assert_eq!(
            bare.with_request_id(Some("a\nb")).request_id(),
            Some("ab"),
            "control characters never reach the chat"
        );
    }

    /// Only the transient codes are tried again, and only the ones that say so.
    #[test]
    fn the_retryable_stream_codes_are_the_transient_ones() {
        for code in [
            "subscription_sharing_usage_unavailable",
            "subscription_sharing_user_unavailable",
            "server_error",
            "rate_limit_exceeded",
        ] {
            assert!(is_retryable_stream_code(code), "{code}");
            assert!(
                PlanUsageError::from_code(code, None, "", None)
                    .unwrap()
                    .is_temporary(),
                "{code}: a retry that fails again is shown as temporary"
            );
        }
        for code in [
            "subscription_sharing_usage_limit_exceeded",
            "subscription_sharing_workplace_individual_usage_limit_exceeded",
            "subscription_sharing_user_not_eligible",
            "subscription_sharing_unsupported_capability",
            "subscription_sharing_invalid_user",
            "chatpass_v2_scope_not_authorized",
            "context_length_exceeded",
            "",
        ] {
            assert!(!is_retryable_stream_code(code), "{code}");
        }
    }

    /// A bare 401 is renewed; a 401 that carries a plan code is that code's
    /// answer (except the one that asks for exactly one renewal).
    #[test]
    fn a_401_is_renewed_unless_it_carries_a_plan_code() {
        let with = |code: &str| serde_json::json!({ "error": { "code": code } }).to_string();
        assert!(renews_on_401(""));
        assert!(renews_on_401("Unauthorized"));
        assert!(renews_on_401(
            &serde_json::json!({ "detail": "no" }).to_string()
        ));
        assert!(renews_on_401(&with("invalid_token")));
        assert!(renews_on_401(&with("subscription_sharing_invalid_user")));
        for code in [
            "subscription_sharing_user_not_eligible",
            "subscription_sharing_usage_limit_exceeded",
            "chatpass_v2_scope_not_authorized",
            "subscription_sharing_something_new",
        ] {
            assert!(!renews_on_401(&with(code)), "{code}");
        }
    }

    #[test]
    fn a_session_that_cannot_give_a_bearer_is_a_plan_usage_error() {
        use super::super::session::EndReason;
        assert_eq!(
            PlanUsageError::from(BearerError::PlanDisabled).kind(),
            &PlanUsageKind::PlanDisabled
        );
        for error in [
            BearerError::NotSignedIn,
            BearerError::SessionEnded(EndReason::NotStored),
            BearerError::SessionEnded(EndReason::RefreshTokenUnusable {
                code: "invalid_grant".into(),
            }),
        ] {
            assert_eq!(
                PlanUsageError::from(error).code(),
                "session_ended",
                "a session that is gone is a sign-in away"
            );
        }
        assert!(PlanUsageError::from(BearerError::Unavailable {
            reason: "the network".into()
        })
        .is_temporary());
    }

    /// Codes are what the window switches on, so they are fixed and distinct.
    #[test]
    fn the_codes_are_distinct_and_stable() {
        let all = [
            PlanUsageKind::UsageLimit,
            PlanUsageKind::NotEligible,
            PlanUsageKind::PlanDisabled,
            PlanUsageKind::SessionEnded { detail: None },
            PlanUsageKind::Unavailable { detail: "x".into() },
            PlanUsageKind::UnsupportedRequest {
                param: None,
                detail: String::new(),
            },
            PlanUsageKind::Refused {
                code: None,
                detail: String::new(),
            },
        ];
        let codes: Vec<_> = all
            .into_iter()
            .map(|k| PlanUsageError::new(k).code())
            .collect();
        assert_eq!(
            codes,
            [
                "usage_limit",
                "not_eligible",
                "plan_disabled",
                "session_ended",
                "unavailable",
                "unsupported_request",
                "refused"
            ]
        );
    }

    /// The chat offers Settings for text that reads as a refused key
    /// (`chat-errors.ts`: "api key", "authenticat", a stated 401 or 403), and
    /// none of these is that. Each says what to do, and the usage limit says
    /// where to look and that nothing switched.
    #[test]
    fn the_texts_are_not_read_as_a_key_problem_and_say_what_to_do() {
        let all = [
            PlanUsageError::new(PlanUsageKind::UsageLimit),
            PlanUsageError::new(PlanUsageKind::NotEligible),
            PlanUsageError::new(PlanUsageKind::PlanDisabled),
            PlanUsageError::new(PlanUsageKind::SessionEnded {
                detail: Some("the refresh token was refused (invalid_grant)".into()),
            }),
            PlanUsageError::new(PlanUsageKind::Unavailable {
                detail: "OpenAI asked to slow down".into(),
            })
            .with_request_id(Some("req_1")),
            PlanUsageError::new(PlanUsageKind::UnsupportedRequest {
                param: Some("tools".into()),
                detail: "no".into(),
            }),
            PlanUsageError::new(PlanUsageKind::UnsupportedRequest {
                param: Some("model".into()),
                detail: "no".into(),
            }),
            PlanUsageError::new(PlanUsageKind::Refused {
                code: Some("chatpass_v2_scope_not_authorized".into()),
                detail: String::new(),
            }),
            // A renewal the issuer answered 401 to (`invalid_client`): the
            // status is not in the text.
            PlanUsageError::from(BearerError::Refused {
                status: 401,
                code: Some("invalid_client".into()),
            }),
        ];
        for error in &all {
            let text = error.to_string();
            let lower = text.to_lowercase();
            assert!(
                !lower.contains("api key") && !lower.contains("api-key"),
                "{text}"
            );
            assert!(!lower.contains("authenticat"), "{text}");
            assert!(!lower.contains("anthropic"), "{text}");
            for status in ["401", "403"] {
                assert!(!text.contains(status), "{text}");
            }
            assert!(
                lower.contains("settings")
                    || lower.contains("try again")
                    || lower.contains("report"),
                "{error:?} says nothing to do: {text}"
            );
        }
        let limit = PlanUsageError::new(PlanUsageKind::UsageLimit).to_string();
        assert!(limit.contains(USAGE_URL), "{limit}");
        assert!(limit.contains("not switched"), "{limit}");
        // No reset time is promised.
        assert!(!limit.to_lowercase().contains("reset"), "{limit}");
    }

    #[test]
    fn openais_words_are_cut_short_and_flat() {
        let long = "x".repeat(500);
        let e =
            PlanUsageError::from_code("subscription_sharing_usage_unavailable", None, &long, None)
                .unwrap();
        let PlanUsageKind::Unavailable { detail } = e.kind().clone() else {
            panic!("unavailable")
        };
        assert!(detail.chars().count() <= 201, "{}", detail.chars().count());
        let e = PlanUsageError::from_code(
            "subscription_sharing_usage_unavailable",
            None,
            "a\nb\u{7}c",
            None,
        )
        .unwrap();
        let PlanUsageKind::Unavailable { detail } = e.kind().clone() else {
            panic!("unavailable")
        };
        assert_eq!(detail, "a b c");
    }
}
