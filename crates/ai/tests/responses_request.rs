//! The request body of the Responses wire format (#474).
//!
//! Sign in with ChatGPT's plan route takes `POST /v1/responses` and no other
//! shape, and it is strict about what a body carries: `store: false` and
//! `stream: true` always, the system prompt in `instructions`, an `input`
//! array, function tools in a namespace, and none of a list of fields the
//! preview refuses (<https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations>).
//! These pin the body `ai::responses` builds from the canonical request, so
//! that a change to any of it is one failing test and not a 400 from a
//! signed-in user's first message.

use ai::anthropic::{
    ContentBlock, Effort, Message, MessagesRequest, OutputConfig, Role, SystemBlock, ToolChoice,
};
use ai::responses::{request_body, ToolGrouping, UNSUPPORTED_FIELDS};
use serde_json::{json, Value};

const NAMESPACE: &str = "edytlab";

fn grouped() -> ToolGrouping {
    ToolGrouping::Namespace(NAMESPACE.to_string())
}

fn user(text: &str) -> Message {
    Message {
        role: Role::User,
        content: vec![ContentBlock::Text { text: text.into() }],
    }
}

/// A conversation with every block kind: a user turn, an assistant turn that
/// speaks and calls a tool (after thinking), and the user turn that answers
/// the call.
fn conversation() -> Vec<Message> {
    vec![
        user("make the drums louder"),
        Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    thinking: "hmm".into(),
                    signature: "SIG".into(),
                },
                ContentBlock::Text {
                    text: "Raising them.".into(),
                },
                ContentBlock::ToolUse {
                    id: "call_abc".into(),
                    name: "set_track_gain".into(),
                    input: json!({ "track": 1, "gain_db": 3.0 }),
                },
            ],
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call_abc".into(),
                content: "{\"ok\":true}".into(),
                is_error: None,
            }],
        },
    ]
}

fn tools() -> Value {
    json!([
        {
            "name": "set_track_gain",
            "description": "Set a track's gain in dB.",
            "input_schema": {
                "type": "object",
                "properties": { "track": { "type": "integer" }, "gain_db": { "type": "number" } },
                "required": ["track", "gain_db"]
            }
        },
        {
            "name": "normalize",
            "description": "Normalize a track.",
            "input_schema": { "type": "object", "properties": {} },
            // What the agent loop puts on the last tool for Anthropic's cache.
            "cache_control": { "type": "ephemeral" }
        }
    ])
}

/// Everything the canonical request can carry, set: the case where a field
/// that should not go out would.
fn body_of(messages: &[Message], grouping: &ToolGrouping) -> Value {
    let request = MessagesRequest {
        model: "gpt-test",
        max_tokens: 32_000,
        system: vec![
            SystemBlock {
                kind: "text",
                text: "You are an audio editor.",
                cache_control: Some(ai::anthropic::CacheControl::EPHEMERAL),
            },
            SystemBlock {
                kind: "text",
                text: "Use the tools.",
                cache_control: None,
            },
        ],
        messages,
        tools: Some(tools()),
        tool_choice: Some(ToolChoice::AUTO),
        // A caller that asked for a plain reply: the body is streamed anyway.
        stream: false,
        output_config: Some(OutputConfig::effort(Effort::High)),
    };
    request_body(&request, grouping)
}

fn input(body: &Value) -> &Vec<Value> {
    body["input"].as_array().expect("input is an array")
}

#[test]
fn the_body_is_never_stored_and_always_streamed() {
    let body = body_of(&conversation(), &grouped());
    assert_eq!(body["store"], json!(false));
    assert_eq!(body["stream"], json!(true));
    assert_eq!(body["model"], "gpt-test");
}

/// The list is written once, in `UNSUPPORTED_FIELDS`; this checks the body
/// against it, and against the other direction too: nothing but the fields
/// the format knows may be at the top level, so a field another wire format
/// uses (`max_tokens`, `system`, `output_config`) cannot slip in either.
#[test]
fn none_of_the_previews_unsupported_fields_is_sent() {
    assert!(!UNSUPPORTED_FIELDS.is_empty());
    for body in [
        body_of(&conversation(), &grouped()),
        body_of(&conversation(), &ToolGrouping::Flat),
        body_of(&[user("hi")], &grouped()),
    ] {
        let object = body.as_object().expect("a JSON object");
        for &field in UNSUPPORTED_FIELDS {
            assert!(
                !object.contains_key(field),
                "`{field}` is refused by the preview but was sent: {body}"
            );
        }
        let allowed = [
            "model",
            "instructions",
            "input",
            "tools",
            "tool_choice",
            "store",
            "stream",
        ];
        for key in object.keys() {
            assert!(
                allowed.contains(&key.as_str()),
                "`{key}` is not a field of this format's body: {body}"
            );
        }
    }
}

