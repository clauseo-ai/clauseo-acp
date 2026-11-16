//! Clauseo's `pdfinfo`: prints a PDF's page count and details through the Python runtime
//! (`clauseo_pdf`). No poppler code.

fn main() -> std::process::ExitCode {
    clauseo_pdf::run(clauseo_pdf::Tool::Pdfinfo)
}
