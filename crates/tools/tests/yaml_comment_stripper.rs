//! The shared `without_comment` the workflow text tests use follows YAML's
//! quoting, not just "a quote opens a quote".
//!
//! YAML 1.2.2 (<https://yaml.org/spec/1.2.2/>): a quoted scalar starts only
//! where a value starts (section 7.3, flow scalar styles), so the `'` in the
//! plain scalar `bundler's tools` is just a character (section 7.3.3, plain
//! style). Inside single quotes `''` is an escaped quote (7.3.2) and inside
//! double quotes `\"` is (7.3.1). A `#` starts a comment only at the line
//! start or after white space (section 6.6, comments).

#[path = "support/yaml_text.rs"]
mod yaml_text;

use yaml_text::without_comment;

#[test]
fn an_apostrophe_in_a_plain_scalar_does_not_hide_the_comment() {
    assert_eq!(
        without_comment("name: bundler's tools # note"),
        "name: bundler's tools "
    );
    assert_eq!(without_comment("- it's # c"), "- it's ");
}

#[test]
fn a_hash_inside_quotes_is_kept() {
    assert_eq!(without_comment("name: 'a # b'"), "name: 'a # b'");
    assert_eq!(without_comment("name: \"a # b\""), "name: \"a # b\"");
}

#[test]
fn a_hash_without_a_space_before_it_is_not_a_comment() {
    assert_eq!(without_comment("uses: a#b"), "uses: a#b");
    assert_eq!(without_comment("uses: a#b # c"), "uses: a#b ");
}

#[test]
fn a_comment_only_line_is_all_comment() {
    assert_eq!(without_comment("# whole line"), "");
    assert_eq!(without_comment("    # indented"), "    ");
}

#[test]
fn a_quoted_sequence_item_then_a_comment() {
    assert_eq!(without_comment("- 'x # y' # c"), "- 'x # y' ");
}

#[test]
fn an_escaped_double_quote_does_not_end_the_scalar() {
    assert_eq!(
        without_comment(r#"key: "a\" # b" # c"#),
        r#"key: "a\" # b" "#
    );
}

#[test]
fn a_doubled_single_quote_does_not_end_the_scalar() {
    assert_eq!(without_comment("key: 'a'' # b' # c"), "key: 'a'' # b' ");
}

#[test]
fn quotes_in_a_flow_sequence() {
    assert_eq!(
        without_comment("key: [a, 'b # c'] # d"),
        "key: [a, 'b # c'] "
    );
    assert_eq!(
        without_comment("key: {a: 'b # c', d: \"e # f\"} # g"),
        "key: {a: 'b # c', d: \"e # f\"} "
    );
}

#[test]
fn a_quote_in_the_middle_of_a_plain_scalar_opens_nothing() {
    assert_eq!(without_comment("run: echo \"hi # there"), "run: echo \"hi ");
}

#[test]
fn a_line_without_a_comment_comes_back_whole() {
    assert_eq!(
        without_comment("uses: actions/checkout@abc"),
        "uses: actions/checkout@abc"
    );
    assert_eq!(without_comment(""), "");
}
