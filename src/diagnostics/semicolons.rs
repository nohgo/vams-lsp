use super::{
    lex::{Kind, Token},
    Edit, Explanation,
};
use tree_sitter::Parser;

#[derive(Clone, Copy)]
enum Wrapper {
    Module,
    Digital,
    Analog,
    ModuleHeader,
    FunctionHeader,
    TaskHeader,
    Nature,
    Discipline,
}
struct Class {
    label: &'static str,
    wrapper: Wrapper,
}
fn is_type(word: &str) -> bool {
    matches!(
        word,
        "input"
            | "output"
            | "inout"
            | "wire"
            | "tri"
            | "tri0"
            | "tri1"
            | "wand"
            | "wor"
            | "triand"
            | "trior"
            | "supply0"
            | "supply1"
            | "reg"
            | "integer"
            | "real"
            | "realtime"
            | "time"
            | "wreal"
            | "event"
            | "parameter"
            | "localparam"
            | "genvar"
            | "electrical"
            | "signed"
            | "unsigned"
    )
}
fn close_keyword(word: &str) -> bool {
    matches!(
        word,
        "end"
            | "endmodule"
            | "endfunction"
            | "endtask"
            | "endcase"
            | "endnature"
            | "enddiscipline"
            | "endgenerate"
            | "endconnectrules"
            | "join"
            | "endspecify"
            | "endtable"
    )
}
fn new_statement(input: &str, tokens: &[Token], at: usize, in_case: bool) -> bool {
    let text = tokens[at].text(input);
    if close_keyword(text)
        || is_type(text)
        || matches!(
            text,
            "analog"
                | "initial"
                | "always"
                | "assign"
                | "branch"
                | "if"
                | "for"
                | "while"
                | "repeat"
                | "case"
                | "casex"
                | "casez"
                | "nature"
                | "discipline"
                | "module"
                | "connectmodule"
                | "function"
                | "task"
                | "units"
                | "access"
                | "abstol"
                | "potential"
                | "flow"
                | "domain"
                | "aliasparam"
        )
    {
        return true;
    }
    let next = tokens.get(at + 1).map(|t| t.text(input)).unwrap_or("");
    (in_case && next == ":")
        || (tokens[at].kind == Kind::Word
            && (tokens.get(at + 1).is_some_and(|t| t.kind == Kind::Word)
                || matches!(next, "=" | "<=" | "<+" | "#")))
}
fn control_end(input: &str, tokens: &[Token], open: usize) -> Option<usize> {
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
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}
fn classify(input: &str, tokens: &[Token], region: Option<Wrapper>) -> Option<Class> {
    let first = tokens.first()?.text(input);
    let mut body = tokens;
    let original_module_item = matches!(first, "analog" | "always" | "initial");
    if first == "module" || first == "macromodule" || first == "connectmodule" {
        return Some(Class {
            label: "module header",
            wrapper: Wrapper::ModuleHeader,
        });
    }
    if first == "function"
        || (first == "analog" && tokens.get(1).is_some_and(|t| t.text(input) == "function"))
    {
        return Some(Class {
            label: "function header",
            wrapper: Wrapper::FunctionHeader,
        });
    }
    if first == "task" {
        return Some(Class {
            label: "task header",
            wrapper: Wrapper::TaskHeader,
        });
    }
    for _ in 0..8 {
        let first = body.first()?.text(input);
        if matches!(first, "analog" | "initial" | "always" | "else") {
            body = &body[1..];
            continue;
        }
        if first == "@" || first == "#" {
            let end = if body.get(1)?.text(input) == "(" {
                control_end(input, body, 1)?
            } else {
                2
            };
            body = body.get(end..)?;
            continue;
        }
        if matches!(
            first,
            "if" | "for" | "while" | "repeat" | "case" | "casex" | "casez"
        ) {
            let end = control_end(input, body, 1)?;
            body = &body[end..];
            continue;
        }
        break;
    }
    let first = body.first()?.text(input);
    if matches!(first, "begin" | "end" | "generate" | "default") {
        return None;
    }
    let word_count = body.iter().filter(|t| t.kind == Kind::Word).count();
    if word_count < 2
        && (is_type(first) || matches!(first, "branch" | "potential" | "flow" | "domain"))
    {
        return None;
    }
    let wrapper = if original_module_item {
        Wrapper::Module
    } else {
        region.unwrap_or(Wrapper::Digital)
    };
    if body.iter().any(|t| t.text(input) == "<+") {
        return Some(Class {
            label: "analog contribution",
            wrapper: if original_module_item {
                Wrapper::Module
            } else {
                Wrapper::Analog
            },
        });
    }
    if first == "assign" {
        return Some(Class {
            label: "continuous assignment",
            wrapper: Wrapper::Module,
        });
    }
    if matches!(first, "parameter" | "localparam" | "specparam") {
        return Some(Class {
            label: "parameter declaration",
            wrapper: Wrapper::Module,
        });
    }
    if matches!(first, "input" | "output" | "inout") {
        return Some(Class {
            label: "port declaration",
            wrapper: Wrapper::Module,
        });
    }
    if first == "branch" {
        return Some(Class {
            label: "branch declaration",
            wrapper: Wrapper::Module,
        });
    }
    if matches!(first, "potential" | "flow" | "domain") {
        return Some(Class {
            label: "discipline attribute",
            wrapper: Wrapper::Discipline,
        });
    }
    if is_type(first) {
        return Some(Class {
            label: "declaration",
            wrapper: Wrapper::Module,
        });
    }
    if body.iter().any(|t| matches!(t.text(input), "=" | "<=")) {
        return Some(Class {
            label: if matches!(region, Some(Wrapper::Nature)) {
                "nature attribute"
            } else {
                "assignment"
            },
            wrapper,
        });
    }
    if body.first()?.kind == Kind::Word
        && body
            .get(1)
            .is_some_and(|t| t.kind == Kind::Word || t.text(input) == "#")
    {
        return Some(Class {
            label: "declaration or module instance",
            wrapper: Wrapper::Module,
        });
    }
    if body.first()?.kind == Kind::Word && body.get(1).is_some_and(|t| t.text(input) == "(") {
        return Some(Class {
            label: "function or task call",
            wrapper,
        });
    }
    None
}
fn program(source: &str, wrapper: Wrapper) -> String {
    match wrapper {
        Wrapper::Module => format!("module __vams_probe;\n{source}\nendmodule"),
        Wrapper::Digital => format!("module __vams_probe; initial begin\n{source}\nend endmodule"),
        Wrapper::Analog => format!("module __vams_probe; analog begin\n{source}\nend endmodule"),
        Wrapper::ModuleHeader => format!("{source}\nendmodule"),
        Wrapper::FunctionHeader => {
            format!("module __vams_probe;\n{source}\nreal __probe_value;\nbegin end endfunction endmodule")
        }
        Wrapper::TaskHeader => {
            format!("module __vams_probe;\n{source}\nbegin end endtask endmodule")
        }
        Wrapper::Nature => format!("nature __vams_probe;\n{source}\nendnature"),
        Wrapper::Discipline => format!("discipline __vams_probe;\n{source}\nenddiscipline"),
    }
}
fn valid(parser: &mut Parser, source: &str) -> bool {
    parser
        .parse(source, None)
        .is_some_and(|t| !t.root_node().has_error())
}
fn candidate(
    input: &str,
    tokens: &[Token],
    region: Option<Wrapper>,
    parser: &mut Parser,
) -> Option<Explanation> {
    let class = classify(input, tokens, region)?;
    let first = tokens.first()?.span.start;
    let end = tokens.last()?.span.end;
    if end - first > 8192 || tokens.iter().any(|t| t.kind == Kind::Macro) {
        return None;
    }
    let source = &input[first..end];
    // Require the AMS grammar to confirm this isolated construct both needs a
    // terminator and becomes valid with exactly that terminator, not another edit.
    if !valid(parser, &program(&format!("{source};"), class.wrapper))
        || valid(parser, &program(source, class.wrapper))
    {
        return None;
    }
    Some(Explanation::new(
        end..end,
        format!("Missing `;` after {}", class.label),
        Some(Edit::insert(end, ";")),
    ))
}
pub(super) fn explain(
    input: &str,
    tokens: &[Token],
    parser: &mut Parser,
    allow_eof: bool,
) -> Vec<Explanation> {
    let mut result = vec![];
    let mut start = 0;
    let mut depth: usize = 0;
    let mut cases: usize = 0;
    let mut region = None;
    let mut probes = 0;
    for (i, token) in tokens.iter().enumerate() {
        let text = token.text(input);
        if i > start && depth == 0 && probes < 32 && new_statement(input, tokens, i, cases > 0) {
            let previous = &tokens[i - 1];
            let last = previous.text(input);
            let end_value = matches!(previous.kind, Kind::Word | Kind::Number | Kind::String)
                || matches!(last, ")" | "]" | "}");
            if end_value
                && !is_type(last)
                && !close_keyword(last)
                && !matches!(
                    last,
                    "analog"
                        | "always"
                        | "initial"
                        | "begin"
                        | "else"
                        | "generate"
                        | "function"
                        | "task"
                )
            {
                probes += 1;
                if let Some(issue) = candidate(input, &tokens[start..i], region, parser) {
                    result.push(issue);
                    start = i;
                }
            }
        }
        match text {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            ";" if depth == 0 => start = i + 1,
            "begin" | "generate" | "fork" => {
                start = if tokens.get(i + 1).is_some_and(|t| t.text(input) == ":") {
                    (i + 3).min(tokens.len())
                } else {
                    i + 1
                };
            }
            "case" | "casex" | "casez" => cases += 1,
            ":" if depth == 0
                && cases > 0
                && !tokens[start.min(i)..i].iter().any(|t| t.text(input) == "?") =>
            {
                start = i + 1
            }
            "endcase" => {
                cases = cases.saturating_sub(1);
                start = i + 1;
                depth = 0;
            }
            "nature" | "discipline" => {
                region = Some(if text == "nature" {
                    Wrapper::Nature
                } else {
                    Wrapper::Discipline
                });
                // These two headers have OPTIONAL semicolons in AMS. Begin the
                // attribute statement after the name/inheritance, not the header.
                let mut end = (i + 2).min(tokens.len());
                if text == "nature" && tokens.get(end).is_some_and(|t| t.text(input) == ":") {
                    end = (end + 2).min(tokens.len());
                    if tokens.get(end).is_some_and(|t| t.text(input) == ".") {
                        end = (end + 2).min(tokens.len());
                    }
                }
                start = end;
            }
            "endnature" | "enddiscipline" => {
                region = None;
                start = i + 1;
                depth = 0;
            }
            _ if close_keyword(text) => {
                start = i + 1;
                depth = 0;
            }
            _ => {}
        }
    }
    if allow_eof && start < tokens.len() && depth == 0 && probes < 32 {
        if let Some(issue) = candidate(input, &tokens[start..], region, parser) {
            result.push(issue);
        }
    }
    result
}

