use super::{
    lex::{Kind, Token},
    Edit, Explanation,
};

struct Frame<'a> {
    open: &'a str,
    close: &'static str,
    span: std::ops::Range<usize>,
    interval: bool,
    uncertain: bool,
}
fn opener(word: &str) -> Option<&'static str> {
    Some(match word {
        "(" => ")",
        "[" => "]",
        "{" => "}",
        "module" | "macromodule" | "connectmodule" => "endmodule",
        "begin" => "end",
        "case" | "casex" | "casez" => "endcase",
        "function" => "endfunction",
        "task" => "endtask",
        "nature" => "endnature",
        "discipline" => "enddiscipline",
        "generate" => "endgenerate",
        "fork" => "join",
        "specify" => "endspecify",
        "table" => "endtable",
        "connectrules" => "endconnectrules",
        _ => return None,
    })
}
fn closing(word: &str) -> bool {
    matches!(
        word,
        ")" | "]"
            | "}"
            | "endmodule"
            | "end"
            | "endcase"
            | "endfunction"
            | "endtask"
            | "endnature"
            | "enddiscipline"
            | "endgenerate"
            | "join"
            | "endspecify"
            | "endtable"
            | "endconnectrules"
    )
}
fn word_frame(frame: &Frame<'_>) -> bool {
    frame.open.len() > 1
}
fn missing(frame: Frame<'_>, at: usize, boundary: Option<&Token>, input: &str) -> Explanation {
    let message = if frame.interval {
        "Unclosed parameter interval; expected `)` or `]`".to_owned()
    } else if let Some(boundary) = boundary {
        format!(
            "Missing `{}` to close `{}` before `{}`",
            frame.close,
            frame.open,
            boundary.text(input)
        )
    } else {
        format!("Missing `{}` to close `{}`", frame.close, frame.open)
    };
    let span = boundary.map_or(frame.span, |t| t.span.clone());
    let replacement = if word_frame_text(frame.open) {
        format!("\n{}\n", frame.close)
    } else {
        frame.close.into()
    };
    Explanation::new(span, message, Some(Edit::insert(at, replacement)))
}
fn word_frame_text(open: &str) -> bool {
    open.len() > 1
}
pub(super) fn explain(
    input: &str,
    tokens: &[Token],
    opaque_tail: Option<usize>,
) -> Vec<Explanation> {
    let mut stack: Vec<Frame<'_>> = vec![];
    let mut issues = vec![];
    let mut uncertain_global = false;
    for (i, token) in tokens.iter().enumerate() {
        let text = token.text(input);
        if token.kind == Kind::String || token.kind == Kind::Number {
            continue;
        }
        if token.kind == Kind::Macro {
            let previous = i
                .checked_sub(1)
                .map(|j| tokens[j].text(input))
                .unwrap_or("");
            // Macros in an expression cannot legally supply structural endings.
            if !matches!(
                previous,
                "=" | "<+"
                    | "<="
                    | "+"
                    | "-"
                    | "*"
                    | "/"
                    | "("
                    | "["
                    | ","
                    | "?"
                    | ":"
                    | "&&"
                    | "||"
            ) {
                if stack.is_empty() {
                    uncertain_global = true;
                }
                for frame in &mut stack {
                    frame.uncertain = true;
                }
            }
            continue;
        }
        if matches!(
            text,
            "module" | "macromodule" | "connectmodule" | "nature" | "discipline" | "connectrules"
        ) {
            while let Some(frame) = stack.pop() {
                if !frame.uncertain && !uncertain_global {
                    issues.push(missing(frame, token.span.start, Some(token), input));
                }
            }
            uncertain_global = false;
        }
        // A block boundary cannot be inside an unfinished expression.
        if text == "begin" {
            while stack.last().is_some_and(|f| !word_frame(f)) {
                let frame = stack.pop().unwrap();
                if !frame.uncertain {
                    issues.push(missing(frame, token.span.start, Some(token), input));
                }
            }
        }
        if let Some(close) = opener(text) {
            let previous = i
                .checked_sub(1)
                .map(|j| tokens[j].text(input))
                .unwrap_or("");
            stack.push(Frame {
                open: text,
                close,
                span: token.span.clone(),
                interval: matches!(text, "(" | "[") && matches!(previous, "from" | "exclude"),
                uncertain: uncertain_global || stack.last().is_some_and(|f| f.uncertain),
            });
            continue;
        }
        if !closing(text) {
            continue;
        }
        let matches = |f: &Frame<'_>| f.close == text || (f.interval && matches!(text, ")" | "]"));
        if let Some(position) = stack.iter().rposition(matches) {
            while stack.len() > position + 1 {
                let frame = stack.pop().unwrap();
                if !frame.uncertain {
                    issues.push(missing(frame, token.span.start, Some(token), input));
                }
            }
            stack.pop();
        } else if text.len() == 1 && stack.last().is_some_and(|f| !word_frame(f) && !f.uncertain) {
            let frame = stack.pop().unwrap();
            issues.push(Explanation::new(
                token.span.clone(),
                format!(
                    "Mismatched delimiter: expected `{}`, found `{text}`",
                    frame.close
                ),
                Some(Edit {
                    span: token.span.clone(),
                    text: frame.close.into(),
                }),
            ));
        } else if !uncertain_global && !stack.iter().any(|f| f.uncertain) {
            let expected_open = match text {
                ")" => "(",
                "]" => "[",
                "}" => "{",
                "end" => "begin",
                "endcase" => "case",
                "endmodule" => "module",
                "endfunction" => "function",
                "endtask" => "task",
                "endnature" => "nature",
                "enddiscipline" => "discipline",
                "endgenerate" => "generate",
                "join" => "fork",
                "endspecify" => "specify",
                "endtable" => "table",
                _ => "connectrules",
            };
            issues.push(Explanation::new(
                token.span.clone(),
                format!("Unexpected `{text}`; no matching `{expected_open}`"),
                Some(Edit {
                    span: token.span.clone(),
                    text: String::new(),
                }),
            ));
        }
    }
    if opaque_tail.is_none() {
        while let Some(frame) = stack.pop() {
            if !frame.uncertain {
                issues.push(missing(frame, input.len(), None, input));
            }
        }
    }
    issues.truncate(40);
    issues
}

