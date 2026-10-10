//! Telling "the model's context is too small" apart from any other failed
//! request (#395).
//!
//! A local model server refuses a request that does not fit its context
//! with a 400 whose body is the server's own log line:
//!
//! ```text
//! request (15157 tokens) exceeds the available context size (8192 tokens), try increasing it
//! ```
//!
//! That reached the chat as "the model provider returned an error (400)"
//! followed by the raw JSON, which tells a person nothing they can act on.
//! [`api_error`] reads the body and, when it is this error, returns
//! [`Error::ContextTooSmall`] with the numbers where the server gave them.
//! Every other non-2xx stays [`Error::Api`], unchanged.
//!
//! Only a 400 or a 413 is looked at. A 500 that happens to mention a
//! context length is the server failing, not the request being too big,
//! and a 401 is a key problem whatever its body says.

use serde_json::Value;

use crate::Error;

/// Phrases, lowercased, that providers use for an over-long request when
/// they do not also set a machine-readable code.
const OVERFLOW_PHRASES: &[&str] = &[
    // llama.cpp: "request (N tokens) exceeds the available context size (M tokens)".
    "exceeds the available context size",
    // OpenAI, Groq and OpenRouter: "This model's maximum context length is ...".
    "maximum context length",
    // Anthropic: "prompt is too long: N tokens > M maximum".
    "prompt is too long",
    // Gemini: "The input token count exceeds the maximum number of tokens allowed".
    "exceeds the maximum number of tokens allowed",
];

/// The error for a non-2xx answer: [`Error::ContextTooSmall`] when the
/// body says the request did not fit, [`Error::Api`] otherwise.
pub(crate) fn api_error(status: u16, body: String) -> Error {
    match overflow(status, &body) {
        Some((needed, available)) => {
            // The person is shown an explanation, not the server's body
            // (see `Error::ContextTooSmall`), so this is where the body
            // is kept for whoever has to find out what the server said.
            tracing::warn!(
                status,
                needed = ?needed,
                available = ?available,
                body = %body,
                "the model's context window is too small for the request"
            );
            Error::ContextTooSmall {
                needed,
                available,
                message: body,
            }
        }
        None => Error::Api {
            status,
            message: body,
        },
    }
}

/// `Some((needed, available))` when `status` and `body` are a context
/// overflow, each count `None` where the server did not give it.
fn overflow(status: u16, body: &str) -> Option<(Option<u64>, Option<u64>)> {
    if !matches!(status, 400 | 413) {
        return None;
    }
    let json: Option<Value> = serde_json::from_str(body).ok();
    let error = json.as_ref().and_then(|v| v.get("error"));
    let field = |name: &str| error.and_then(|e| e.get(name));

    // llama.cpp sets a type; OpenAI and the providers that copy it set a
    // code. Either is exact, so neither needs the wording to agree.
    let by_type = field("type").and_then(Value::as_str) == Some("exceed_context_size_error");
    let by_code = field("code").and_then(Value::as_str) == Some("context_length_exceeded");
    let lowered = body.to_lowercase();
    let by_wording = OVERFLOW_PHRASES.iter().any(|p| lowered.contains(p));
    if !(by_type || by_code || by_wording) {
        return None;
    }

    let (parsed_needed, parsed_available) = counts_in_text(&lowered);
    let needed = field("n_prompt_tokens")
        .and_then(Value::as_u64)
        .or(parsed_needed);
    let available = field("n_ctx").and_then(Value::as_u64).or(parsed_available);
    Some((needed, available))
}

/// The two counts in llama.cpp's sentence, "(N tokens) exceeds the
/// available context size (M tokens)", for a server that gives the
/// sentence but not the fields. Anything else gives `None`s: guessing at
/// other providers' prose would put wrong numbers in front of a person.
fn counts_in_text(lowered: &str) -> (Option<u64>, Option<u64>) {
    const MARKER: &str = "exceeds the available context size";
    let Some(at) = lowered.find(MARKER) else {
        return (None, None);
    };
    let needed = numbers_before_tokens(&lowered[..at]).last().copied();
    let available = numbers_before_tokens(&lowered[at + MARKER.len()..])
        .first()
        .copied();
    (needed, available)
}

/// Every number written immediately before the word "tokens", in order.
fn numbers_before_tokens(text: &str) -> Vec<u64> {
    text.match_indices(" tokens")
        .filter_map(|(end, _)| {
            let head = &text[..end];
            let digits = head.bytes().rev().take_while(u8::is_ascii_digit).count();
            head[head.len() - digits..].parse().ok()
        })
        .collect()
}

