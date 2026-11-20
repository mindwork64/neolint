use neolint::lex;
use neolint::{Diagnostic, Severity};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Human,
    Json,
}

struct FileResult {
    path: String,
    tokens: Vec<neolint::Token>,
    diagnostics: Vec<Diagnostic>,
}

fn main() {
    let code = run();
    process::exit(code);
}

fn run() -> i32 {
    let mut format = OutputFormat::Human;
    let mut show_tokens = false;
    let mut paths = Vec::new();
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return 0;
            }
            "-V" | "--version" => {
                println!("neolint {}", env!("CARGO_PKG_VERSION"));
                return 0;
            }
            "--tokens" => show_tokens = true,
            "--format" => {
                let Some(value) = args.next() else {
                    eprintln!("neolint: --format requires 'human' or 'json'");
                    return 2;
                };
                format = match value.as_str() {
                    "human" => OutputFormat::Human,
                    "json" => OutputFormat::Json,
                    _ => {
                        eprintln!("neolint: unsupported output format '{value}'");
                        return 2;
                    }
                };
            }
            _ if arg.starts_with('-') && arg != "-" => {
                eprintln!("neolint: unknown option '{arg}'");
                eprintln!("Try 'neolint --help' for usage.");
                return 2;
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        print_help();
        return 2;
    }

    if show_tokens && format == OutputFormat::Json {
        eprintln!("neolint: --tokens cannot be combined with --format json");
        return 2;
    }

    let mut results = Vec::new();
    let mut input_failed = false;
    let mut stdin_used = false;

    for path in paths {
        let source = if path == "-" {
            if stdin_used {
                eprintln!("neolint: stdin ('-') may only be specified once");
                input_failed = true;
                continue;
            }
            stdin_used = true;
            let mut input = String::new();
            if let Err(error) = io::stdin().read_to_string(&mut input) {
                eprintln!("neolint: cannot read stdin: {error}");
                input_failed = true;
                continue;
            }
            input
        } else {
            match fs::read_to_string(&path) {
                Ok(source) => source,
                Err(error) => {
                    eprintln!("neolint: cannot read '{path}': {error}");
                    input_failed = true;
                    continue;
                }
            }
        };

        let (tokens, diagnostics) = lex(&source);
        results.push(FileResult {
            path,
            tokens,
            diagnostics,
        });
    }

    match format {
        OutputFormat::Human => print_human(&results, show_tokens),
        OutputFormat::Json => print_json(&results),
    }

    let has_errors = results
        .iter()
        .flat_map(|result| &result.diagnostics)
        .any(|diagnostic| diagnostic.severity == Severity::Error);

    if input_failed {
        2
    } else if has_errors {
        1
    } else {
        0
    }
}

fn print_help() {
    println!(
        "neolint {} — CLI-only LSL linter\n\nUSAGE:\n    neolint [OPTIONS] <FILE...|->\n\nOPTIONS:\n    --format <human|json>  Diagnostic output format (default: human)\n    --tokens               Print lexer tokens (development aid)\n    -h, --help             Print help\n    -V, --version          Print version\n\nEXIT CODES:\n    0  No lexical errors\n    1  Lexical errors found\n    2  Input or command-line error\n\nNOTE: This initial build performs lexical analysis only. Parser, semantic, and lint rules are being implemented.",
        env!("CARGO_PKG_VERSION")
    );
}

fn print_human(results: &[FileResult], show_tokens: bool) {
    for result in results {
        if show_tokens {
            for token in &result.tokens {
                println!(
                    "{}:{}:{}: {:?} {:?}",
                    result.path,
                    token.span.line,
                    token.span.column,
                    token.kind,
                    token.lexeme
                );
            }
        }

        if result.diagnostics.is_empty() {
            if !show_tokens {
                println!(
                    "{}: no lexical errors (parser and lint rules are not implemented yet)",
                    result.path
                );
            }
        } else {
            for diagnostic in &result.diagnostics {
                println!(
                    "{}:{}:{}: {}[{}]: {}",
                    result.path,
                    diagnostic.span.line,
                    diagnostic.span.column,
                    diagnostic.severity.as_str(),
                    diagnostic.code,
                    diagnostic.message
                );
            }
        }
    }
}

fn print_json(results: &[FileResult]) {
    let mut first = true;
    print!("[");
    for result in results {
        for diagnostic in &result.diagnostics {
            if !first {
                print!(",");
            }
            first = false;
            print!(
                "{{\"file\":\"{}\",\"code\":\"{}\",\"severity\":\"{}\",\"kind\":\"{}\",\"message\":\"{}\",\"line\":{},\"column\":{},\"end_line\":{},\"end_column\":{},\"start\":{},\"end\":{}}}",
                json_escape(&result.path),
                diagnostic.code,
                diagnostic.severity.as_str(),
                diagnostic.kind.as_str(),
                json_escape(&diagnostic.message),
                diagnostic.span.line,
                diagnostic.span.column,
                diagnostic.span.end_line,
                diagnostic.span.end_column,
                diagnostic.span.start,
                diagnostic.span.end
            );
        }
    }
    println!("]");
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", ch as u32);
            }
            ch => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::json_escape;

    #[test]
    fn json_escaping_handles_quotes_slashes_and_control_chars() {
        assert_eq!(
            json_escape("a\"b\\c\n"),
            "a\\\"b\\\\c\\n"
        );
    }
}
