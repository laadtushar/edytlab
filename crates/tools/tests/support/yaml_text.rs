//! Reading GitHub workflow YAML as text, for the tests that hold a workflow
//! to a rule without running it.
#![allow(dead_code)]

/// A YAML line up to its trailing comment, so a gate or a pin copied into a
/// comment does not count as one.
///
/// A `#` starts the comment at the line start or after white space, outside
/// quotes. A quote opens a quoted scalar only where a value starts (YAML
/// 1.2.2, 7.3): after indentation, after `- `, after `: `, or after `[`, `{`
/// or `,`. Anywhere else it is a plain character, as the `'` in
/// `bundler's tools # note`. In single quotes `''` is an escaped quote, in
/// double quotes `\"` is.
pub fn without_comment(line: &str) -> &str {
    let mut chars = line.char_indices().peekable();
    let mut after_space = true;
    while let Some((i, c)) = chars.next() {
        match c {
            '#' if after_space => return &line[..i],
            '\'' | '"' if opens_a_value(&line[..i]) => {
                while let Some((_, q)) = chars.next() {
                    match q {
                        '\\' if c == '"' => {
                            chars.next();
                        }
                        '\'' if c == '\'' && chars.peek().is_some_and(|&(_, n)| n == '\'') => {
                            chars.next();
                        }
                        _ if q == c => break,
                        _ => {}
                    }
                }
                after_space = false;
                continue;
            }
            _ => {}
        }
        after_space = c.is_whitespace();
    }
    line
}

/// Whether a value starts right after `before`, the line up to a quote.
fn opens_a_value(before: &str) -> bool {
    let head = before.trim_end();
    let spaced = head.len() < before.len();
    match head.chars().last() {
        None | Some('[' | '{' | ',') => true,
        Some('-') => {
            spaced
                && head[..head.len() - 1]
                    .chars()
                    .last()
                    .is_none_or(char::is_whitespace)
        }
        Some(':') => spaced,
        Some(_) => false,
    }
}