/// The list is the one on the preview page, name for name: fifteen fields
/// from "Unsupported fields" and `previous_response_id` from "Conversation
/// state". This is the one deliberate second copy, so that a change to the
/// constant, or to the page, is a test failure to look at.
#[test]
fn the_unsupported_list_is_the_one_the_preview_page_gives() {
    let page = [
        "background",
        "conversation",
        "max_output_tokens",
        "max_tool_calls",
        "metadata",
        "moderation",
        "multi_agent",
        "prompt",
        "prompt_cache_retention",
        "safety_identifier",
        "temperature",
        "top_logprobs",
        "top_p",
        "truncation",
        "user",
        "previous_response_id",
    ];
    let mut ours = UNSUPPORTED_FIELDS.to_vec();
    ours.sort_unstable();
    let mut theirs = page.to_vec();
    theirs.sort_unstable();
    assert_eq!(ours, theirs);
}

#[test]
fn the_system_prompt_is_the_instructions_and_never_an_input_item() {
    let body = body_of(&conversation(), &grouped());
    assert_eq!(
        body["instructions"],
        "You are an audio editor.\n\nUse the tools."
    );
    for item in input(&body) {
        assert_ne!(item["role"], "system", "a system item is rejected: {item}");
    }
    assert!(
        !body.to_string().contains("\"system\""),
        "no system field or role anywhere: {body}"
    );
}

#[test]
fn a_request_with_no_system_prompt_has_no_instructions() {
    let messages = [user("hi")];
    let request = MessagesRequest {
        model: "m",
        max_tokens: 1,
        system: Vec::new(),
        messages: &messages,
        tools: None,
        tool_choice: None,
        stream: true,
        output_config: None,
    };
    let body = request_body(&request, &grouped());
    assert!(body.get("instructions").is_none(), "{body}");
    assert!(body.get("tools").is_none(), "{body}");
    assert!(body.get("tool_choice").is_none(), "{body}");
    assert_eq!(
        body["input"],
        json!([{
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": "hi" }]
        }])
    );
}

#[test]
fn function_tools_are_grouped_in_one_namespace_with_the_name_given() {
    let body = body_of(&conversation(), &grouped());
    let tools = body["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 1, "one namespace holds them all: {body}");
    let namespace = &tools[0];
    assert_eq!(namespace["type"], "namespace");
    assert_eq!(namespace["name"], NAMESPACE);
    assert!(
        namespace["description"]
            .as_str()
            .is_some_and(|d| !d.is_empty()),
        "a namespace is described: {namespace}"
    );

    let inner = namespace["tools"]
        .as_array()
        .expect("the namespace's tools");
    assert_eq!(inner.len(), 2);
    assert_eq!(inner[0]["type"], "function");
    assert_eq!(inner[0]["name"], "set_track_gain");
    assert_eq!(inner[0]["description"], "Set a track's gain in dB.");
    assert_eq!(
        inner[0]["parameters"],
        tools_schema("set_track_gain"),
        "the input schema is the parameters, as it was"
    );
    // The schemas are not strict-shaped, and the API would otherwise try to
    // make them so (function-calling guide, "Strict mode").
    assert_eq!(inner[0]["strict"], json!(false));
    assert_eq!(inner[1]["name"], "normalize");

    let namespace_name_is_a_parameter = body_of(
        &conversation(),
        &ToolGrouping::Namespace("another_name".into()),
    );
    assert_eq!(
        namespace_name_is_a_parameter["tools"][0]["name"],
        "another_name"
    );
}

fn tools_schema(name: &str) -> Value {
    tools()
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == name)
        .unwrap()["input_schema"]
        .clone()
}

#[test]
fn anthropics_cache_marker_never_reaches_the_body() {
    for grouping in [grouped(), ToolGrouping::Flat] {
        let text = body_of(&conversation(), &grouping).to_string();
        assert!(!text.contains("cache_control"), "{text}");
    }
}

