//! Clauseo's `pdftoppm`: renders PDF pages to JPEG or PNG through the Python runtime
//! (`clauseo_pdf`). No poppler code.

fn main() -> std::process::ExitCode {
    clauseo_pdf::run(clauseo_pdf::Tool::Pdftoppm)
}
