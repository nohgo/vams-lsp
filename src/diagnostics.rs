//! Focused syntax explanations. Analysis always uses the untouched source tree.
use crate::syntax::descendants;
use std::ops::Range;
use tree_sitter::{Node, Parser};

type Issue = (Range<usize>, String);

fn trivia(input: &[u8], mut at: usize) -> Option<usize> {
    loop {
        while input.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if input.get(at..at + 2) == Some(b"//") {
            while input.get(at).is_some_and(|b| *b != b'\n') {
                at += 1;
            }
        } else if input.get(at..at + 2) == Some(b"/*") {
            at += 2;
            while input.get(at..at + 2) != Some(b"*/") {
                if at >= input.len() {
                    return None;
                }
                at += 1;
            }
            at += 2;
        } else {
            return Some(at);
        }
    }
}

fn parentheses(input: &[u8], mut at: usize) -> Option<usize> {
    if input.get(at) != Some(&b'(') {
        return None;
    }
    let mut depth = 0;
    while at < input.len() {
        at = trivia(input, at)?;
        match *input.get(at)? {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at + 1);
                }
            }
            b'"' => {
                at += 1;
                loop {
                    match input.get(at)? {
                        b'"' => break,
                        b'\\' => at += 1,
                        _ => {}
                    }
                    at += 1;
                }
            }
            b'\\' => {
                while input.get(at).is_some_and(|b| !b.is_ascii_whitespace()) {
                    at += 1;
                }
                continue;
            }
            _ => {}
        }
        at += 1;
    }
    None
}

// Start from a parser-recognized module name and read only its optional header.
// Never infer a terminator through unbalanced ports or an unexpanded macro.
fn missing_header_semicolon(input: &str, header: Node<'_>) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut end = header.end_byte();
    let mut next = trivia(bytes, end)?;
    if bytes.get(next) == Some(&b'#') {
        next = trivia(bytes, next + 1)?;
        end = parentheses(bytes, next)?;
        next = trivia(bytes, end)?;
    }
    if bytes.get(next) == Some(&b'(') && bytes.get(next + 1) != Some(&b'*') {
        end = parentheses(bytes, next)?;
        next = trivia(bytes, end)?;
    }
    match bytes.get(next) {
        None => Some(end),
        Some(b) if b.is_ascii_alphabetic() || matches!(*b, b'_' | b'\\') => Some(end),
        Some(b'(') if bytes.get(next + 1) == Some(&b'*') => Some(end),
        _ => None,
    }
}

fn collect(input: &str, root: Node<'_>) -> Vec<Issue> {
    let mut result = Vec::new();
    let mut stack = vec![(root, false)];
    while let Some((node, inside_error)) = stack.pop() {
        let error = node.is_error() || node.is_missing();
        if error && !inside_error && result.len() < 100 {
            let raw = input[node.byte_range()].trim();
            let message = if node.is_missing() {
                format!("Missing `{}`", node.kind())
            } else if raw.len() == 1 && ",;)]}".contains(raw) {
                format!("Unexpected `{raw}`")
            } else {
                "Incomplete or unrecognized Verilog-AMS syntax".into()
            };
            result.push((node.byte_range(), message));
        }
        if matches!(
            node.kind(),
            "comment" | "string_literal" | "text_macro_definition"
        ) {
            continue;
        }
        for i in (0..node.child_count()).rev() {
            if let Some(child) = node.child(i as u32) {
                stack.push((child, inside_error || error));
            }
        }
    }
    result
}

pub(crate) fn present(input: &str, root: Node<'_>, parser: &mut Parser) -> Vec<Issue> {
    if !root.has_error() {
        return Vec::new();
    }
    let mut insertions: Vec<_> = descendants(root, "module_header")
        .into_iter()
        .filter_map(|header| missing_header_semicolon(input, header))
        .take(16)
        .collect();
    insertions.sort_unstable();
    insertions.dedup();
    if insertions.is_empty() {
        return collect(input, root);
    }

    // Parse a diagnostic-only header repair to avoid blaming the next declaration
    // for the missing terminator. Map every remaining error back to source bytes.
    // No repaired tree is used by completion, navigation, rename, or formatting.
    let mut repaired = input.to_owned();
    for at in insertions.iter().rev() {
        repaired.insert(*at, ';');
    }
    let Some(tree) = parser.parse(repaired.as_bytes(), None) else {
        return collect(input, root);
    };
    let original = |offset: usize| {
        offset
            - insertions
                .iter()
                .enumerate()
                .take_while(|(i, at)| **at + i < offset)
                .count()
    };
    let mut result: Vec<_> = insertions
        .iter()
        .map(|at| (*at..*at, "Missing `;` after module header".into()))
        .collect();
    result.extend(
        collect(&repaired, tree.root_node())
            .into_iter()
            .map(|(span, message)| (original(span.start)..original(span.end), message)),
    );
    result.sort_by_key(|(span, _)| span.start);
    result.truncate(100);
    result
}