pub(super) fn initializers(input: &str, tokens: &[Token], parser: &mut Parser) -> Vec<Explanation> {
    let mut issues = vec![];
    for (i, token) in tokens.iter().enumerate() {
        if !matches!(token.text(input), "parameter" | "localparam") || issues.len() >= 16 {
            continue;
        }
        let mut depth: usize = 0;
        let mut end = None;
        for (j, item) in tokens.iter().enumerate().skip(i + 1) {
            match item.text(input) {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                ";" if depth == 0 => {
                    end = Some(j);
                    break;
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            continue;
        };
        let declaration = &tokens[i..end];
        if declaration.len() < 2
            || declaration
                .iter()
                .any(|t| t.kind == Kind::Macro || matches!(t.text(input), "=" | ","))
        {
            continue;
        }
        let last = declaration.last().unwrap();
        if last.kind != Kind::Word
            || is_type(last.text(input))
            || last.span.end - token.span.start > 8192
        {
            continue;
        }
        let raw = &input[token.span.start..last.span.end];
        if valid(parser, &program(&format!("{raw} = 0;"), Wrapper::Module))
            && !valid(parser, &program(&format!("{raw};"), Wrapper::Module))
        {
            issues.push(Explanation::new(
                last.span.clone(),
                format!(
                    "Parameter `{}` requires an initializer (`= ...`)",
                    last.text(input)
                ),
                Some(Edit::insert(last.span.end, " = 0")),
            ));
        }
    }
    issues
}
