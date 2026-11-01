//! Conversation history and ordered completion of an assistant's tool step.
//!
//! History is read-only outside this module. The loop decides when a request
//! prefix changes, which tools run, and what the user sees.

use crate::anthropic::{ContentBlock, Message, Role};

#[derive(Default)]
pub(crate) struct Conversation {
    messages: Vec<Message>,
}

impl Conversation {
    pub(crate) fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub(crate) fn reset(&mut self) {
        self.messages.clear();
    }

    /// Join a trailing user message, including one holding tool results.
    pub(crate) fn push_user_text(&mut self, text: String) {
        if let Some(last) = self.messages.last_mut() {
            if last.role == Role::User {
                match last.content.last_mut() {
                    Some(ContentBlock::Text { text: previous }) => {
                        previous.push_str("\n\n");
                        previous.push_str(&text);
                    }
                    _ => last.content.push(ContentBlock::Text { text }),
                }
                return;
            }
        }
        self.messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text }],
        });
    }

    /// Withdraw only the exact final text or the suffix previously joined.
    pub(crate) fn withdraw_user_text(&mut self, text: &str) -> bool {
        let Some(last) = self.messages.last_mut() else {
            return false;
        };
        if last.role != Role::User {
            return false;
        }
        match last.content.last() {
            Some(ContentBlock::Text { text: t }) if t == text => {
                last.content.pop();
            }
            Some(ContentBlock::Text { text: t }) => {
                let Some(rest) = t.strip_suffix(text).and_then(|t| t.strip_suffix("\n\n")) else {
                    return false;
                };
                let rest = rest.to_string();
                if let Some(ContentBlock::Text { text: t }) = last.content.last_mut() {
                    *t = rest;
                }
            }
            _ => return false,
        }
        if last.content.is_empty() {
            self.messages.pop();
        }
        true
    }

    /// Remove both thinking kinds before replay under a changed prefix (#468).
    ///
    /// Signatures bind to system, tools and preceding messages. The loop calls
    /// this at turn start and before provider adaptation changes the tool set;
    /// within a turn, stored blocks and their signatures remain unchanged.
    pub(crate) fn invalidate_thinking(&mut self) {
        for message in &mut self.messages {
            message.content.retain(|block| !is_thinking(block));
        }
    }

    /// Thinking alone (including an empty response) is not a replayable reply.
    pub(crate) fn store_assistant(&mut self, content: Vec<ContentBlock>) {
        if content.iter().any(|block| !is_thinking(block)) {
            self.messages.push(Message {
                role: Role::Assistant,
                content,
            });
        }
    }

    /// Complete the calls of the stored assistant step in their original order.
    pub(crate) fn pending_step(
        &mut self,
        call_ids: impl IntoIterator<Item = String>,
    ) -> PendingStep<'_> {
        PendingStep {
            conversation: self,
            call_ids: call_ids.into_iter().collect(),
            answers: Vec::new(),
        }
    }
}

fn is_thinking(block: &ContentBlock) -> bool {
    matches!(
        block,
        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. }
    )
}

/// What a call is told when the loop finished its step without a result for it.
const UNANSWERED: &str = "No result was recorded for this call. It may not have run.";

pub(crate) struct PendingStep<'a> {
    conversation: &'a mut Conversation,
    call_ids: Vec<String>,
    answers: Vec<ContentBlock>,
}

impl<'history> PendingStep<'history> {
    /// Record the next call's result. Release builds abort on a panic, so an
    /// answer past the last call is a bug that debug builds assert on and
    /// release builds ignore: history stays valid either way.
    pub(crate) fn answer(&mut self, content: String, is_error: Option<bool>) {
        let Some(id) = self.call_ids.get(self.answers.len()) else {
            debug_assert!(false, "one answer per call");
            return;
        };
        self.answers.push(ContentBlock::ToolResult {
            tool_use_id: id.clone(),
            content,
            is_error,
        });
    }

    pub(crate) fn unanswered_ids(&self) -> impl Iterator<Item = &str> {
        self.call_ids[self.answers.len()..]
            .iter()
            .map(String::as_str)
    }

    /// Store the answers; every call is expected to have one. In a debug
    /// build a missing answer is a failure. In a release build (`panic =
    /// "abort"`) any call left unanswered gets an error result, so the
    /// history still pairs every `tool_use` with a `tool_result`.
    pub(crate) fn finish(mut self) {
        debug_assert_eq!(self.answers.len(), self.call_ids.len(), "answer every call");
        self.answer_remaining(UNANSWERED);
        self.store_answers(None);
    }

    pub(crate) fn finish_not_run(mut self, content: &str, guidance: Option<String>) {
        self.answer_remaining(content);
        self.store_answers(guidance);
    }

