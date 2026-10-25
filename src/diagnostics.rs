//! Focused syntax explanations. Analysis always uses the untouched source tree.
use crate::syntax::descendants;
use std::ops::Range;
use tree_sitter::{Node, Parser};

mod expressions;
mod lex;
mod semicolons;
mod structure;
type Issue = (Range<usize>, String);

#[derive(Clone, PartialEq, Eq)]
struct Edit {
    span: Range<usize>,
    text: String,
}
impl Edit {
    fn insert(at: usize, text: impl Into<String>) -> Self {
        Self {
            span: at..at,
            text: text.into(),
        }
    }
}
struct Explanation {
    span: Range<usize>,
    message: String,
    edit: Option<Edit>,
}
impl Explanation {
    fn new(span: Range<usize>, message: impl Into<String>, edit: Option<Edit>) -> Self {
        Self {
            span,
            message: message.into(),
            edit,
        }
    }
}

struct Applied {
    source: Range<usize>,
    output: Range<usize>,
}
fn apply(input: &str, explanations: &[Explanation]) -> (String, Vec<Applied>) {
    let mut edits: Vec<_> = explanations.iter().filter_map(|e| e.edit.clone()).collect();
    edits.sort_by_key(|edit| (edit.span.start, edit.span.end));
    let mut output = String::new();
    let mut mapping = vec![];
    let mut at = 0;
    for edit in edits {
        if edit.span.start < at {
            continue;
        }
        output.push_str(&input[at..edit.span.start]);
        let start = output.len();
        output.push_str(&edit.text);
        mapping.push(Applied {
            source: edit.span.clone(),
            output: start..output.len(),
        });
        at = edit.span.end;
    }
    output.push_str(&input[at..]);
    (output, mapping)
}
fn original(offset: usize, mapping: &[Applied], end: bool) -> usize {
    let mut source_end = 0;
    let mut output_end = 0;
    for edit in mapping {
        if offset < edit.output.start || (end && offset == edit.output.start) {
            break;
        }
        if offset < edit.output.end {
            return edit.source.start;
        }
        source_end = edit.source.end;
        output_end = edit.output.end;
    }
    source_end + offset.saturating_sub(output_end)
}
fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start <= b.end && b.start <= a.end
}
fn add(issues: &mut Vec<Explanation>, issue: Explanation) {
    if issue.edit.is_some() && issues.iter().any(|old| old.edit == issue.edit) {
        return;
    }
    if issues
        .iter()
        .any(|old| old.span == issue.span && old.message == issue.message)
    {
        return;
    }
    issues.push(issue);
}

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
    let lexed = lex::scan(input);
    let mut explanations = vec![];
    for issue in lexed.issues {
        add(&mut explanations, issue);
    }
    for header in descendants(root, "module_header").into_iter().take(32) {
        if let Some(at) = missing_header_semicolon(input, header) {
            add(
                &mut explanations,
                Explanation::new(
                    at..at,
                    "Missing `;` after module header",
                    Some(Edit::insert(at, ";")),
                ),
            );
        }
    }
    for issue in semicolons::explain(input, &lexed.tokens, parser, lexed.opaque_tail.is_none()) {
        add(&mut explanations, issue);
    }
    for issue in semicolons::initializers(input, &lexed.tokens, parser) {
        add(&mut explanations, issue);
    }
    for issue in expressions::explain(input, &lexed.tokens) {
        add(&mut explanations, issue);
    }
    for issue in structure::explain(input, &lexed.tokens, lexed.opaque_tail) {
        add(&mut explanations, issue);
    }
    explanations.truncate(64);
    if explanations.is_empty() {
        return collect(input, root);
    }

    // Recovery edits exist only in this diagnostic presentation. The real CST,
    // raw errors, completion, navigation and formatter never consume this text.
    let (repaired, mapping) = apply(input, &explanations);
    let remaining = parser
        .parse(repaired.as_bytes(), None)
        .map(|tree| collect(&repaired, tree.root_node()))
        .unwrap_or_else(|| collect(input, root));
    let mut result: Vec<_> = explanations
        .iter()
        .map(|e| (e.span.clone(), e.message.clone()))
        .collect();
    for (span, message) in remaining {
        let mapped = original(span.start, &mapping, false)..original(span.end, &mapping, true);
        if lexed.opaque_tail.is_some_and(|at| mapped.end >= at) {
            continue;
        }
        // Prefer a specific explanation over a cascade covering the same region.
        if message.starts_with("Incomplete or unrecognized")
            && explanations.iter().any(|e| overlaps(&mapped, &e.span))
        {
            continue;
        }
        if !result
            .iter()
            .any(|(span, old)| *span == mapped && *old == message)
        {
            result.push((mapped, message));
        }
    }
    result.sort_by_key(|(span, _)| span.start);
    result.truncate(100);
    result
}

pub(crate) fn module_body_at(input: &str) -> bool {
    let lexed = lex::scan(input);
    structure::module_body(input, &lexed.tokens, lexed.opaque_tail.is_some())
}
