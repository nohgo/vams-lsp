use std::{collections::BTreeMap, fs, sync::Arc};
use tower_lsp::lsp_types::{CompletionTextEdit, InsertTextFormat, Position, Url};
use vams_lsp::{
    project::{find_root, Project},
    syntax::{Document, Kind},
    text::{self, Encoding},
    workspace::Workspace,
};

fn doc(name: &str, source: &str) -> Arc<Document> {
    Arc::new(
        Document::parse(
            Url::from_file_path(format!("/tmp/vams-test/{name}.va")).unwrap(),
            source.into(),
            Some(1),
            &BTreeMap::new(),
        )
        .unwrap(),
    )
}
fn workspace(docs: &[Arc<Document>]) -> Workspace {
    let mut ws = Workspace::new(Project::new("/tmp/vams-test".into(), "test".into()));
    for d in docs {
        ws.open.insert(d.uri.clone(), d.clone());
    }
    ws
}
fn at(doc: &Document, needle: &str) -> Position {
    text::position(&doc.text, doc.text.find(needle).unwrap(), Encoding::Utf16)
}
fn completion_text(item: &tower_lsp::lsp_types::CompletionItem) -> &str {
    match item.text_edit.as_ref().unwrap() {
        CompletionTextEdit::Edit(e) => &e.new_text,
        _ => panic!("unexpected edit"),
    }
}
const RESISTOR:&str="// A simple resistor.\nmodule resistor(p,n);\ninout n,p;\nelectrical p,n;\nparameter real r=1k;\nlocalparam real fixed=2;\nanalog I(p,n)<+V(p,n)/r;\nendmodule\n";