// A recovery parse may discard an unfinished procedural block. Use original
// tokens as a conservative guard before offering a module-instance template.
pub(super) fn module_body(input: &str, tokens: &[Token], opaque: bool) -> bool {
    if opaque {
        return false;
    }
    let mut stack: Vec<&str> = vec![];
    let mut header_done = false;
    let mut pending = false;
    let mut uncertain = false;
    for (i, token) in tokens.iter().enumerate() {
        if matches!(token.kind, Kind::String | Kind::Number) {
            continue;
        }
        let text = token.text(input);
        if token.kind == Kind::Macro {
            let previous = i
                .checked_sub(1)
                .map(|j| tokens[j].text(input))
                .unwrap_or("");
            if !matches!(
                previous,
                "=" | "<+"
                    | "<="
                    | "+"
                    | "-"
                    | "*"
                    | "/"
                    | "("
                    | "["
                    | ","
                    | "?"
                    | ":"
                    | "&&"
                    | "||"
            ) {
                uncertain = true;
            }
            continue;
        }
        if matches!(text, "module" | "macromodule" | "connectmodule") {
            stack.clear();
            header_done = false;
            uncertain = false;
            pending = false;
        }
        if matches!(text, "analog" | "always" | "initial") {
            pending = true;
        }
        if text == ";" && stack.len() == 1 && stack[0] == "endmodule" {
            header_done = true;
            pending = false;
        }
        if let Some(close) = opener(text) {
            stack.push(close);
            if text == "begin" {
                pending = false;
            }
        } else if closing(text) {
            if let Some(i) = stack.iter().rposition(|close| {
                *close == text || (matches!(*close, ")" | "]") && matches!(text, ")" | "]"))
            }) {
                stack.truncate(i);
            }
            if matches!(text, "end" | "endfunction" | "endtask") {
                pending = false;
            }
        }
    }
    !uncertain && header_done && !pending && stack == ["endmodule"]
}
