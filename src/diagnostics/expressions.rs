use super::{
    lex::{Kind, Token},
    Edit, Explanation,
};

fn call_end(input: &str, tokens: &[Token], open: usize) -> Option<usize> {
    if tokens.get(open)?.text(input) != "(" {
        return None;
    }
    let mut depth = 0;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        match token.text(input) {
            "(" => depth += 1,
            ")" => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}
pub(super) fn explain(input: &str, tokens: &[Token]) -> Vec<Explanation> {
    let mut result = vec![];
    let mut blocks: Vec<bool> = vec![];
    let mut pending = None;
    let mut function = false;
    let mut depth: usize = 0;
    let mut assigned = false;
    let mut attribute = false;
    for (i, token) in tokens.iter().enumerate() {
        if matches!(token.kind, Kind::String | Kind::Macro | Kind::Number) {
            continue;
        }
        let text = token.text(input);
        let next = tokens.get(i + 1).map(|t| t.text(input)).unwrap_or("");
        let previous = i
            .checked_sub(1)
            .map(|j| tokens[j].text(input))
            .unwrap_or("");
        if text == "(" && next == "*" {
            attribute = true;
        }
        if attribute {
            if text == ")" && previous == "*" {
                attribute = false;
            }
            continue;
        }
        match text {
            "module" | "connectmodule" | "endmodule" => {
                blocks.clear();
                pending = None;
                function = false;
                depth = 0;
                assigned = false;
            }
            "analog" => pending = Some(next != "function"),
            "always" | "initial" if previous != "analog" => pending = Some(false),
            "function" | "task" => {
                function = true;
                pending = Some(false);
            }
            "endfunction" | "endtask" => {
                function = false;
                pending = None;
            }
            "begin" => {
                let domain = pending
                    .take()
                    .unwrap_or_else(|| blocks.last().copied().unwrap_or(false));
                blocks.push(domain);
                assigned = false;
            }
            "end" => {
                blocks.pop();
                pending = None;
                assigned = false;
            }
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            ";" if depth == 0 => {
                pending = None;
                assigned = false;
            }
            _ => {}
        }
        let analog =
            !function && pending.unwrap_or_else(|| blocks.last().copied().unwrap_or(false));
        if matches!(text, "always_ff" | "always_comb" | "always_latch")
            && (next == "@"
                || next == "begin"
                || (tokens.get(i + 1).is_some_and(|t| t.kind == Kind::Word)
                    && tokens
                        .get(i + 2)
                        .is_some_and(|t| matches!(t.text(input), "=" | "<="))))
        {
            let replacement = if text == "always_ff" {
                "always"
            } else {
                "always @*"
            };
            let advice = if text == "always_ff" {
                "always @(...)"
            } else {
                "always @*"
            };
            result.push(Explanation::new(token.span.clone(), format!("SystemVerilog `{text}` is outside Verilog-AMS 2.4; use `{advice}` or a SystemVerilog module"), Some(Edit { span: token.span.clone(), text: replacement.into() })));
            pending = Some(false);
        }
        if matches!(
            text,
            "if" | "while" | "repeat" | "case" | "casex" | "casez" | "@"
        ) && next == "("
            && tokens.get(i + 2).is_some_and(|t| t.text(input) == ")")
        {
            let close = &tokens[i + 2];
            let description = match text {
                "if" | "while" => "condition",
                "repeat" => "repeat count",
                "@" => "event expression",
                _ => "selector expression",
            };
            let filler = if text == "@" { "__event" } else { "1" };
            result.push(Explanation::new(
                close.span.clone(),
                format!("Missing {description} inside `{text}(...)`"),
                Some(Edit::insert(close.span.start, filler)),
            ));
        }
        if analog && matches!(text, "V" | "I") && next == "(" {
            if let Some(end) = call_end(input, tokens, i + 1) {
                if let Some(operator) = tokens.get(end + 1).filter(|t| t.text(input) == "=") {
                    result.push(Explanation::new(
                        operator.span.clone(),
                        format!("Use `<+` to contribute to `{text}(...)`; `=` assigns a variable"),
                        Some(Edit {
                            span: operator.span.clone(),
                            text: "<+".into(),
                        }),
                    ));
                }
            }
        }
        if text == "<+" && !analog {
            let message = if function {
                "Analog contributions (`<+`) are not allowed inside functions"
            } else {
                "Analog contributions (`<+`) must be inside an `analog` block"
            };
            result.push(Explanation::new(token.span.clone(), message, None));
        }
        if analog && text == "<=" && depth == 0 && !assigned {
            result.push(Explanation::new(token.span.clone(), "Nonblocking `<=` assignments are not allowed in analog blocks; use `=` for a variable", Some(Edit { span: token.span.clone(), text: "=".into() })));
        }
        let binary = matches!(
            text,
            "=" | "<="
                | "<+"
                | "+"
                | "-"
                | "*"
                | "/"
                | "%"
                | "**"
                | "&&"
                | "||"
                | "=="
                | "!="
                | "<"
                | ">"
                | ">="
        );
        let wildcard = text == "*"
            && (previous == "@" || (previous == "(" && i >= 2 && tokens[i - 2].text(input) == "@"));
        let closes_statement = matches!(
            next,
            "end" | "endmodule" | "endfunction" | "endcase" | "endnature" | "enddiscipline"
        );
        if binary && !wildcard && (matches!(next, ";" | ")" | "]" | "}") || closes_statement) {
            let end = &tokens[i + 1];
            result.push(Explanation::new(
                token.span.clone(),
                format!("Missing expression after `{text}`"),
                Some(Edit::insert(
                    end.span.start,
                    if closes_statement { "0;" } else { "0" },
                )),
            ));
        }
        if depth == 0 && matches!(text, "=" | "<=" | "<+") {
            assigned = true;
        }
    }
    result.truncate(40);
    result
}
