//! Runs beside the extracted release binary so native libraries resolve identically.
fn main() {
    let svg = jazyk::render::render_svg(
        "@startuml\nclass Release\nclass Archive\nRelease --> Archive\n@enduml\n",
    )
    .expect("bundled Graphviz renders diagrams");
    assert!(svg.contains("<svg"));
    let pdf = jazyk::documents::from_bytes(
        std::path::Path::new("contract.pdf"),
        include_bytes!("../tests/fixtures/document-proof/contract.pdf"),
    )
    .expect("adjacent bundled PDFium reads PDFs");
    assert!(pdf.text.contains("[contract](missing.md)"));
    let docx = jazyk::documents::from_bytes(
        std::path::Path::new("contract.docx"),
        include_bytes!("../tests/fixtures/document-proof/contract.docx"),
    )
    .expect("native DOCX reading works");
    assert!(!docx.text.is_empty());
    println!("Distribution smoke passed: diagrams, PDF, DOCX");
}