/// The grouping is a parameter. `Flat` is the ordinary Responses shape, for
/// a provider on the API-key endpoint; it is not a fallback on the plan
/// route, which allows only namespaces or `additional_tools` input items.
/// With it nothing mentions a namespace, not even the calls echoed back in
/// `input`.
#[test]
fn flat_grouping_has_no_namespace_anywhere() {
    let body = body_of(&conversation(), &ToolGrouping::Flat);
    let tools = body["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 2);
    assert!(tools.iter().all(|t| t["type"] == "function"), "{tools:?}");
    assert!(
        !body.to_string().contains("namespace"),
        "no namespace anywhere: {body}"
    );
}

#[test]
fn a_tool_result_round_trips_as_a_function_call_and_its_output() {
    let body = body_of(&conversation(), &grouped());
    let items = input(&body);
    assert_eq!(items.len(), 4, "{items:?}");

    assert_eq!(
        items[0],
        json!({
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": "make the drums louder" }]
        })
    );
    // Assistant text is `output_text`, and the thinking block is gone.
    assert_eq!(
        items[1],
        json!({
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "Raising them." }]
        })
    );
    assert_eq!(items[2]["type"], "function_call");
    assert_eq!(items[2]["call_id"], "call_abc");
    assert_eq!(items[2]["name"], "set_track_gain");
    // The arguments are a JSON *string*, as the API sends them.
    let arguments: Value =
        serde_json::from_str(items[2]["arguments"].as_str().expect("a string")).unwrap();
    assert_eq!(arguments, json!({ "track": 1, "gain_db": 3.0 }));
    // The call came out of the namespace, so it is echoed with it.
    assert_eq!(items[2]["namespace"], NAMESPACE);

    assert_eq!(
        items[3],
        json!({
            "type": "function_call_output",
            "call_id": "call_abc",
            "output": "{\"ok\":true}"
        })
    );
    assert_eq!(items[2]["call_id"], items[3]["call_id"]);
    assert!(
        !body.to_string().contains("SIG"),
        "thinking is not replayed"
    );
}

#[test]
fn a_failed_tool_result_says_so_in_its_output() {
    let messages = [Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "call_x".into(),
            content: "no such track".into(),
            is_error: Some(true),
        }],
    }];
    let body = body_of(&messages, &grouped());
    assert_eq!(
        input(&body)[0],
        json!({
            "type": "function_call_output",
            "call_id": "call_x",
            "output": "error: no such track"
        })
    );
}

/// The results lead the user's own words, which is where the agent loop
/// puts them (`approval::push_user_text`), and every call is answered in
/// the order it was made.
#[test]
fn results_come_before_the_users_next_words_and_calls_keep_their_order() {
    let messages = [
        Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text { text: "a".into() },
                ContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "one".into(),
                    input: json!({}),
                },
                ContentBlock::Text { text: "b".into() },
                ContentBlock::ToolUse {
                    id: "c2".into(),
                    name: "two".into(),
                    input: json!({}),
                },
            ],
        },
        Message {
            role: Role::User,
            content: vec![
                ContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: "r1".into(),
                    is_error: None,
                },
                ContentBlock::ToolResult {
                    tool_use_id: "c2".into(),
                    content: "r2".into(),
                    is_error: None,
                },
                ContentBlock::Text {
                    text: "now louder".into(),
                },
            ],
        },
    ];
    let body = body_of(&messages, &grouped());
    let kinds: Vec<(String, String)> = input(&body)
        .iter()
        .map(|i| {
            let key = i
                .get("call_id")
                .or_else(|| i["content"][0].get("text"))
                .and_then(Value::as_str)
                .unwrap()
                .to_string();
            (i["type"].as_str().unwrap().to_string(), key)
        })
        .collect();
    let expected = [
        ("message", "a"),
        ("function_call", "c1"),
        ("message", "b"),
        ("function_call", "c2"),
        ("function_call_output", "c1"),
        ("function_call_output", "c2"),
        ("message", "now louder"),
    ];
    let expected: Vec<(String, String)> = expected
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    assert_eq!(kinds, expected);
}

#[test]
fn tool_choice_follows_the_request_and_is_only_sent_with_tools() {
    let messages = [user("hi")];
    let with_choice = |tools: Option<Value>, choice: Option<ToolChoice>| {
        let request = MessagesRequest {
            model: "m",
            max_tokens: 1,
            system: Vec::new(),
            messages: &messages,
            tools,
            tool_choice: choice,
            stream: true,
            output_config: None,
        };
        request_body(&request, &grouped())
    };
    assert_eq!(
        with_choice(Some(tools()), Some(ToolChoice::AUTO))["tool_choice"],
        "auto"
    );
    assert_eq!(
        with_choice(Some(tools()), Some(ToolChoice::NONE))["tool_choice"],
        "none"
    );
    // No tool to offer is no tool_choice either (the API rejects an empty
    // `tools`); the request after the tool budget is spent is the `"none"`
    // above, with the tools still there.
    assert!(with_choice(Some(json!([])), Some(ToolChoice::AUTO))
        .get("tool_choice")
        .is_none());
    assert!(with_choice(None, Some(ToolChoice::AUTO))
        .get("tool_choice")
        .is_none());
}
