use crate::{
    syntax::{Document, Kind},
    text::{self, Encoding},
    workspace::{Target, Workspace},
};
use std::{collections::BTreeSet, sync::Arc};
use tower_lsp::lsp_types::*;

pub fn symbol_kind(kind: Kind) -> SymbolKind {
    match kind {
        Kind::Module => SymbolKind::MODULE,
        Kind::Function => SymbolKind::FUNCTION,
        Kind::Parameter | Kind::LocalParameter => SymbolKind::CONSTANT,
        Kind::Discipline | Kind::Nature => SymbolKind::TYPE_PARAMETER,
        Kind::Instance => SymbolKind::OBJECT,
        Kind::Block => SymbolKind::NAMESPACE,
        _ => SymbolKind::VARIABLE,
    }
}
fn completion_kind(kind: Kind) -> CompletionItemKind {
    match kind {
        Kind::Module => CompletionItemKind::MODULE,
        Kind::Function => CompletionItemKind::FUNCTION,
        Kind::Parameter | Kind::LocalParameter => CompletionItemKind::CONSTANT,
        Kind::Discipline | Kind::Nature => CompletionItemKind::CLASS,
        Kind::Port => CompletionItemKind::FIELD,
        _ => CompletionItemKind::VARIABLE,
    }
}
pub fn location(target: &Target, encoding: Encoding) -> Location {
    Location::new(
        target.0.uri.clone(),
        text::range(
            &target.0.text,
            target.0.symbols[target.1].span.clone(),
            encoding,
        ),
    )
}
fn same(a: &Target, b: &Target) -> bool {
    a.0.path == b.0.path && a.0.symbols[a.1].span == b.0.symbols[b.1].span
}
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('$', "\\$")
        .replace('}', "\\}")
}
fn identifier(name: &str) -> String {
    if name.starts_with('\\') {
        format!("{name} ")
    } else {
        name.to_owned()
    }
}

pub fn prefix(text: &str, byte: usize) -> (usize, String) {
    let before = &text[..byte];
    let line = before.rfind('\n').map_or(0, |p| p + 1);
    // Escaped names end at whitespace, unlike ordinary Verilog identifiers.
    if let Some(slash) = before[line..].rfind('\\') {
        let start = line + slash;
        if !before[start..].chars().any(char::is_whitespace) {
            return (start, before[start..].to_owned());
        }
    }
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphanumeric() || matches!(c, '_' | '$'))
        .last()
        .map_or(byte, |(p, _)| p);
    (start, before[start..].to_owned())
}

fn snippet(
    doc: &Document,
    module: &crate::syntax::Module,
    config: &crate::project::CompletionConfig,
    snippets: bool,
    indent: &str,
    newline: &str,
) -> String {
    let symbol = &doc.symbols[module.symbol];
    let mut next = 1;
    let mut field = |value: &str| {
        let result = if snippets {
            format!("${{{next}:{}}}", escape(value))
        } else {
            value.to_owned()
        };
        next += 1;
        result
    };
    let mut result = if snippets {
        escape(&identifier(&symbol.name))
    } else {
        identifier(&symbol.name)
    };
    if config.parameter_overrides == "defaults" && !module.parameters.is_empty() {
        result.push_str(" #(");
        result.push_str(newline);
        let values = module
            .parameters
            .iter()
            .map(|id| {
                let s = &doc.symbols[*id];
                let name = if snippets {
                    escape(&identifier(&s.name))
                } else {
                    identifier(&s.name)
                };
                format!(
                    "{indent}  .{name}({})",
                    field(s.default.as_deref().unwrap_or("0"))
                )
            })
            .collect::<Vec<_>>();
        result.push_str(&values.join(&format!(",{newline}")));
        result.push_str(newline);
        result.push_str(indent);
        result.push(')');
    }
    let instance = format!(
        "u_{}",
        symbol
            .name
            .trim_start_matches('\\')
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            })
            .collect::<String>()
    );
    result.push(' ');
    result.push_str(&field(&instance));
    result.push_str(" (");
    if !module.ports.is_empty() {
        result.push_str(newline);
        let values = module
            .ports
            .iter()
            .map(|id| {
                let s = &doc.symbols[*id];
                let name = if snippets {
                    escape(&identifier(&s.name))
                } else {
                    identifier(&s.name)
                };
                let filler = if config.connections == "empty" {
                    String::new()
                } else {
                    format!(
                        "{}_signal",
                        s.name
                            .trim_start_matches('\\')
                            .chars()
                            .map(|c| if c.is_ascii_alphanumeric() || c == '_' {
                                c
                            } else {
                                '_'
                            })
                            .collect::<String>()
                    )
                };
                format!("{indent}  .{name}({})", field(&filler))
            })
            .collect::<Vec<_>>();
        result.push_str(&values.join(&format!(",{newline}")));
        result.push_str(newline);
        result.push_str(indent);
    }
    result.push_str(");");
    if snippets {
        result.push_str("$0");
    }
    result
}

