use tower_lsp::lsp_types::Url;
use vams_lsp::syntax::Document;
fn main() {
    let text = std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    let doc = Document::parse(
        Url::from_file_path("/tmp/inspect.va").unwrap(),
        text,
        None,
        &Default::default(),
    )
    .unwrap();
    println!(
        "{}\nSYMBOLS: {:#?}\nOCCURRENCES: {:#?}\nINSTANCES: {:#?}\nSCOPES: {:#?}",
        doc.tree.root_node().to_sexp(),
        doc.symbols,
        doc.occurrences,
        doc.instances,
        doc.scopes
    );
}
