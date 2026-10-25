use crate::text::{self, Encoding};
use std::{collections::BTreeMap, ops::Range as Span, path::PathBuf};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Url};
use tree_sitter::{Node, Parser, Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Module,
    Port,
    Parameter,
    LocalParameter,
    Variable,
    Net,
    Function,
    Branch,
    Discipline,
    Nature,
    Instance,
    Block,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub kind: Kind,
    pub span: Span<usize>,
    pub full: Span<usize>,
    pub scope: usize,
    pub child_scope: Option<usize>,
    pub detail: String,
    pub default: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Scope {
    pub span: Span<usize>,
    pub parent: Option<usize>,
}
#[derive(Clone, Debug)]
pub struct Module {
    pub symbol: usize,
    pub ports: Vec<usize>,
    pub parameters: Vec<usize>,
    pub complete: bool,
}
#[derive(Clone, Debug)]
pub struct Instance {
    pub symbol: usize,
    pub target: String,
    pub type_span: Span<usize>,
    pub full: Span<usize>,
    pub ports: Vec<(String, Span<usize>)>,
    pub parameters: Vec<(String, Span<usize>)>,
    pub parameter_span: Option<Span<usize>>,
}
#[derive(Clone, Debug)]
pub struct Occurrence {
    pub name: String,
    pub span: Span<usize>,
    pub scope: usize,
}
#[derive(Clone, Debug)]
pub struct Include {
    pub name: String,
    pub span: Span<usize>,
}

pub struct Document {
    pub uri: Url,
    pub path: PathBuf,
    pub text: String,
    pub version: Option<i32>,
    pub tree: Tree,
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
    pub modules: Vec<Module>,
    pub instances: Vec<Instance>,
    pub occurrences: Vec<Occurrence>,
    pub includes: Vec<Include>,
    pub errors: Vec<(Span<usize>, String)>,
    diagnostic_errors: Vec<(Span<usize>, String)>,
    pub directive_errors: Vec<(Span<usize>, String)>,
    pub has_macros: bool,
}

pub fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}
pub fn descendants<'a>(node: Node<'a>, kind: &str) -> Vec<Node<'a>> {
    let mut result = Vec::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if n.kind() == kind {
            result.push(n);
        } else {
            stack.extend(children(n).into_iter().rev());
        }
    }
    result
}
fn ident(node: Node<'_>) -> Option<Node<'_>> {
    if matches!(node.kind(), "simple_identifier" | "escaped_identifier") {
        return Some(node);
    }
    children(node).into_iter().find_map(ident)
}
fn spelling(text: &str, node: Node<'_>) -> String {
    text[node.byte_range()].trim().to_owned()
}

// Mask inactive branches without changing byte offsets or line endings. This is
// directive selection, not token macro expansion; original source remains intact.
fn preprocess(
    text: &str,
    configured: &BTreeMap<String, String>,
) -> (String, Vec<(Span<usize>, String)>) {
    let mut defined: std::collections::HashSet<String> = configured.keys().cloned().collect();
    let mut bytes = text.as_bytes().to_vec();
    let mut frames: Vec<(bool, bool, bool)> = Vec::new(); // parent, branch taken, else seen
    let mut active = true;
    let mut offset = 0;
    let mut errors = Vec::new();
    let mut continuation = false;
    let mut block_comment = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let words: Vec<_> = trimmed.split_whitespace().collect();
        let directive = if !continuation && !block_comment {
            words.first().copied().unwrap_or("")
        } else {
            ""
        };
        let name = words
            .get(1)
            .copied()
            .unwrap_or("")
            .split('(')
            .next()
            .unwrap_or("");
        let control = matches!(
            directive,
            "`ifdef" | "`ifndef" | "`elsif" | "`else" | "`endif"
        );
        let before = active;
        match directive {
            "`ifdef" | "`ifndef" => {
                let condition = defined.contains(name) ^ (directive == "`ifndef");
                frames.push((active, condition, false));
                active &= condition;
            }
            "`elsif" => {
                if let Some((parent, taken, otherwise)) = frames.last_mut() {
                    if *otherwise {
                        errors.push((
                            offset..offset + line.trim_end().len(),
                            "`elsif after `else".into(),
                        ));
                    }
                    let condition = !*taken && defined.contains(name);
                    *taken |= condition;
                    active = *parent && condition;
                } else {
                    errors.push((
                        offset..offset + line.trim_end().len(),
                        "Unmatched `elsif".into(),
                    ));
                }
            }
            "`else" => {
                if let Some((parent, taken, otherwise)) = frames.last_mut() {
                    if *otherwise {
                        errors.push((
                            offset..offset + line.trim_end().len(),
                            "Duplicate `else".into(),
                        ));
                    }
                    active = *parent && !*taken;
                    *taken = true;
                    *otherwise = true;
                } else {
                    errors.push((
                        offset..offset + line.trim_end().len(),
                        "Unmatched `else".into(),
                    ));
                }
            }
            "`endif" => {
                if let Some((parent, _, _)) = frames.pop() {
                    active = parent;
                } else {
                    errors.push((
                        offset..offset + line.trim_end().len(),
                        "Unmatched `endif".into(),
                    ));
                }
            }
            "`define" if active => {
                defined.insert(name.to_owned());
            }
            "`undef" if active => {
                defined.remove(name);
            }
            _ => {}
        }
        if !before || control {
            for b in &mut bytes[offset..offset + line.len()] {
                if *b != b'\r' && *b != b'\n' {
                    *b = b' ';
                }
            }
        }
        continuation = line.trim_end_matches(['\r', '\n']).ends_with('\\');
        // Only recognize directives at the beginning of a real source line.
        if !control {
            let mut tail = line;
            while !tail.is_empty() {
                if block_comment {
                    if let Some(end) = tail.find("*/") {
                        block_comment = false;
                        tail = &tail[end + 2..];
                    } else {
                        break;
                    }
                } else if let Some(start) = tail.find("/*") {
                    if tail.find("//").is_some_and(|n| n < start) {
                        break;
                    }
                    block_comment = true;
                    tail = &tail[start + 2..];
                } else {
                    break;
                }
            }
        }
        offset += line.len();
    }
    if !frames.is_empty() {
        errors.push((text.len()..text.len(), "Missing `endif".into()));
    }
    (
        String::from_utf8(bytes).expect("mask preserves UTF-8"),
        errors,
    )
}

