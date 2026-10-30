//! Converts one PDF to the Markdown PDF import would write, and prints it with a few numbers.
//!   cd apps/desktop && npm run fetch-pdfium -- --platform linux-x64   (or win-x64)
//!   cargo run -p pla-core --example pdf_to_md -- apps/desktop/src-tauri/pdfium <file.pdf> [out.md]

use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [lib, pdf, rest @ ..] = args.as_slice() else {
        eprintln!("usage: pdf_to_md <pdfium folder> <file.pdf> [out.md]");
        std::process::exit(2);
    };
    let bytes = std::fs::read(pdf).expect("read the PDF");
    let name = PathBuf::from(pdf).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let started = Instant::now();
    match pla_core::pdf::convert(&PathBuf::from(lib), &bytes, &name) {
        Ok(c) => {
            match rest.first() {
                Some(out) => std::fs::write(out, &c.markdown).expect("write the Markdown"),
                None => print!("{}", c.markdown),
            }
            let tokens = pla_core::util::tokens(&c.markdown);
            eprintln!("{} pages, {} characters, ~{tokens} tokens, {:?}", c.pages, c.chars, started.elapsed());
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
