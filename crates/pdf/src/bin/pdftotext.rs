//! Clauseo's `pdftotext`: prints a PDF's text, recognising pages without a text layer with the
//! Python runtime's tesseract (`clauseo_pdf`). No poppler code.

fn main() -> std::process::ExitCode {
    clauseo_pdf::run(clauseo_pdf::Tool::Pdftotext)
}
