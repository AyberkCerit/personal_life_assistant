//! PDF import with the real PDFium, on the fixtures in tests/fixtures/pdf. Needs the library:
//!   cd apps/desktop && npm run fetch-pdfium -- --platform linux-x64   (or win-x64)
//!   cargo test -p pla-core --test pdf_live -- --ignored
//! (PLA_PDFIUM=<folder> points elsewhere.)

use std::path::PathBuf;

use pla_core::pdf::{convert, import, PdfError};
use pla_core::vault::open_vault;

fn lib() -> PathBuf {
    std::env::var_os("PLA_PDFIUM").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/src-tauri/pdfium"))
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pdf").join(name)).unwrap()
}

#[test]
#[ignore]
fn lecture_notes_become_structured_markdown() {
    let md = convert(&lib(), &fixture("ders.pdf"), "ders.pdf").unwrap().markdown;
    for want in [
        "# Nesne Yönelimli Programlama\n",
        "## 2. Kapsülleme\n",
        "### 2.1 Erişim Belirleyiciler\n",
        "```\npublic class BankaHesabi {\n    private String hesapNo;\n",
        "        if (tutar <= 0) {\n",
        "- Veri gizleme: alanlar `private` tanımlanır ve yalnızca sınıfın kendi metotları onlara dokunabilir.\n",
        "    - getter metotları okumayı,\n",
        "1. Alt sınıf, üst sınıfın public ve protected üyelerini kullanabilir.\n2. super",
        "Bir `List<BankaHesabi>` içinde",
        "her **VadeliHesap** bir **BankaHesabi**dır.",
        "\\#include gibi",
        "ayrıntıların dışarı sızmasını engeller",
        "<!-- page 2 -->",
    ] {
        assert!(md.contains(want), "missing {want:?} in\n{md}");
    }
    // the running header and the page numbers are gone
    assert!(!md.contains("Ders Notları") && !md.contains("Sayfa 1"), "{md}");
}

#[test]
#[ignore]
fn slides_with_drawn_bullets_become_lists() {
    let md = convert(&lib(), &fixture("slayt.pdf"), "slayt.pdf").unwrap().markdown;
    assert!(md.contains("## Kalıtım nedir?\n\n- Bir sınıfın başka bir sınıfın üyelerini devralması\n- \"is-a\" ilişkisi"), "{md}");
    assert!(md.contains("```\nclass Hayvan {\n    void sesCikar() { }\n}"), "{md}");
    assert!(!md.contains("BLM201"), "the footer of every slide is left out:\n{md}");
}

#[test]
#[ignore]
fn a_pdf_without_text_is_refused() {
    assert!(matches!(convert(&lib(), &fixture("tarama.pdf"), "tarama.pdf"), Err(PdfError::NoText)));
}

#[test]
#[ignore]
fn import_puts_the_pdf_and_its_note_in_the_vault_under_a_free_name() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = open_vault(tmp.path()).unwrap();
    let bytes = fixture("ders.pdf");
    let first = import(&vault, &lib(), &bytes, "OOP ders.pdf").unwrap();
    assert_eq!((first.pdf.as_str(), first.note.as_str(), first.pages), ("attachments/OOP ders.pdf", "sources/OOP ders.md", 3));
    assert_eq!(std::fs::read(tmp.path().join(&first.pdf)).unwrap(), bytes);
    let note = std::fs::read_to_string(tmp.path().join(&first.note)).unwrap();
    assert!(note.contains("source: \"[[OOP ders.pdf]]\""), "{note}");
    let second = import(&vault, &lib(), &bytes, "OOP ders.pdf").unwrap();
    assert_eq!((second.pdf.as_str(), second.note.as_str()), ("attachments/OOP ders 1.pdf", "sources/OOP ders 1.md"));
}

#[test]
#[ignore]
fn nothing_is_written_for_a_pdf_that_cannot_be_read() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = open_vault(tmp.path()).unwrap();
    assert!(matches!(import(&vault, &lib(), &fixture("tarama.pdf"), "tarama.pdf"), Err(PdfError::NoText)));
    assert!(matches!(import(&vault, &lib(), b"%PDF-1.7 bozuk", "bozuk.pdf"), Err(PdfError::Damaged)));
    assert!(std::fs::read_dir(tmp.path().join("attachments")).unwrap().next().is_none());
    assert!(!tmp.path().join("sources").exists());
}