/// What the person is shown for [`Error::ContextTooSmall`].
///
/// It names no hosted provider (#405) and none of the credential wording
/// the chat reads as "fix this in Settings" (#414): the fix is a bigger
/// context, not a different key. It does name the two local servers the
/// Ollama provider is pointed at, because how to give a local model more
/// context is a setting of that server and nowhere else. The two ways to
/// fit a request that is already too long are the two things the app can
/// do today: reopen the project, which builds the agent afresh with an
/// empty conversation (`rebuild_agent` runs on opening a project, and
/// there is no new-chat or clear-conversation control), and choose a
/// model with a larger context, which rebuilds it the same way.
pub(crate) fn explain(needed: &Option<u64>, available: &Option<u64>) -> String {
    let counts = match (needed, available) {
        (Some(n), Some(a)) => format!(" (it needs about {n} tokens; the model has {a})"),
        (Some(n), None) => format!(" (it needs about {n} tokens)"),
        (None, Some(a)) => format!(" (the model has {a} tokens)"),
        (None, None) => String::new(),
    };
    format!(
        "The model's context window is too small for this request{counts}. Reopen the project \
         to drop the earlier messages, or choose a model with a larger context. For a local \
         model, give it more: with Ollama, raise its context length; with llama.cpp, start the \
         server with a larger --ctx-size."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The body from the issue, as llama-server sent it.
    const LLAMA_CPP: &str = r#"{"error":{"code":400,"message":"request (15157 tokens) exceeds the available context size (8192 tokens), try increasing it","type":"exceed_context_size_error","n_prompt_tokens":15157,"n_ctx":8192}}"#;

    fn context_error(e: Error) -> (Option<u64>, Option<u64>) {
        match e {
            Error::ContextTooSmall {
                needed, available, ..
            } => (needed, available),
            other => panic!("expected ContextTooSmall, got {other:?}"),
        }
    }

    #[test]
    fn llama_cpp_gives_both_counts_and_the_text_says_them() {
        let err = api_error(400, LLAMA_CPP.into());
        let text = err.to_string();
        assert_eq!(context_error(err), (Some(15157), Some(8192)));
        assert!(text.contains("15157") && text.contains("8192"), "{text}");
    }

    #[test]
    fn the_counts_are_read_from_the_sentence_when_the_fields_are_absent() {
        let body = r#"{"error":{"message":"request (15157 tokens) exceeds the available context size (8192 tokens), try increasing it"}}"#;
        assert_eq!(
            context_error(api_error(400, body.into())),
            (Some(15157), Some(8192))
        );
    }

    #[test]
    fn a_body_that_is_not_json_is_still_recognised_by_its_wording() {
        let body = "request (900 tokens) exceeds the available context size (512 tokens)";
        assert_eq!(
            context_error(api_error(400, body.into())),
            (Some(900), Some(512))
        );
    }

    #[test]
    fn openai_code_is_recognised_without_counts() {
        let body = r#"{"error":{"message":"Too long.","type":"invalid_request_error","code":"context_length_exceeded"}}"#;
        assert_eq!(
            context_error(api_error(400, body.into())),
            (None, None),
            "other providers' prose is not parsed for numbers"
        );
    }

    #[test]
    fn the_other_providers_wordings_are_recognised() {
        for body in [
            r#"{"error":{"message":"This model's maximum context length is 8192 tokens. However, your messages resulted in 9000 tokens."}}"#,
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 250000 tokens > 200000 maximum"}}"#,
            r#"{"error":{"code":400,"message":"The input token count (2000000) exceeds the maximum number of tokens allowed (1048575).","status":"INVALID_ARGUMENT"}}"#,
        ] {
            assert!(
                matches!(api_error(400, body.into()), Error::ContextTooSmall { .. }),
                "{body}"
            );
        }
    }

    #[test]
    fn a_413_counts_too() {
        assert!(matches!(
            api_error(413, LLAMA_CPP.into()),
            Error::ContextTooSmall { .. }
        ));
    }

    #[test]
    fn an_unrelated_400_stays_an_api_error() {
        let body = r#"{"error":{"message":"model 'nope' not found","type":"not_found"}}"#;
        match api_error(400, body.into()) {
            Error::Api {
                status: 400,
                message,
            } => assert_eq!(message, body),
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn a_401_stays_an_api_error_whatever_the_body_says() {
        assert!(matches!(
            api_error(401, LLAMA_CPP.into()),
            Error::Api { status: 401, .. }
        ));
    }

    #[test]
    fn a_500_that_mentions_a_context_length_stays_an_api_error() {
        let body = "internal error: maximum context length handling failed";
        assert!(matches!(
            api_error(500, body.into()),
            Error::Api { status: 500, .. }
        ));
    }

    #[test]
    fn the_text_names_no_provider_and_no_credential() {
        for (n, a) in [
            (Some(1), Some(2)),
            (Some(1), None),
            (None, Some(2)),
            (None, None),
        ] {
            let text = explain(&n, &a).to_lowercase();
            assert!(text.contains("context"), "{text}");
            for banned in ["anthropic", "api key", "api-key", "authenticat", "settings"] {
                assert!(!text.contains(banned), "{banned:?} in {text}");
            }
        }
    }

    /// The app has no new-chat or clear-conversation control: the agent
    /// (and with it the conversation) is built afresh by `rebuild_agent`,
    /// which runs when a project is opened or the model is changed. The
    /// text must send a person to one of those, not to a button that is
    /// not there.
    #[test]
    fn the_text_offers_only_what_the_app_can_do() {
        let text = explain(&Some(15157), &Some(8192));
        assert!(text.contains("Reopen the project"), "{text}");
        assert!(text.contains("a model with a larger context"), "{text}");
        for not_there in ["new chat", "clear the chat", "clear the conversation"] {
            assert!(
                !text.to_lowercase().contains(not_there),
                "{not_there:?} in {text}"
            );
        }
    }

    #[test]
    fn counts_are_left_out_of_the_text_when_unknown() {
        let text = explain(&None, &None);
        assert!(!text.contains('('), "{text}");
    }
}