    /// Arm history-only cleanup solely across the actual held approval await.
    /// Dispatch and callbacks have no generic panic recovery: an unstored
    /// result does not imply that its tool never ran.
    pub(crate) fn awaiting_approval(self) -> ApprovalWait<'history> {
        ApprovalWait { step: Some(self) }
    }

    fn answer_remaining(&mut self, content: &str) {
        while self.answers.len() < self.call_ids.len() {
            self.answer(content.to_string(), Some(true));
        }
    }

    fn store_answers(&mut self, guidance: Option<String>) {
        let mut content = std::mem::take(&mut self.answers);
        if let Some(text) = guidance {
            content.push(ContentBlock::Text { text });
        }
        self.conversation.messages.push(Message {
            role: Role::User,
            content,
        });
    }
}

pub(crate) struct ApprovalWait<'history> {
    step: Option<PendingStep<'history>>,
}

impl<'history> ApprovalWait<'history> {
    pub(crate) fn resume(mut self) -> PendingStep<'history> {
        self.step.take().expect("an active approval wait")
    }
}

impl Drop for ApprovalWait<'_> {
    fn drop(&mut self) {
        if let Some(step) = self.step.take() {
            step.finish_not_run(
                "Not run: the turn was interrupted before this call was made. It changed nothing.",
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const DECLINED: &str = "declined";
    #[test]
    fn push_user_text_merges_into_a_trailing_user_message() {
        let mut conversation = Conversation {
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "a".into(),
                    content: DECLINED.into(),
                    is_error: Some(true),
                }],
            }],
        };
        conversation.push_user_text("next request".to_string());
        assert_eq!(
            conversation.messages().len(),
            1,
            "two user messages in a row"
        );
        assert_eq!(conversation.messages()[0].content.len(), 2);
        assert!(matches!(
            &conversation.messages()[0].content[1],
            ContentBlock::Text { text } if text == "next request"
        ));

        // Text already there joins into one block, not two.
        let mut conversation = Conversation {
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "first".into(),
                }],
            }],
        };
        conversation.push_user_text("second".to_string());
        assert!(matches!(
            conversation.messages()[0].content.as_slice(),
            [ContentBlock::Text { text }] if text == "first\n\nsecond"
        ));
    }

    #[test]
    fn push_user_text_pushes_after_an_assistant_message() {
        let mut conversation = Conversation {
            messages: vec![Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }],
        };
        conversation.push_user_text("hello".to_string());
        assert_eq!(conversation.messages().len(), 2);
        assert_eq!(conversation.messages()[1].role, Role::User);

        let mut empty = Conversation::default();
        empty.push_user_text("hello".to_string());
        assert_eq!(empty.messages().len(), 1);
    }
    #[test]
    fn withdrawing_takes_the_text_back_the_way_push_user_text_put_it() {
        // On its own: the message goes.
        let mut c = Conversation {
            messages: vec![Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }],
        };
        c.push_user_text("cut it".to_string());
        assert!(c.withdraw_user_text("cut it"));
        assert_eq!(c.messages().len(), 1, "{:?}", c.messages());

        // Joined onto tool results: only the text goes.
        let mut c = Conversation {
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "a".into(),
                    content: DECLINED.into(),
                    is_error: Some(true),
                }],
            }],
        };
        c.push_user_text("next".to_string());
        assert!(c.withdraw_user_text("next"));
        assert_eq!(c.messages().len(), 1);
        assert!(matches!(
            c.messages()[0].content.as_slice(),
            [ContentBlock::ToolResult { .. }]
        ));

        // Joined onto text in the same block: what was there stays.
        let mut c = Conversation::default();
        c.push_user_text("first".to_string());
        c.push_user_text("second".to_string());
        assert!(c.withdraw_user_text("second"));
        assert!(matches!(
            c.messages()[0].content.as_slice(),
            [ContentBlock::Text { text }] if text == "first"
        ));
    }

    #[test]
    fn withdrawing_leaves_a_conversation_that_does_not_end_in_the_text_alone() {
        // The model answered: nothing to take back.
        let mut c = Conversation::default();
        c.push_user_text("cut it".to_string());
        c.store_assistant(vec![ContentBlock::Text {
            text: "done".into(),
        }]);
        let before = format!("{:?}", c.messages());
        assert!(!c.withdraw_user_text("cut it"));
        assert_eq!(format!("{:?}", c.messages()), before);

        // Another text at the end, an empty conversation, a suffix that is
        // not the whole text.
        let mut c = Conversation::default();
        c.push_user_text("cut it now".to_string());
        assert!(!c.withdraw_user_text("it now"));
        assert!(!c.withdraw_user_text("something else"));
        assert!(!Conversation::default().withdraw_user_text("x"));
        assert_eq!(c.messages().len(), 1);
    }
    fn thinking(signature: &str) -> ContentBlock {
        ContentBlock::Thinking {
            thinking: format!("reasoning {signature}"),
            signature: signature.into(),
        }
    }

    /// `role:kind,kind` per message, to compare a conversation's shape.
    fn shape_of(conversation: &[Message]) -> Vec<String> {
        conversation
            .iter()
            .map(|m| {
                let kinds: Vec<&str> = m
                    .content
                    .iter()
                    .map(|b| match b {
                        ContentBlock::Text { .. } => "text",
                        ContentBlock::ToolUse { .. } => "tool_use",
                        ContentBlock::ToolResult { .. } => "tool_result",
                        ContentBlock::Thinking { .. } => "thinking",
                        ContentBlock::RedactedThinking { .. } => "redacted_thinking",
                    })
                    .collect();
                format!("{:?}:{}", m.role, kinds.join(","))
            })
            .collect()
    }

    #[test]
    fn invalidate_thinking_removes_both_kinds_and_keeps_the_rest_in_order() {
        let mut conversation = Conversation {
            messages: vec![
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text { text: "go".into() }],
                },
                Message {
                    role: Role::Assistant,
                    content: vec![
                        thinking("A"),
                        ContentBlock::RedactedThinking { data: "R".into() },
                        ContentBlock::Text {
                            text: "on it".into(),
                        },
                        thinking("B"),
                        ContentBlock::ToolUse {
                            id: "t1".into(),
                            name: "gain".into(),
                            input: json!({ "db": 3 }),
                        },
                    ],
                },
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "t1".into(),
                        content: "{}".into(),
                        is_error: None,
                    }],
                },
                Message {
                    role: Role::Assistant,
                    content: vec![
                        thinking("C"),
                        ContentBlock::Text {
                            text: "done".into(),
                        },
                    ],
                },
            ],
        };
        conversation.invalidate_thinking();

        assert_eq!(
            shape_of(conversation.messages()),
            [
                "User:text",
                "Assistant:text,tool_use",
                "User:tool_result",
                "Assistant:text"
            ]
        );
        // The survivors are untouched.
        assert!(matches!(
            &conversation.messages()[1].content[0],
            ContentBlock::Text { text } if text == "on it"
        ));
        assert!(matches!(
            &conversation.messages()[1].content[1],
            ContentBlock::ToolUse { id, input, .. } if id == "t1" && input == &json!({ "db": 3 })
        ));
        assert!(matches!(
            &conversation.messages()[2].content[0],
            ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "t1"
        ));
    }

    #[test]
    fn invalidating_without_thinking_preserves_history() {
        let mut conversation = Conversation::default();
        conversation.invalidate_thinking();
        conversation.push_user_text("go".into());
        conversation.store_assistant(vec![ContentBlock::Text {
            text: "done".into(),
        }]);
        conversation.invalidate_thinking();
        assert_eq!(
            shape_of(conversation.messages()),
            ["User:text", "Assistant:text"]
        );
    }

    fn step(conversation: &mut Conversation) -> PendingStep<'_> {
        conversation.push_user_text("go".into());
        conversation.store_assistant(vec![
            thinking("signed"),
            ContentBlock::ToolUse {
                id: "a".into(),
                name: "edit".into(),
                input: json!({}),
            },
            ContentBlock::ToolUse {
                id: "b".into(),
                name: "edit".into(),
                input: json!({}),
            },
            ContentBlock::ToolUse {
                id: "c".into(),
                name: "peek".into(),
                input: json!({}),
            },
        ]);
        conversation.pending_step(["a".into(), "b".into(), "c".into()])
    }

    #[test]
    fn completion_keeps_actual_results_before_not_run_and_guidance() {
        let mut conversation = Conversation::default();
        let mut pending = step(&mut conversation);
        pending.answer("applied".into(), None);
        pending.answer("invalid arguments".into(), Some(true));
        pending.finish_not_run("not reached", Some("try another edit".into()));
        assert_eq!(
            serde_json::to_value(&conversation.messages()[2]).unwrap(),
            json!({
                "role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "a", "content": "applied"},
                    {"type": "tool_result", "tool_use_id": "b", "content": "invalid arguments", "is_error": true},
                    {"type": "tool_result", "tool_use_id": "c", "content": "not reached", "is_error": true},
                    {"type": "text", "text": "try another edit"}
                ]
            })
        );
        assert!(matches!(&conversation.messages()[1].content[0],
            ContentBlock::Thinking { thinking, signature } if thinking == "reasoning signed" && signature == "signed"));
    }

    #[test]
    fn answer_remaining_fills_every_unanswered_call_with_an_error() {
        let mut conversation = Conversation::default();
        let mut pending = step(&mut conversation);
        pending.answer("applied".into(), None);
        pending.answer_remaining(UNANSWERED);
        pending.store_answers(None);
        assert_eq!(
            serde_json::to_value(&conversation.messages()[2].content).unwrap(),
            json!([
                {"type": "tool_result", "tool_use_id": "a", "content": "applied"},
                {"type": "tool_result", "tool_use_id": "b", "content": UNANSWERED, "is_error": true},
                {"type": "tool_result", "tool_use_id": "c", "content": UNANSWERED, "is_error": true}
            ])
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "answer every call")]
    fn finishing_a_step_with_a_missing_answer_asserts_in_debug() {
        let mut conversation = Conversation::default();
        let mut pending = step(&mut conversation);
        pending.answer("applied".into(), None);
        pending.finish();
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "one answer per call")]
    fn an_extra_answer_asserts_in_debug() {
        let mut conversation = Conversation::default();
        let mut pending = step(&mut conversation);
        for _ in 0..4 {
            pending.answer("x".into(), None);
        }
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn release_fills_a_missing_answer_and_ignores_an_extra_one() {
        let mut conversation = Conversation::default();
        let mut pending = step(&mut conversation);
        pending.answer("applied".into(), None);
        pending.finish();
        assert_eq!(conversation.messages()[2].content.len(), 3);

        let mut conversation = Conversation::default();
        let mut pending = step(&mut conversation);
        for _ in 0..4 {
            pending.answer("x".into(), None);
        }
        pending.finish();
        assert_eq!(conversation.messages()[2].content.len(), 3);
    }

    #[test]
    fn a_completed_approval_wait_has_only_explicit_answers() {
        let mut conversation = Conversation::default();
        let mut pending = step(&mut conversation);
        pending = pending.awaiting_approval().resume();
        pending.answer("one".into(), None);
        pending.answer("two".into(), None);
        pending.answer("three".into(), None);
        pending.finish();
        assert_eq!(conversation.messages().len(), 3);
        assert_eq!(
            serde_json::to_value(&conversation.messages()[2].content).unwrap(),
            json!([
                {"type": "tool_result", "tool_use_id": "a", "content": "one"},
                {"type": "tool_result", "tool_use_id": "b", "content": "two"},
                {"type": "tool_result", "tool_use_id": "c", "content": "three"}
            ])
        );
    }

    #[test]
    fn an_interrupted_approval_wait_answers_only_unexecuted_calls() {
        let mut conversation = Conversation::default();
        {
            let pending = step(&mut conversation);
            drop(pending.awaiting_approval());
        }
        assert_eq!(conversation.messages().len(), 3);
        let answers = &conversation.messages()[2];
        assert_eq!(answers.role, Role::User);
        assert_eq!(answers.content.len(), 3);
        for (answer, expected) in answers.content.iter().zip(["a", "b", "c"]) {
            assert!(
                matches!(answer, ContentBlock::ToolResult { tool_use_id, content, is_error: Some(true) }
                if tool_use_id == expected && content.contains("interrupted") && content.contains("before this call was made"))
            );
        }
        conversation.push_user_text("next".into());
        assert_eq!(conversation.messages().len(), 3);
        assert!(
            matches!(&conversation.messages()[2].content[3], ContentBlock::Text { text } if text == "next")
        );
    }

    #[test]
    fn dropping_after_approval_does_not_invent_not_run_results() {
        let mut conversation = Conversation::default();
        {
            let pending = step(&mut conversation);
            let _resumed = pending.awaiting_approval().resume();
            // A dispatch may have completed before a callback panicked or its
            // result was stored. This scope must not claim it never ran.
        }
        assert_eq!(conversation.messages().len(), 2);
        assert_eq!(conversation.messages()[1].role, Role::Assistant);
    }

    #[test]
    fn assistant_storage_rejects_empty_and_thinking_only_replies() {
        let mut conversation = Conversation::default();
        conversation.push_user_text("go".into());
        conversation.store_assistant(vec![]);
        conversation.store_assistant(vec![
            thinking("A"),
            ContentBlock::RedactedThinking { data: "R".into() },
        ]);
        assert_eq!(conversation.messages().len(), 1);
        conversation.store_assistant(vec![
            thinking("B"),
            ContentBlock::Text {
                text: "done".into(),
            },
        ]);
        assert_eq!(
            shape_of(conversation.messages()),
            ["User:text", "Assistant:thinking,text"]
        );
        conversation.invalidate_thinking();
        assert_eq!(
            shape_of(conversation.messages()),
            ["User:text", "Assistant:text"]
        );
        conversation.reset();
        conversation.push_user_text("fresh".into());
        assert_eq!(
            serde_json::to_value(conversation.messages()).unwrap(),
            json!([
                {"role": "user", "content": [{"type": "text", "text": "fresh"}]}
            ])
        );
    }
}
