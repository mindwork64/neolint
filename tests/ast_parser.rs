use neolint::{lex, parse_ast, parse_legacy, Diagnostic, Severity};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

type NormalizedDiagnostic = (
    String,
    String,
    String,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    String,
);

fn normalized(diagnostics: &[Diagnostic]) -> Vec<NormalizedDiagnostic> {
    let mut values: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.to_owned(),
                diagnostic.severity.as_str().to_owned(),
                diagnostic.kind.as_str().to_owned(),
                diagnostic.span.start,
                diagnostic.span.end,
                diagnostic.span.line,
                diagnostic.span.column,
                diagnostic.span.end_line,
                diagnostic.span.end_column,
                diagnostic.message.clone(),
            )
        })
        .collect();
    values.sort();
    values
}

fn code_counts(diagnostics: &[Diagnostic]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for diagnostic in diagnostics {
        *counts.entry(diagnostic.code).or_insert(0) += 1;
    }
    counts
}

fn parity_differences(source: &str) -> Vec<String> {
    let (tokens, lexical) = lex(source);
    assert!(
        lexical.is_empty(),
        "fixture has lexical errors: {lexical:?}"
    );
    let legacy = parse_legacy(&tokens);
    let (_, ast) = parse_ast(&tokens);
    let mut differences = Vec::new();
    if legacy.is_empty() != ast.is_empty() {
        differences.push(format!(
            "accept/reject mismatch: legacy_empty={} ast_empty={}",
            legacy.is_empty(),
            ast.is_empty()
        ));
    }
    let legacy_codes = code_counts(&legacy);
    let ast_codes = code_counts(&ast);
    let legacy = normalized(&legacy);
    let ast = normalized(&ast);
    if legacy != ast {
        differences.push(format!(
            "per-code legacy={legacy_codes:?} ast={ast_codes:?}\nlegacy={legacy:#?}\nast={ast:#?}"
        ));
    }
    differences
}

fn visit_lsl_files(path: &Path, files: &mut Vec<std::path::PathBuf>) {
    if path.is_file() {
        if path.extension().and_then(|value| value.to_str()) == Some("lsl") {
            files.push(path.to_path_buf());
        }
        return;
    }
    let mut entries: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for entry in entries {
        visit_lsl_files(&entry, files);
    }
}

#[test]
fn full_corpus_parity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus");
    let mut files = Vec::new();
    visit_lsl_files(&root, &mut files);
    assert!(
        files.len() >= 25,
        "corpus unexpectedly small: {}",
        files.len()
    );
    let mut failures = Vec::new();
    for path in &files {
        let source = fs::read_to_string(path).unwrap();
        for difference in parity_differences(&source) {
            failures.push(format!("{}: {difference}", path.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "checked {} fixtures:\n{}",
        files.len(),
        failures.join("\n\n")
    );
}

#[test]
fn ast_recursion_guard_is_the_documented_ast_only_difference() {
    let source = format!(
        "default {{ state_entry() {{ integer value = {}1{}; }} }}",
        "(".repeat(300),
        ")".repeat(300)
    );
    let (tokens, lexical) = lex(&source);
    assert!(lexical.is_empty());
    let legacy = parse_legacy(&tokens);
    let (_, ast) = parse_ast(&tokens);
    assert!(legacy.iter().all(|diagnostic| diagnostic.code != "AST001"));
    let guard: Vec<_> = ast
        .iter()
        .filter(|diagnostic| diagnostic.code == "AST001")
        .collect();
    assert_eq!(guard.len(), 1);
    assert_eq!(guard[0].severity, Severity::Error);
    assert_eq!(guard[0].kind, neolint::DiagnosticKind::Syntax);
}

#[test]
fn focused_malformed_examples_keep_parity() {
    for source in [
        "",
        "default",
        "default {",
        "default state_entry() { }",
        "state ;",
        "integer value = ; default { state_entry() { } }",
        "integer first = 1, second = ; default { state_entry() { } }",
        "default { state_entry(",
        "default { state_entry() {",
        "default { state_entry() { integer value = ; } }",
        "default { state_entry() { integer value = 1 + ; } }",
        "default { state_entry() { list values = [1, 2; } }",
        "default { state_entry() { vector value = <1, 2; } }",
        "default { state_entry() { if () ; } }",
        "default { state_entry() { for (;;) ; } }",
        "default { state_entry() { integer value = (1)); } }",
        "default { state_entry() { integer value = (1; } }",
        "default { state_entry() { vector value = <1, 2, 3; } }",
        "default { state_entry() { @; } }",
        "integer broken( { } default { state_entry() { } }",
        "state running { state_entry() { } }",
        "void function() { } default { }",
        "default { bad( { } state_entry() { } }",
        "default { state_entry(integer) { integer local; integer local; } state_entry() { } }",
        "default { state_entry(bad, integer value) { integer value; } state_entry() { } }",
        "default {\n    state_entry(integer) {\n",
        "integer phantom()\ndefault { }\nstate phantom { }",
    ] {
        let differences = parity_differences(source);
        assert!(
            differences.is_empty(),
            "parity failure for {source:?}:\n{}",
            differences.join("\n")
        );
    }
}

#[test]
fn deterministic_generated_sources_keep_parity() {
    const PIECES: &[&str] = &[
        "default", "state", "{", "}", "(", ")", "[", "]", "<", ">", ",", ";", ":", "@", "integer",
        "float", "string", "key", "list", "vector", "rotation", "if", "else", "for", "while", "do",
        "return", "jump", "x", "y", "z", "s", ".", "=", "+", "-", "*", "/", "%", "!", "~", "++",
        "--", "==", "!=", "<=", ">=", "&&", "||", "1", "1.0", "\"text\"",
    ];
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    for _ in 0..300 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let count = (state as usize % 10) + 1;
        let mut source = String::new();
        for _ in 0..count {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            source.push_str(PIECES[state as usize % PIECES.len()]);
            source.push(' ');
        }
        let differences = parity_differences(&source);
        assert!(
            differences.is_empty(),
            "generated parity failure for {source:?}:\n{}",
            differences.join("\n")
        );
    }
}
