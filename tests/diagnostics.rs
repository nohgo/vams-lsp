use std::collections::BTreeMap;
use tower_lsp::lsp_types::Url;
use vams_lsp::{
    syntax::Document,
    text::{self, Encoding},
};
fn parse(source: &str) -> Document {
    Document::parse(
        Url::parse("file:///tmp/diagnostics.va").unwrap(),
        source.into(),
        Some(1),
        &BTreeMap::new(),
    )
    .unwrap()
}
#[test]
fn common_digital_and_analog_errors_have_specific_messages() {
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/diagnostics.json")).unwrap();
    let mut failures = Vec::new();
    for case in matrix["errors"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let doc = parse(source);
        let issues = doc.syntax_diagnostics(Encoding::Utf16);
        let messages = issues
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>();
        for expected in case["expected"].as_array().unwrap() {
            if !messages
                .iter()
                .any(|m| m.contains(expected.as_str().unwrap()))
            {
                failures.push(format!(
                    "{}: missing {}\n  got: {messages:?}\n{source}",
                    case["name"], expected
                ));
            }
        }
        for forbidden in case["forbidden"].as_array().unwrap() {
            if messages
                .iter()
                .any(|m| m.contains(forbidden.as_str().unwrap()))
            {
                failures.push(format!(
                    "{}: misleading {} in {messages:?}",
                    case["name"], forbidden
                ));
            }
        }
        for encoding in [Encoding::Utf8, Encoding::Utf16] {
            for issue in doc.syntax_diagnostics(encoding) {
                let start = text::offset(source, issue.range.start, encoding).unwrap();
                let end = text::offset(source, issue.range.end, encoding).unwrap();
                assert!(
                    start <= end && end <= source.len(),
                    "{}: {:?}",
                    case["name"],
                    issue
                );
            }
        }
        assert_eq!(source, doc.text, "diagnostic recovery changed source");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
#[test]
fn valid_neighboring_constructs_do_not_get_syntax_diagnostics() {
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/diagnostics.json")).unwrap();
    let mut failures = Vec::new();
    for source in matrix["valid"].as_array().unwrap() {
        let source = source.as_str().unwrap();
        let doc = parse(source);
        let issues = doc.syntax_diagnostics(Encoding::Utf16);
        if !issues.is_empty() {
            failures.push(format!("{source}\n{issues:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