impl Document {
    pub fn parse(
        uri: Url,
        text: String,
        version: Option<i32>,
        defines: &BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let path = uri
            .to_file_path()
            .map_err(|_| "Only file: documents are supported")?;
        let (input, directive_errors) = preprocess(&text, defines);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_verilog_ams::LANGUAGE.into())
            .map_err(|e| e.to_string())?;
        let tree = parser
            .parse(input.as_bytes(), None)
            .ok_or("Parser cancelled")?;
        let mut doc = Self {
            uri,
            path,
            text,
            version,
            tree,
            symbols: Vec::new(),
            scopes: vec![Scope {
                span: 0..input.len(),
                parent: None,
            }],
            modules: Vec::new(),
            instances: Vec::new(),
            occurrences: Vec::new(),
            includes: Vec::new(),
            errors: Vec::new(),
            diagnostic_errors: Vec::new(),
            directive_errors,
            has_macros: input.contains('`'),
        };
        let tree = doc.tree.clone();
        doc.extract(tree.root_node(), 0, 0);
        doc.collect(tree.root_node());
        doc.diagnostic_errors = crate::diagnostics::present(&input, tree.root_node(), &mut parser);
        Ok(doc)
    }
    fn add(
        &mut self,
        name: Node<'_>,
        kind: Kind,
        full: Node<'_>,
        scope: usize,
        child_scope: Option<usize>,
        default: Option<String>,
    ) -> Option<usize> {
        let name = ident(name)?;
        let spelling = spelling(&self.text, name);
        if spelling.is_empty() {
            return None;
        }
        if matches!(kind, Kind::Port | Kind::Variable | Kind::Net) {
            if let Some((id, old)) = self
                .symbols
                .iter_mut()
                .enumerate()
                .find(|(_, s)| s.scope == scope && s.name == spelling && s.kind == Kind::Port)
            {
                old.detail = self.text[full.byte_range()].trim().to_owned();
                return Some(id);
            }
        }
        if let Some(i) = self
            .symbols
            .iter()
            .position(|s| s.span == name.byte_range())
        {
            return Some(i);
        }
        let detail = self.text[full.byte_range()]
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_owned();
        let id = self.symbols.len();
        self.symbols.push(Symbol {
            name: spelling,
            kind,
            span: name.byte_range(),
            full: full.byte_range(),
            scope,
            child_scope,
            detail,
            default,
        });
        Some(id)
    }
    fn new_scope(&mut self, node: Node<'_>, parent: usize) -> usize {
        let id = self.scopes.len();
        self.scopes.push(Scope {
            span: node.byte_range(),
            parent: Some(parent),
        });
        id
    }
    fn extract(&mut self, node: Node<'_>, scope: usize, depth: usize) {
        if depth > 256 {
            return;
        }
        let mut scope = scope;
        match node.kind() {
            "module_declaration" => {
                if let Some(header) = children(node)
                    .into_iter()
                    .find(|n| n.kind() == "module_header")
                {
                    if let Some(name) = header.child_by_field_name("name") {
                        scope = self.new_scope(node, scope);
                        if let Some(id) = self.add(name, Kind::Module, header, 0, Some(scope), None)
                        {
                            self.symbols[id].full = node.byte_range();
                            let mut ports = Vec::new();
                            let header_node = children(node).into_iter().find(|n| {
                                matches!(n.kind(), "module_ansi_header" | "module_nonansi_header")
                            });
                            if let Some(h) = header_node {
                                for p in descendants(h, "port_identifier") {
                                    if let Some(id) = self.add(p, Kind::Port, p, scope, None, None)
                                    {
                                        if !ports.contains(&id) {
                                            ports.push(id);
                                        }
                                    }
                                }
                            }
                            let complete = header_node.is_none_or(|h| !h.has_error())
                                && !header.has_error()
                                && children(node).iter().any(|n| n.kind() == "module_header")
                                && self.text[header.end_byte()..node.end_byte()].contains(';');
                            self.modules.push(Module {
                                symbol: id,
                                ports,
                                parameters: Vec::new(),
                                complete,
                            });
                        }
                    }
                }
            }
            "analog_function_declaration"
            | "function_body_declaration"
            | "task_body_declaration" => {
                let name = node.child_by_field_name("name").or_else(|| {
                    children(node)
                        .into_iter()
                        .find(|n| matches!(n.kind(), "function_identifier" | "task_identifier"))
                });
                if let Some(name) = name {
                    let child = self.new_scope(node, scope);
                    self.add(name, Kind::Function, node, scope, Some(child), None);
                    scope = child;
                }
            }
            "nature_declaration" | "discipline_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let kind = if node.kind() == "nature_declaration" {
                        Kind::Nature
                    } else {
                        Kind::Discipline
                    };
                    let child = self.new_scope(node, scope);
                    self.add(name, kind, node, scope, Some(child), None);
                    scope = child;
                }
            }
            "analog_seq_block" | "analog_function_seq_block" | "seq_block" | "generate_block" => {
                let child = self.new_scope(node, scope);
                if let Some(name) = node.child_by_field_name("name") {
                    self.add(name, Kind::Block, node, scope, Some(child), None);
                }
                scope = child;
            }
            "input_declaration"
            | "output_declaration"
            | "inout_declaration"
            | "tf_port_declaration"
            | "ansi_port_declaration"
            | "tf_port_item1" => {
                for p in descendants(node, "port_identifier") {
                    self.add(p, Kind::Port, node, scope, None, None);
                }
            }
            "param_assignment" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let mut ancestor = node.parent();
                    let mut local = false;
                    while let Some(n) = ancestor {
                        if n.kind() == "local_parameter_declaration" {
                            local = true;
                            break;
                        }
                        if n.kind() == "parameter_declaration" {
                            break;
                        }
                        ancestor = n.parent();
                    }
                    let value = node
                        .child_by_field_name("value")
                        .map(|n| spelling(&self.text, n));
                    if let Some(id) = self.add(
                        name,
                        if local {
                            Kind::LocalParameter
                        } else {
                            Kind::Parameter
                        },
                        node,
                        scope,
                        None,
                        value,
                    ) {
                        for module in &mut self.modules {
                            if self.symbols[module.symbol].child_scope == Some(scope) && !local {
                                module.parameters.push(id);
                            }
                        }
                    }
                }
            }
            "variable_decl_assignment" | "net_decl_assignment" => {
                if let Some(name) = children(node).first().copied().and_then(ident) {
                    let full = node.parent().and_then(|n| n.parent()).unwrap_or(node);
                    self.add(
                        name,
                        if node.kind() == "net_decl_assignment" {
                            Kind::Net
                        } else {
                            Kind::Variable
                        },
                        full,
                        scope,
                        None,
                        None,
                    );
                }
            }
            "branch_declaration" => {
                for i in 0..node.child_count() {
                    if node.field_name_for_child(i as u32) == Some("name") {
                        if let Some(n) = node.child(i as u32) {
                            self.add(n, Kind::Branch, node, scope, None, None);
                        }
                    }
                }
            }
            "module_instantiation" => {
                if let Some(target) = children(node).first().copied().and_then(ident) {
                    let parameters = descendants(node, "named_parameter_assignment")
                        .into_iter()
                        .filter_map(|p| {
                            children(p)
                                .into_iter()
                                .find(|n| n.kind() == "parameter_identifier")
                                .and_then(ident)
                        })
                        .map(|n| (spelling(&self.text, n), n.byte_range()))
                        .collect::<Vec<_>>();
                    let parameter_span = children(node)
                        .into_iter()
                        .find(|n| n.kind() == "parameter_value_assignment")
                        .map(|n| n.byte_range());
                    for instance in children(node)
                        .into_iter()
                        .filter(|n| n.kind() == "hierarchical_instance")
                    {
                        if let Some(name) = descendants(instance, "name_of_instance")
                            .first()
                            .copied()
                            .and_then(ident)
                        {
                            if let Some(symbol) =
                                self.add(name, Kind::Instance, instance, scope, None, None)
                            {
                                let ports = descendants(instance, "named_port_connection")
                                    .into_iter()
                                    .filter_map(|p| {
                                        children(p)
                                            .into_iter()
                                            .find(|n| n.kind() == "port_identifier")
                                            .and_then(ident)
                                    })
                                    .map(|n| (spelling(&self.text, n), n.byte_range()))
                                    .collect();
                                self.instances.push(Instance {
                                    symbol,
                                    target: spelling(&self.text, target),
                                    type_span: target.byte_range(),
                                    full: node.byte_range(),
                                    ports,
                                    parameters: parameters.clone(),
                                    parameter_span: parameter_span.clone(),
                                });
                            }
                        }
                    }
                }
            }
            "include_compiler_directive" => {
                let raw = &self.text[node.byte_range()];
                if let Some(start) = raw.find('"') {
                    if let Some(end) = raw[start + 1..].find('"') {
                        self.includes.push(Include {
                            name: raw[start + 1..start + 1 + end].to_owned(),
                            span: node.start_byte() + start..node.start_byte() + start + end + 2,
                        });
                    }
                }
            }
            _ => {}
        }
        for child in children(node) {
            self.extract(child, scope, depth + 1);
        }
    }
    fn collect(&mut self, root: Node<'_>) {
        let mut stack = vec![(root, false)];
        while let Some((node, inside_error)) = stack.pop() {
            let error = node.is_error() || node.is_missing();
            if error && !inside_error && self.errors.len() < 100 {
                self.errors.push((
                    node.byte_range(),
                    if node.is_missing() {
                        format!("Missing {}", node.kind())
                    } else {
                        "Incomplete or unrecognized Verilog-AMS syntax".into()
                    },
                ));
            }
            if matches!(
                node.kind(),
                "comment" | "string_literal" | "text_macro_definition"
            ) {
                continue;
            }
            if matches!(node.kind(), "simple_identifier" | "escaped_identifier") {
                self.occurrences.push(Occurrence {
                    name: spelling(&self.text, node),
                    span: node.byte_range(),
                    scope: self.scope_at(node.start_byte()),
                });
            }
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    stack.push((child, inside_error || error));
                }
            }
        }
        self.occurrences.sort_by_key(|o| o.span.start);
    }
    pub fn scope_at(&self, byte: usize) -> usize {
        self.scopes
            .iter()
            .enumerate()
            .filter(|(_, s)| s.span.start <= byte && byte < s.span.end)
            .min_by_key(|(id, s)| (s.span.len(), std::cmp::Reverse(*id)))
            .map_or(0, |(id, _)| id)
    }
    pub fn occurrence(&self, byte: usize) -> Option<&Occurrence> {
        self.occurrences
            .iter()
            .find(|o| o.span.start <= byte && byte < o.span.end)
            .or_else(|| self.occurrences.iter().find(|o| o.span.end == byte))
    }
    pub fn syntax_diagnostics(&self, encoding: Encoding) -> Vec<Diagnostic> {
        self.diagnostic_errors
            .iter()
            .chain(&self.directive_errors)
            .map(|(span, message)| Diagnostic {
                range: text::range(&self.text, span.clone(), encoding),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("vams-lsp".into()),
                code: Some(NumberOrString::String(
                    if message.starts_with("Missing") {
                        "missing-token"
                    } else {
                        "syntax"
                    }
                    .into(),
                )),
                message: message.clone(),
                ..Diagnostic::default()
            })
            .collect()
    }
    pub fn opaque_at(&self, byte: usize) -> bool {
        let n = self
            .tree
            .root_node()
            .descendant_for_byte_range(byte.min(self.text.len()), byte.min(self.text.len()));
        let mut node = n;
        while let Some(n) = node {
            if matches!(
                n.kind(),
                "comment" | "string_literal" | "text_macro_definition"
            ) {
                return true;
            }
            node = n.parent();
        }
        false
    }
}