// Adapted from Veridian's keyword_completions/other_completions helpers. Keep
// only traditional Verilog/AMS entries rather than advertising SV-only syntax.
fn basic_completions() -> Vec<CompletionItem> {
    let keywords = [
        "analog",
        "begin",
        "end",
        "module",
        "endmodule",
        "parameter",
        "localparam",
        "real",
        "integer",
        "electrical",
        "branch",
        "discipline",
        "enddiscipline",
        "nature",
        "endnature",
        "function",
        "endfunction",
        "input",
        "output",
        "inout",
        "if",
        "else",
        "case",
        "endcase",
        "for",
        "while",
        "genvar",
        "generate",
        "endgenerate",
        "initial",
        "always",
        "assign",
        "wire",
        "reg",
        "wreal",
        "ground",
        "from",
        "exclude",
        "potential",
        "flow",
        "domain",
    ];
    let functions = [
        "V",
        "I",
        "ddt",
        "idt",
        "idtmod",
        "transition",
        "slew",
        "limexp",
        "cross",
        "above",
        "timer",
        "initial_step",
        "final_step",
        "white_noise",
        "flicker_noise",
        "abs",
        "sqrt",
        "exp",
        "ln",
        "log",
        "sin",
        "cos",
        "tan",
        "pow",
        "min",
        "max",
        "$abstime",
        "$temperature",
        "$bound_step",
        "$display",
        "$strobe",
    ];
    keywords
        .iter()
        .map(|s| CompletionItem {
            label: (*s).into(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        })
        .chain(functions.iter().map(|s| CompletionItem {
            label: (*s).into(),
            kind: Some(CompletionItemKind::FUNCTION),
            ..Default::default()
        }))
        .collect()
}

impl Workspace {
    pub fn completion(
        &self,
        uri: &Url,
        pos: Position,
        encoding: Encoding,
        snippets: bool,
    ) -> Vec<CompletionItem> {
        let Some(doc) = self.document(uri) else {
            return vec![];
        };
        let Some(byte) = text::offset(&doc.text, pos, encoding) else {
            return vec![];
        };
        if doc.opaque_at(byte.saturating_sub(1)) {
            return vec![];
        }
        let (start, typed) = prefix(&doc.text, byte);
        let before = &doc.text[..start];
        let edit_range = text::range(&doc.text, start..byte, encoding);
        if before.trim_end().ends_with('.') {
            if let Some(instance) = doc
                .instances
                .iter()
                .find(|i| i.full.start <= byte && byte <= i.full.end)
            {
                let targets = self.globals(&instance.target, Some(Kind::Module));
                if targets.len() == 1 {
                    let (target, id) = &targets[0];
                    if let Some(module) = target
                        .modules
                        .iter()
                        .find(|m| m.symbol == *id && m.complete)
                    {
                        let parameter = instance
                            .parameter_span
                            .as_ref()
                            .is_some_and(|r| r.start <= byte && byte <= r.end);
                        let used = if parameter {
                            &instance.parameters
                        } else {
                            &instance.ports
                        };
                        return (if parameter {
                            &module.parameters
                        } else {
                            &module.ports
                        })
                        .iter()
                        .filter_map(|id| {
                            let s = &target.symbols[*id];
                            if used.iter().any(|(name, span)| {
                                name == &s.name && !(span.start <= byte && byte <= span.end)
                            }) {
                                return None;
                            }
                            Some(CompletionItem {
                                label: s.name.clone(),
                                kind: Some(CompletionItemKind::FIELD),
                                detail: Some(s.detail.clone()),
                                text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                                    edit_range,
                                    identifier(&s.name),
                                ))),
                                ..Default::default()
                            })
                        })
                        .collect();
                    }
                }
            }
            return vec![];
        }
        let scope = doc.scope_at(byte.saturating_sub(1));
        let line_start = doc.text[..start].rfind('\n').map_or(0, |p| p + 1);
        let lead = &doc.text[line_start..start];
        let mut module_context = lead.trim().is_empty()
            && doc
                .modules
                .iter()
                .any(|m| doc.symbols[m.symbol].child_scope == Some(scope));
        // A bare partially typed module name can turn the entire enclosing module
        // into ERROR. Probe only this prefix as whitespace to recover its context;
        // never use the probe as a declaration/signature or publish its diagnostics.
        if !module_context && lead.trim().is_empty() && !doc.errors.is_empty() {
            let mut probe = doc.text.clone();
            probe.replace_range(start..byte, &" ".repeat(byte - start));
            // A new module often has no endmodule yet. A temporary closing
            // token can establish its body scope without changing the source.
            // Recovery covering the cursor is still too ambiguous to expand.
            for closing in ["", "\nendmodule\n"] {
                let Ok(recovered) = Document::parse(
                    doc.uri.clone(),
                    format!("{probe}{closing}"),
                    doc.version,
                    &self.project.config.defines,
                ) else {
                    continue;
                };
                if !closing.is_empty()
                    && recovered
                        .errors
                        .iter()
                        .any(|(span, _)| span.start <= start && start <= span.end)
                {
                    continue;
                }
                let scope = recovered.scope_at(start.saturating_sub(1));
                module_context = recovered
                    .modules
                    .iter()
                    .any(|m| recovered.symbols[m.symbol].child_scope == Some(scope));
                if module_context {
                    break;
                }
            }
        }
        let newline = if doc.text.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let mut items = Vec::new();
        let mut seen = BTreeSet::new();
        let mut at = Some(scope);
        while let Some(s) = at {
            for symbol in doc
                .symbols
                .iter()
                .filter(|x| x.scope == s && x.kind != Kind::Module)
            {
                if seen.insert(symbol.name.clone()) {
                    items.push(CompletionItem {
                        label: symbol.name.clone(),
                        kind: Some(completion_kind(symbol.kind)),
                        detail: Some(symbol.detail.clone()),
                        text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                            edit_range,
                            identifier(&symbol.name),
                        ))),
                        ..Default::default()
                    });
                }
            }
            at = doc.scopes[s].parent;
        }
        for file in self.files().into_values() {
            if module_context {
                for module in &file.modules {
                    if !module.complete {
                        continue;
                    }
                    let s = &file.symbols[module.symbol];
                    let content = snippet(
                        &file,
                        module,
                        &self.project.config.completion,
                        snippets,
                        // Snippet engines apply the insertion line's base indent;
                        // plain text edits need absolute indentation instead.
                        if snippets { "" } else { lead },
                        newline,
                    );
                    items.push(CompletionItem {
                        label: s.name.clone(),
                        filter_text: Some(s.name.clone()),
                        kind: Some(CompletionItemKind::MODULE),
                        detail: Some(format!(
                            "{} ports · {}",
                            module.ports.len(),
                            file.path.display()
                        )),
                        documentation: Some(Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: module_doc(&file, module),
                        })),
                        insert_text_format: Some(if snippets {
                            InsertTextFormat::SNIPPET
                        } else {
                            InsertTextFormat::PLAIN_TEXT
                        }),
                        insert_text_mode: Some(InsertTextMode::AS_IS),
                        text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                            edit_range, content,
                        ))),
                        ..Default::default()
                    });
                }
            }
            for s in file
                .symbols
                .iter()
                .filter(|s| s.scope == 0 && matches!(s.kind, Kind::Nature | Kind::Discipline))
            {
                if seen.insert(s.name.clone()) {
                    items.push(CompletionItem {
                        label: s.name.clone(),
                        kind: Some(completion_kind(s.kind)),
                        detail: Some(file.path.display().to_string()),
                        ..Default::default()
                    });
                }
            }
        }
        for item in basic_completions() {
            if seen.insert(item.label.clone()) {
                items.push(item);
            }
        }
        // The client performs fuzzy matching. Keep a practical bounded response.
        items.sort_by_key(|i| {
            (
                !i.label.to_lowercase().starts_with(&typed.to_lowercase()),
                i.label.clone(),
            )
        });
        items.truncate(500);
        items
    }

    pub fn definitions(&self, uri: &Url, pos: Position, encoding: Encoding) -> Vec<Location> {
        let Some(doc) = self.document(uri) else {
            return vec![];
        };
        let Some(byte) = text::offset(&doc.text, pos, encoding) else {
            return vec![];
        };
        for include in &doc.includes {
            if include.span.start <= byte && byte <= include.span.end {
                return self
                    .project
                    .resolve_include(&doc.path, &include.name)
                    .and_then(|p| Url::from_file_path(p).ok())
                    .map(|u| vec![Location::new(u, Range::default())])
                    .unwrap_or_default();
            }
        }
        self.resolve(&doc, byte)
            .iter()
            .map(|target| location(target, encoding))
            .collect()
    }
    pub fn hover(&self, uri: &Url, pos: Position, encoding: Encoding) -> Option<Hover> {
        let doc = self.document(uri)?;
        let byte = text::offset(&doc.text, pos, encoding)?;
        let targets = self.resolve(&doc, byte);
        if targets.is_empty() {
            return None;
        }
        let mut values = Vec::new();
        for (target, id) in targets.iter().take(10) {
            let s = &target.symbols[*id];
            let value = if let Some(module) = target.modules.iter().find(|m| m.symbol == *id) {
                module_doc(target, module)
            } else {
                format!("```verilog\n{}\n```", s.detail)
            };
            values.push(format!("{value}\n\n{}", target.path.display()));
        }
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: values.join("\n\n---\n\n"),
            }),
            range: doc
                .occurrence(byte)
                .map(|o| text::range(&doc.text, o.span.clone(), encoding)),
        })
    }
    pub fn document_symbols(&self, uri: &Url, encoding: Encoding) -> Vec<DocumentSymbol> {
        let Some(doc) = self.document(uri) else {
            return vec![];
        };
        fn build(doc: &Document, id: usize, encoding: Encoding) -> DocumentSymbol {
            let s = &doc.symbols[id];
            let children = s.child_scope.map(|scope| {
                doc.symbols
                    .iter()
                    .enumerate()
                    .filter(|(_, x)| x.scope == scope)
                    .map(|(i, _)| build(doc, i, encoding))
                    .collect()
            });
            #[allow(deprecated)]
            DocumentSymbol {
                name: s.name.clone(),
                detail: Some(s.detail.clone()),
                kind: symbol_kind(s.kind),
                tags: None,
                deprecated: None,
                range: text::range(&doc.text, s.full.clone(), encoding),
                selection_range: text::range(&doc.text, s.span.clone(), encoding),
                children,
            }
        }
        doc.symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| s.scope == 0)
            .map(|(id, _)| build(&doc, id, encoding))
            .collect()
    }
    pub fn workspace_symbols(&self, query: &str, encoding: Encoding) -> Vec<SymbolInformation> {
        let mut results = Vec::new();
        let query = query.to_lowercase();
        for doc in self.files().into_values() {
            for (id, s) in doc.symbols.iter().enumerate() {
                if s.name.to_lowercase().contains(&query) {
                    #[allow(deprecated)]
                    let item = SymbolInformation {
                        name: s.name.clone(),
                        kind: symbol_kind(s.kind),
                        tags: None,
                        deprecated: None,
                        location: location(&(doc.clone(), id), encoding),
                        container_name: Some(doc.path.display().to_string()),
                    };
                    results.push(item);
                    if results.len() == 1000 {
                        return results;
                    }
                }
            }
        }
        results
    }
    pub fn references(
        &self,
        uri: &Url,
        pos: Position,
        encoding: Encoding,
        include_declaration: bool,
    ) -> Vec<Location> {
        let Some(doc) = self.document(uri) else {
            return vec![];
        };
        let Some(byte) = text::offset(&doc.text, pos, encoding) else {
            return vec![];
        };
        let targets = self.resolve(&doc, byte);
        if targets.len() != 1 {
            return vec![];
        }
        let target = &targets[0];
        let name = &target.0.symbols[target.1].name;
        let mut refs = Vec::new();
        for file in self.files().into_values() {
            for occurrence in file.occurrences.iter().filter(|o| &o.name == name) {
                let resolved = self.resolve(&file, occurrence.span.start);
                if resolved.len() == 1 && same(&resolved[0], target) {
                    let declaration = file.path == target.0.path
                        && occurrence.span == target.0.symbols[target.1].span;
                    if include_declaration || !declaration {
                        refs.push(Location::new(
                            file.uri.clone(),
                            text::range(&file.text, occurrence.span.clone(), encoding),
                        ));
                    }
                }
            }
        }
        refs
    }
    pub fn signature(&self, uri: &Url, pos: Position, encoding: Encoding) -> Option<SignatureHelp> {
        let doc = self.document(uri)?;
        let byte = text::offset(&doc.text, pos, encoding)?;
        if doc.opaque_at(byte.saturating_sub(1)) {
            return None;
        }
        let mut depth = 0;
        let mut commas = 0;
        let mut opening = None;
        for (i, c) in doc.text[..byte].char_indices().rev() {
            match c {
                ')' => depth += 1,
                '(' => {
                    if depth == 0 {
                        opening = Some(i);
                        break;
                    }
                    depth -= 1;
                }
                ',' if depth == 0 => commas += 1,
                _ => {}
            }
        }
        let opening = opening?;
        let (_, name) = prefix(&doc.text, opening);
        let target = self.local(&doc, &name, doc.scope_at(opening))?;
        let symbol = &target.0.symbols[target.1];
        if symbol.kind != Kind::Function {
            return None;
        }
        let scope = symbol.child_scope?;
        let args: Vec<_> = target
            .0
            .symbols
            .iter()
            .filter(|s| s.scope == scope && s.kind == Kind::Port)
            .collect();
        let label = format!(
            "{}({})",
            symbol.name,
            args.iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label,
                documentation: Some(Documentation::String(symbol.detail.clone())),
                parameters: Some(
                    args.iter()
                        .map(|s| ParameterInformation {
                            label: ParameterLabel::Simple(s.name.clone()),
                            documentation: Some(Documentation::String(s.detail.clone())),
                        })
                        .collect(),
                ),
                active_parameter: None,
            }],
            active_signature: Some(0),
            active_parameter: Some(commas),
        })
    }
    pub fn rename_target(&self, uri: &Url, pos: Position, encoding: Encoding) -> Option<Target> {
        let doc = self.document(uri)?;
        let byte = text::offset(&doc.text, pos, encoding)?;
        let targets = self.resolve(&doc, byte);
        if targets.len() != 1 {
            return None;
        }
        let target = targets[0].clone();
        let s = &target.0.symbols[target.1];
        if !matches!(s.kind, Kind::Variable | Kind::Port | Kind::LocalParameter)
            || target.0.has_macros
            || !target.0.errors.is_empty()
            || self.is_library(&target.0.path)
            || s.name.starts_with('\\')
        {
            return None;
        }
        let mut scope = Some(s.scope);
        let mut function_local = false;
        while let Some(id) = scope {
            function_local |= target
                .0
                .symbols
                .iter()
                .any(|s| s.kind == Kind::Function && s.child_scope == Some(id));
            scope = target.0.scopes[id].parent;
        }
        function_local.then_some(target)
    }
    pub fn rename(
        &self,
        uri: &Url,
        pos: Position,
        encoding: Encoding,
        new_name: &str,
    ) -> Option<WorkspaceEdit> {
        let target = self.rename_target(uri, pos, encoding)?;
        let mut chars = new_name.chars();
        if !chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$'))
        {
            return None;
        }
        if basic_completions()
            .iter()
            .any(|c| c.kind == Some(CompletionItemKind::KEYWORD) && c.label == new_name)
        {
            return None;
        }
        let symbol = &target.0.symbols[target.1];
        // Reject capture by any binding in this function's overlapping scopes.
        if target.0.symbols.iter().any(|s| s.name == new_name) {
            return None;
        }
        let refs = self.references(uri, pos, encoding, true);
        if refs.is_empty() || refs.iter().any(|r| r.uri != target.0.uri) {
            return None;
        }
        let edits = refs
            .into_iter()
            .map(|r| OneOf::Left(TextEdit::new(r.range, new_name.to_owned())))
            .collect();
        let _ = symbol;
        Some(WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: target.0.uri.clone(),
                    version: target.0.version,
                },
                edits,
            }])),
            ..Default::default()
        })
    }
    pub fn diagnostics(&self, doc: &Arc<Document>, encoding: Encoding) -> Vec<Diagnostic> {
        let mut result = doc.syntax_diagnostics(encoding);
        if !doc.errors.is_empty() {
            return result;
        }
        for instance in &doc.instances {
            let modules = self.globals(&instance.target, Some(Kind::Module));
            if modules.len() != 1 {
                continue;
            }
            let (target, id) = &modules[0];
            let Some(module) = target
                .modules
                .iter()
                .find(|m| m.symbol == *id && m.complete)
            else {
                continue;
            };
            for (labels, allowed, kind) in [
                (&instance.ports, &module.ports, "port"),
                (&instance.parameters, &module.parameters, "parameter"),
            ] {
                let mut used = BTreeSet::new();
                for (name, span) in labels {
                    let message = if !used.insert(name) {
                        Some(format!("Duplicate named {kind} '{name}'"))
                    } else if !allowed.iter().any(|i| target.symbols[*i].name == *name) {
                        Some(format!(
                            "Unknown {kind} '{name}' on module '{}'",
                            instance.target
                        ))
                    } else {
                        None
                    };
                    if let Some(message) = message {
                        result.push(Diagnostic {
                            range: text::range(&doc.text, span.clone(), encoding),
                            severity: Some(DiagnosticSeverity::WARNING),
                            source: Some("vams-lsp".into()),
                            code: Some(NumberOrString::String("named-connection".into())),
                            message,
                            ..Default::default()
                        });
                    }
                }
            }
        }
        result.truncate(100);
        result
    }
}

fn module_doc(doc: &Document, module: &crate::syntax::Module) -> String {
    let symbol = &doc.symbols[module.symbol];
    let ports = module
        .ports
        .iter()
        .map(|id| doc.symbols[*id].name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut value = format!("```verilog\nmodule {}({ports});\n```", symbol.name);
    for id in &module.parameters {
        let s = &doc.symbols[*id];
        value.push_str(&format!(
            "\n- `{}` = `{}`",
            s.name,
            s.default.as_deref().unwrap_or("")
        ));
    }
    let before = &doc.text[..symbol.full.start];
    let mut comments = vec![];
    for line in before.trim_end().lines().rev() {
        if let Some(comment) = line.trim().strip_prefix("//") {
            comments.push(comment.trim());
        } else {
            break;
        }
    }
    comments.reverse();
    if !comments.is_empty() {
        value.push_str("\n\n");
        value.push_str(&comments.join("\n"));
    }
    value
}