#[test]
fn utf_positions_roundtrip_and_surrogates() {
    let s = "a😀b\r\n// μ\n";
    for encoding in [Encoding::Utf8, Encoding::Utf16] {
        for (i, c) in s.char_indices() {
            if c == '\r' || c == '\n' {
                continue;
            }
            assert_eq!(
                Some(i),
                text::offset(s, text::position(s, i, encoding), encoding)
            );
        }
    }
    assert_eq!(None, text::offset(s, Position::new(0, 2), Encoding::Utf16));
    assert_eq!(
        Some(5),
        text::offset(s, Position::new(0, 3), Encoding::Utf16)
    );
}
#[test]
fn nonansi_ports_keep_header_order_and_defaults() {
    let d = doc("resistor", RESISTOR);
    assert!(d.errors.is_empty(), "{:?}", d.errors);
    let m = &d.modules[0];
    assert!(m.complete);
    assert_eq!(
        vec!["p", "n"],
        m.ports
            .iter()
            .map(|i| d.symbols[*i].name.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        vec!["r"],
        m.parameters
            .iter()
            .map(|i| d.symbols[*i].name.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(Some("1k"), d.symbols[m.parameters[0]].default.as_deref());
}
#[test]
fn ansi_ports_and_connectmodules() {
    let d = doc(
        "ansi",
        "connectmodule adapter(input electrical p, n, output electrical q); endmodule\n",
    );
    assert!(d.errors.is_empty(), "{:?}", d.errors);
    assert_eq!(3, d.modules[0].ports.len());
    assert_eq!(
        vec!["p", "n", "q"],
        d.modules[0]
            .ports
            .iter()
            .map(|i| d.symbols[*i].name.as_str())
            .collect::<Vec<_>>()
    );
}
#[test]
fn cross_file_module_snippet_and_plain_text_fallback() {
    let model = doc("resistor", RESISTOR);
    let top = doc("top", "module top;\n  resi\nendmodule\n");
    let ws = workspace(&[model, top.clone()]);
    let pos = text::position(
        &top.text,
        top.text.find("resi").unwrap() + 4,
        Encoding::Utf16,
    );
    for snippets in [true, false] {
        let items = ws.completion(&top.uri, pos, Encoding::Utf16, snippets);
        let item = items
            .iter()
            .find(|i| i.label == "resistor")
            .unwrap_or_else(|| {
                panic!(
                    "modules {:?}, scopes {:?}, items {:?}",
                    top.modules,
                    top.scopes,
                    items.iter().map(|i| &i.label).collect::<Vec<_>>()
                )
            });
        let content = completion_text(item);
        assert!(content.contains(".p("));
        assert!(content.contains(".n("));
        assert!(!content.contains("fixed"));
        assert_eq!(snippets, content.contains("${1:u_resistor}"));
        assert_eq!(
            Some(if snippets {
                InsertTextFormat::SNIPPET
            } else {
                InsertTextFormat::PLAIN_TEXT
            }),
            item.insert_text_format
        );
        assert!(
            content.contains(if snippets { "\n  .p(" } else { "\n    .p(" }),
            "{content}"
        );
    }
}
#[test]
fn optional_parameter_defaults_and_empty_connections() {
    let model = doc("resistor", RESISTOR);
    let top = doc("top", "module top;\nres\nendmodule");
    let mut ws = workspace(&[model, top.clone()]);
    ws.project.config.completion.parameter_overrides = "defaults".into();
    ws.project.config.completion.connections = "empty".into();
    let pos = text::position(
        &top.text,
        top.text.find("res\n").unwrap() + 3,
        Encoding::Utf16,
    );
    let items = ws.completion(&top.uri, pos, Encoding::Utf16, true);
    let item = items.iter().find(|i| i.label == "resistor").unwrap();
    assert!(completion_text(item).contains(".r(${1:1k})"));
    assert!(completion_text(item).contains(".p(${3:})"));
}
#[test]
fn no_module_expansion_in_comments_strings_or_analog_expressions() {
    let model = doc("resistor", RESISTOR);
    for source in [
        "module top;\n// resi\nendmodule",
        "module top;\nparameter string x=\"resi\";\nendmodule",
        "module top; real x; analog x=resi; endmodule",
    ] {
        let top = doc("top", source);
        let ws = workspace(&[model.clone(), top.clone()]);
        let pos = text::position(source, source.find("resi").unwrap() + 4, Encoding::Utf16);
        assert!(!ws
            .completion(&top.uri, pos, Encoding::Utf16, true)
            .iter()
            .any(|i| i.label == "resistor"));
    }
}
#[test]
fn definitions_named_labels_and_parent_connections_are_distinct() {
    let model = doc("resistor", RESISTOR);
    let top = doc(
        "top",
        "module top; electrical p,n; resistor u(.p(p),.n(n)); endmodule",
    );
    let ws = workspace(&[model.clone(), top.clone()]);
    let module = ws.definitions(&top.uri, at(&top, "resistor"), Encoding::Utf16);
    assert_eq!(1, module.len());
    assert_eq!(model.uri, module[0].uri);
    let port = text::position(
        &top.text,
        top.text.find(".p(p)").unwrap() + 1,
        Encoding::Utf16,
    );
    assert_eq!(
        model.uri,
        ws.definitions(&top.uri, port, Encoding::Utf16)[0].uri
    );
    let signal = text::position(
        &top.text,
        top.text.find(".p(p)").unwrap() + 3,
        Encoding::Utf16,
    );
    assert_eq!(
        top.uri,
        ws.definitions(&top.uri, signal, Encoding::Utf16)[0].uri
    );
    assert!(ws
        .hover(&top.uri, at(&top, "resistor"), Encoding::Utf16)
        .is_some());
}
#[test]
fn ambiguous_modules_never_choose_arbitrary_target() {
    let a = doc("a", RESISTOR);
    let b = doc("b", RESISTOR);
    let top = doc("top", "module top; resistor u(); endmodule");
    let ws = workspace(&[a, b, top.clone()]);
    assert_eq!(
        2,
        ws.definitions(&top.uri, at(&top, "resistor"), Encoding::Utf16)
            .len()
    );
}
#[test]
fn unsaved_buffers_override_disk_and_stale_versions_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("m.va");
    fs::write(&file, "module before; endmodule").unwrap();
    let scan = Project::new(dir.path().to_owned(), "test".into()).scan(Default::default());
    let mut ws = Workspace::new(scan.project.clone());
    ws.merge(scan);
    let uri = Url::from_file_path(file).unwrap();
    assert!(ws.update(uri.clone(), "module after; endmodule".into(), 5));
    assert!(!ws.update(uri.clone(), "module stale; endmodule".into(), 4));
    assert_eq!(1, ws.globals("after", Some(Kind::Module)).len());
    assert!(ws.globals("before", Some(Kind::Module)).is_empty());
    ws.open.remove(&uri);
    assert_eq!(1, ws.globals("before", Some(Kind::Module)).len());
}
#[test]
fn scopes_shadow_without_mixing_references() {
    let d=doc("scope","module scope; real x; analog function real f; input x; real x; begin f=x; end endfunction analog x=1; endmodule");
    assert!(d.errors.is_empty(), "{:?}", d.errors);
    let ws = workspace(&[d.clone()]);
    let function = text::position(&d.text, d.text.find("f=x").unwrap() + 2, Encoding::Utf16);
    let outer = text::position(
        &d.text,
        d.text.find("analog x").unwrap() + 7,
        Encoding::Utf16,
    );
    assert_ne!(
        ws.definitions(&d.uri, function, Encoding::Utf16),
        ws.definitions(&d.uri, outer, Encoding::Utf16)
    );
    assert_eq!(
        3,
        ws.references(&d.uri, function, Encoding::Utf16, true).len()
    );
}
#[test]
fn function_signature_and_safe_local_rename() {
    let d=doc("function","module m; analog function real f; input x,y; real x,y; begin f=x+y; end endfunction real z; analog z=f(1,2); endmodule");
    assert!(d.errors.is_empty(), "{:?}", d.errors);
    let ws = workspace(&[d.clone()]);
    let pos = text::position(&d.text, d.text.find("f(1,2)").unwrap() + 5, Encoding::Utf16);
    let signature = ws.signature(&d.uri, pos, Encoding::Utf16).unwrap();
    assert_eq!("f(x, y)", signature.signatures[0].label);
    assert_eq!(Some(1), signature.active_parameter);
    let local = text::position(&d.text, d.text.find("x+y").unwrap(), Encoding::Utf16);
    assert!(ws.rename(&d.uri, local, Encoding::Utf16, "value").is_some());
    assert!(ws.rename(&d.uri, local, Encoding::Utf16, "y").is_none());
    assert!(ws
        .rename(&d.uri, local, Encoding::Utf16, "analog")
        .is_none());
    assert!(ws
        .rename(&d.uri, at(&d, "z;"), Encoding::Utf16, "new_z")
        .is_none());
}
#[test]
fn conditional_selection_preserves_original_offsets_and_comments() {
    let src = "`ifdef FAST\nmodule fast; endmodule\n`else\nmodule slow; endmodule\n`endif\n";
    let uri = Url::from_file_path("/tmp/conditional.va").unwrap();
    let d = Document::parse(uri.clone(), src.into(), None, &BTreeMap::new()).unwrap();
    assert_eq!("slow", d.symbols[d.modules[0].symbol].name);
    assert!(d.errors.is_empty());
    let mut defs = BTreeMap::new();
    defs.insert("FAST".into(), "1".into());
    let d = Document::parse(uri, src.into(), None, &defs).unwrap();
    assert_eq!("fast", d.symbols[d.modules[0].symbol].name);
    assert_eq!(
        src.find("fast").unwrap(),
        d.symbols[d.modules[0].symbol].span.start
    );
    let d = doc("comments", "/*\n`ifdef NEVER\n*/\nmodule yes; endmodule");
    assert!(d.directive_errors.is_empty());
    assert_eq!(1, d.modules.len());
}
#[test]
fn syntax_errors_repair_and_incomplete_header_not_expanded() {
    let broken = doc("broken", "module broken(input electrical p; endmodule");
    assert!(!broken.errors.is_empty());
    assert!(broken.modules.iter().all(|m| !m.complete));
    let fixed = doc("fixed", "module fixed(input electrical p); endmodule");
    assert!(fixed.syntax_diagnostics(Encoding::Utf16).is_empty());
    let directive = doc("directive", "`ifdef X\nmodule m; endmodule\n");
    assert!(!directive.directive_errors.is_empty());
}
#[test]
fn port_label_completion_and_conservative_named_connection_warnings() {
    let model = doc("resistor", RESISTOR);
    let top = doc(
        "top",
        "module top; electrical p,n; resistor #(.r(1k)) u(.p(p),.bogus(n)); endmodule",
    );
    let ws = workspace(&[model, top.clone()]);
    assert!(top.errors.is_empty(), "{:?}", top.errors);
    let diagnostics = ws.diagnostics(&top, Encoding::Utf16);
    assert_eq!(1, diagnostics.len());
    assert!(diagnostics[0].message.contains("bogus"));
    let pos = text::position(
        &top.text,
        top.text.find(".bogus").unwrap() + 1,
        Encoding::Utf16,
    );
    let names = ws
        .completion(&top.uri, pos, Encoding::Utf16, true)
        .into_iter()
        .map(|i| i.label)
        .collect::<Vec<_>>();
    assert_eq!(vec!["n"], names);
}
#[test]
fn declared_ams_types_branches_and_workspace_symbols() {
    let d=doc("types","nature Voltage; access=V; units=\"V\"; endnature\ndiscipline electrical; potential Voltage; enddiscipline\nmodule m(p); inout p; electrical p; branch(p) named_branch; analog I(named_branch)<+0; endmodule");
    assert!(d.errors.is_empty(), "{:?}", d.errors);
    let ws = workspace(&[d.clone()]);
    assert_eq!(1, ws.workspace_symbols("Voltage", Encoding::Utf16).len());
    assert_eq!(3, ws.document_symbols(&d.uri, Encoding::Utf16).len());
    let p = text::position(
        &d.text,
        d.text.find("I(named_branch)").unwrap() + 2,
        Encoding::Utf16,
    );
    assert_eq!(1, ws.definitions(&d.uri, p, Encoding::Utf16).len());
}
#[test]
fn root_discovery_respects_git_barriers_and_worktree_files() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join(".vams.json"), "{}").unwrap();
    let nested = d.path().join("child");
    fs::create_dir_all(nested.join("src")).unwrap();
    fs::write(nested.join(".git"), "gitdir: elsewhere").unwrap();
    let file = nested.join("src/model.va");
    let (root, reason) = find_root(&file);
    assert_eq!(nested, root);
    assert_eq!("git", reason);
    fs::write(nested.join("src/.vams.json"), "{}").unwrap();
    assert_eq!(nested.join("src"), find_root(&file).0);
}
#[test]
fn project_scans_include_graph_ignore_output_and_refresh_deletions() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    fs::write(p.join(".vams.json"), "{}").unwrap();
    fs::write(
        p.join("m.va"),
        "`include \"include/h.vah\"\nmodule m; endmodule",
    )
    .unwrap();
    fs::create_dir(p.join("include")).unwrap();
    fs::write(p.join("include/h.vah"), "nature X; access=XX; endnature").unwrap();
    fs::create_dir(p.join("build")).unwrap();
    fs::write(p.join("build/no.va"), "module no; endmodule").unwrap();
    fs::write(p.join(".gitignore"), "ignored.va\n").unwrap();
    fs::write(p.join("ignored.va"), "module ignored; endmodule").unwrap();
    let first = Project::new(p.to_owned(), "test".into()).scan(Default::default());
    assert_eq!(2, first.files.len(), "{:?}", first.files.keys());
    fs::remove_file(p.join("m.va")).unwrap();
    let next = first.project.scan(first.files);
    assert!(next.files.is_empty());
}
#[test]
fn invalid_configuration_preserves_last_valid_project() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join(".vams.json");
    fs::write(&f, "{\"include_dirs\":[\"headers\"]}").unwrap();
    let mut project = Project::new(d.path().to_owned(), "test".into());
    assert_eq!(vec!["headers"], project.config.include_dirs);
    fs::write(&f, "{\"typo\":true}").unwrap();
    assert!(!project.reload());
    assert!(project.config_error.is_some());
    assert_eq!(vec!["headers"], project.config.include_dirs);
}
#[test]
fn escaped_ports_and_crlf_snippet_escaping() {
    let model = doc(
        "escaped",
        "module \\special.name (\\p.name ,n); inout \\p.name ,n; endmodule",
    );
    assert!(model.errors.is_empty(), "{:?}", model.errors);
    let top = doc("top", "module top;\r\n  spec\r\nendmodule\r\n");
    let ws = workspace(&[model, top.clone()]);
    let pos = text::position(
        &top.text,
        top.text.find("spec").unwrap() + 4,
        Encoding::Utf16,
    );
    let items = ws.completion(&top.uri, pos, Encoding::Utf16, true);
    let item = items.iter().find(|i| i.label == "\\special.name").unwrap();
    assert!(completion_text(item).contains("\\\\p.name "));
    assert!(completion_text(item).contains("\r\n"));
}
#[test]
fn bounded_scanning_and_explicit_external_libraries() {
    let project = tempfile::tempdir().unwrap();
    let library = tempfile::tempdir().unwrap();
    fs::write(library.path().join("shared.va"), RESISTOR).unwrap();
    fs::write(
        project.path().join(".vams.json"),
        serde_json::json!({"source_dirs":[],"library_dirs":[library.path()],"max_files":1})
            .to_string(),
    )
    .unwrap();
    let scan = Project::new(project.path().to_owned(), "test".into()).scan(Default::default());
    assert_eq!(1, scan.files.len());
    assert_eq!(
        fs::canonicalize(library.path()).unwrap(),
        scan.files.keys().next().unwrap().parent().unwrap()
    );
}

