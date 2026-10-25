use super::{Edit, Explanation};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Word,
    Number,
    String,
    Symbol,
    Macro,
}
#[derive(Clone)]
pub(super) struct Token {
    pub span: Range<usize>,
    pub kind: Kind,
}
impl Token {
    pub fn text<'a>(&self, input: &'a str) -> &'a str {
        &input[self.span.clone()]
    }
}
pub(super) struct Lexed {
    pub tokens: Vec<Token>,
    pub issues: Vec<Explanation>,
    pub opaque_tail: Option<usize>,
}
fn word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$')
}
fn macro_arguments(bytes: &[u8], mut at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'(') {
        return Some(at);
    }
    let mut depth = 0;
    while at < bytes.len() {
        at = super::trivia(bytes, at)?;
        match *bytes.get(at)? {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at + 1);
                }
            }
            b'"' => {
                at += 1;
                while *bytes.get(at)? != b'"' {
                    if bytes[at] == b'\\' {
                        at += 1;
                    }
                    at += 1;
                }
            }
            b'\\' => {
                while bytes.get(at).is_some_and(|b| !b.is_ascii_whitespace()) {
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
pub(super) fn scan(input: &str) -> Lexed {
    let bytes = input.as_bytes();
    let mut result = Lexed {
        tokens: vec![],
        issues: vec![],
        opaque_tail: None,
    };
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let start = at;
        if bytes.get(at..at + 2) == Some(b"//") {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if bytes.get(at..at + 2) == Some(b"/*") {
            at += 2;
            while at < bytes.len() && bytes.get(at..at + 2) != Some(b"*/") {
                at += 1;
            }
            if at == bytes.len() {
                result.issues.push(Explanation::new(
                    start..start + 2,
                    "Unterminated block comment; expected `*/`",
                    Some(Edit::insert(at, "*/")),
                ));
                result.opaque_tail = Some(start);
                break;
            }
            at += 2;
            continue;
        }
        let kind;
        if bytes[at] == b'"' {
            kind = Kind::String;
            at += 1;
            let mut bad_newline = None;
            while at < bytes.len() && bytes[at] != b'"' {
                if bytes[at] == b'\\' {
                    at += 1;
                    if bytes.get(at..at + 2) == Some(b"\r\n") {
                        at += 1;
                    }
                } else if matches!(bytes[at], b'\r' | b'\n') && bad_newline.is_none() {
                    bad_newline = Some(at);
                }
                at = (at + 1).min(bytes.len());
            }
            if at == bytes.len() || bad_newline.is_some() {
                let repair_at = bad_newline.unwrap_or(at);
                result.issues.push(Explanation::new(
                    start..start + 1,
                    "Unterminated string literal; expected a closing `\"`",
                    Some(Edit::insert(repair_at, "\"")),
                ));
                result.opaque_tail = Some(start);
            }
            if at < bytes.len() {
                at += 1;
            }
        } else if bytes[at] == b'\\' {
            kind = Kind::Word;
            while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
                at += 1;
            }
        } else if bytes[at] == b'`' {
            at += 1;
            while at < bytes.len() && word(bytes[at]) {
                at += 1;
            }
            let name = &input[start + 1..at];
            if matches!(
                name,
                "include"
                    | "define"
                    | "undef"
                    | "ifdef"
                    | "ifndef"
                    | "elsif"
                    | "else"
                    | "endif"
                    | "timescale"
                    | "default_nettype"
                    | "line"
                    | "resetall"
                    | "celldefine"
                    | "endcelldefine"
                    | "begin_keywords"
                    | "end_keywords"
                    | "unconnected_drive"
                    | "nounconnected_drive"
            ) {
                if name == "include" {
                    let mut quote = at;
                    while bytes.get(quote).is_some_and(|b| *b == b' ' || *b == b'\t') {
                        quote += 1;
                    }
                    if bytes.get(quote) == Some(&b'"') {
                        let mut end = quote + 1;
                        while bytes
                            .get(end)
                            .is_some_and(|b| !matches!(*b, b'"' | b'\r' | b'\n'))
                        {
                            if bytes[end] == b'\\'
                                && bytes
                                    .get(end + 1)
                                    .is_some_and(|b| !matches!(*b, b'\r' | b'\n'))
                            {
                                end += 1;
                            }
                            end += 1;
                        }
                        if bytes.get(end) != Some(&b'"') {
                            result.issues.push(Explanation::new(
                                quote..quote + 1,
                                "Unterminated include filename; expected a closing `\"`",
                                Some(Edit::insert(end, "\"")),
                            ));
                        }
                    }
                }
                loop {
                    while at < bytes.len() && bytes[at] != b'\n' {
                        at += 1;
                    }
                    let mut last = at;
                    if last > start && bytes[last - 1] == b'\r' {
                        last -= 1;
                    }
                    let continued = last > start && bytes[last - 1] == b'\\';
                    if at < bytes.len() {
                        at += 1;
                    }
                    if !continued || at == bytes.len() {
                        break;
                    }
                }
                continue;
            }
            kind = Kind::Macro;
            if bytes.get(at) == Some(&b'(') {
                let open = at;
                if let Some(end) = macro_arguments(bytes, at) {
                    at = end;
                } else {
                    result.issues.push(Explanation::new(
                        open..open + 1,
                        "Unclosed macro argument list; expected `)`",
                        Some(Edit::insert(bytes.len(), ")")),
                    ));
                    result.opaque_tail = Some(start);
                    at = bytes.len();
                }
            }
        } else if bytes[at].is_ascii_digit() || bytes[at] == b'\'' {
            kind = Kind::Number;
            at += 1;
            while at < bytes.len() && (word(bytes[at]) || matches!(bytes[at], b'\'' | b'.')) {
                at += 1;
            }
        } else if word(bytes[at]) {
            kind = Kind::Word;
            at += 1;
            while at < bytes.len() && word(bytes[at]) {
                at += 1;
            }
        } else {
            kind = Kind::Symbol;
            let tail = &input[at..];
            if let Some(op) = [
                "<<<", ">>>", "===", "!==", "<+", "<=", ">=", "==", "!=", "&&", "||", "**", "<<",
                ">>", "~&", "~|", "~^", "^~", "->", "+:", "-:",
            ]
            .into_iter()
            .find(|op| tail.starts_with(op))
            {
                at += op.len();
            } else {
                at += input[at..].chars().next().unwrap().len_utf8();
            }
        }
        result.tokens.push(Token {
            span: start..at,
            kind,
        });
    }
    result
}
