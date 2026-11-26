//! NeoLint Language Server Protocol entry point.

fn main() {
    if let Err(error) = neolint::lsp::run_stdio() {
        eprintln!("neolint-lsp: {error}");
        std::process::exit(1);
    }
}