#[test]
fn unmarked_file_root_does_not_walk_unrelated_subdirectories() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("unrelated")).unwrap();
    fs::write(dir.path().join("m.va"), "module m; endmodule").unwrap();
    fs::write(dir.path().join("unrelated/n.va"), "module n; endmodule").unwrap();
    let scan = Project::new(dir.path().to_owned(), "client".into()).scan(Default::default());
    assert_eq!(1, scan.files.len());
    fs::write(dir.path().join(".vams.json"), "{}").unwrap();
    let scan = Project::new(dir.path().to_owned(), "client".into()).scan(Default::default());
    assert_eq!(2, scan.files.len());
}

#[test]
fn module_completion_survives_unrelated_syntax_errors() {
    let model = doc("resistor", RESISTOR);
    let sources = [
        "module top;\n  electrical p,n\n  resi\nendmodule\n",
        "module top;\n  resi\n",
        "module test2;\nresist\n",
        "module top;\n  electrical p,n\n  resi\n",
        "module top;\n  ???\n  resi\nendmodule\n",
        "module top;\n  resistor broken(.p(), .n()\n  resi\nendmodule\n",
        "module broken; parameter real x = ; endmodule\nmodule top;\n  resi\nendmodule\n",
        "module top;\n  resi\nendmodule\nmodule broken( ;\n",
        "module top;\n  parameter real r = ;\n  resi\nendmodule\n",
        "module top;\n  resi\n  analog begin\n    V(p,n) <+ 1\n  end\nendmodule\n",
        "module top;\n  analog begin\n    V(p,n) <+ ;\n  end\n  resi\nendmodule\n",
        "module top;\n  resi\n  analog begin\n    V(p,n) <+ 1;\nendmodule\n",
    ];
    let mut failures = Vec::new();
    for source in sources {
        let top = doc("top", source);
        let ws = workspace(&[model.clone(), top.clone()]);
        let prefix_start = source.rfind("resi").unwrap();
        let cursor = source[prefix_start..]
            .find('\n')
            .map_or(source.len(), |n| prefix_start + n);
        let pos = text::position(source, cursor, Encoding::Utf16);
        let items = ws.completion(&top.uri, pos, Encoding::Utf16, true);
        if !items
            .iter()
            .any(|i| i.label == "resistor" && completion_text(i).contains(".p("))
        {
            failures.push(format!("{source}\n{}", top.tree.root_node().to_sexp()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn incomplete_module_completion_stays_out_of_procedural_and_opaque_contexts() {
    let model = doc("resistor", RESISTOR);
    for source in [
        "module top\n  resist\nendmodule\n",
        "module top;\nanalog begin\n  resist\n",
        "module top;\nanalog begin\n  if (1) begin\n    resist\n",
        "module top;\nanalog\n  resist\n",
        "module top;\ninitial begin\n  resist\n",
        "module top;\nanalog function real f;\ninput x; real x;\nbegin\n  resist\n",
        "module top;\nfunction real f;\n  resist\n",
        "module top;\n// resist\n",
        "module top;\n/* resist",
        "module top;\nparameter string x = \"resist",
        "module top; endmodule\nresist\n",
        "resist\n",
    ] {
        let top = doc("top", source);
        let ws = workspace(&[model.clone(), top.clone()]);
        let pos = text::position(source, source.find("resist").unwrap() + 6, Encoding::Utf16);
        assert!(
            !ws.completion(&top.uri, pos, Encoding::Utf16, true)
                .iter()
                .any(|i| i.label == "resistor"),
            "module expansion in unsafe context: {source}\n{}",
            top.tree.root_node().to_sexp()
        );
    }
}
