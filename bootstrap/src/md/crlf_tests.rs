// CRLF documents through the deterministic pipeline: parsing yields the same section
// tree as the LF text, alignment sees a line-ending conversion as no change, quotes
// locate across CRLF breaks, and a dual write keeps the document's line endings.
// Mirrors docs/compiler/parsing.md, docs/compiler/alignment.md, and
// docs/compiler/graph.md#mutations.
use crate::align::{align, Thresholds};
use crate::model::{hash_hex, DocRecord, Section};
use crate::store::ProseEdit;
use std::collections::BTreeMap;

const LF: &str = "Intro before any heading.\n\n# Shop\n\nThe shop sells goods.\n\n## Cart\n\nThe cart holds items a customer intends to buy.\nItems stay until checkout.\n\n```md\n# not a heading\n```\n\n## Orders\n\n```plantuml\n@startuml\nA -> B\n@enduml\n```\n\nAn order records the address\nand the total.\n";

fn crlf(text: &str) -> String {
    text.replace('\n', "\r\n")
}

fn shape(
    sections: &BTreeMap<String, Section>,
) -> Vec<(String, String, String, String, String, [usize; 2])> {
    sections
        .iter()
        .map(|(k, s)| {
            (
                k.clone(),
                s.title.clone(),
                s.kind.clone(),
                s.raw.clone(),
                s.hash.clone(),
                s.lines,
            )
        })
        .collect()
}

fn parsed(files: &[(&str, &str)]) -> crate::reconcile::ParsedDocs {
    files
        .iter()
        .map(|(d, t)| (d.to_string(), (hash_hex(t), super::parse_sections(t))))
        .collect()
}

fn stored(docs: &crate::reconcile::ParsedDocs) -> BTreeMap<String, DocRecord> {
    docs.iter()
        .map(|(d, (h, s))| {
            (
                d.clone(),
                DocRecord {
                    content_hash: h.clone(),
                    sections: s.clone(),
                    coverage: BTreeMap::new(),
                },
            )
        })
        .collect()
}

// The CRLF text parses to the LF tree: the same references, titles, kinds, raw
// texts (joined with LF), hashes, and line spans; fences and diagrams included.
#[test]
fn a_crlf_document_parses_to_the_lf_section_tree() {
    let lf = super::parse_sections(LF);
    let cr = super::parse_sections(&crlf(LF));
    assert_eq!(shape(&cr), shape(&lf));
    assert!(lf.contains_key("/"), "{:?}", lf.keys());
    assert!(lf.contains_key("/shop/cart"));
    assert!(!lf.keys().any(|k| k.contains("not-a-heading")));
    assert!(lf.values().any(|s| s.kind == "diagram"), "{:?}", lf);
    for s in cr.values() {
        assert!(!s.raw.contains('\r'), "{:?}", s.raw);
    }
}

// A quote wrapped across a CRLF break locates whitespace-insensitively, at the same
// line and column as in the LF text; an exact one-line quote too.
#[test]
fn quotes_locate_across_crlf_line_breaks() {
    let text = crlf(LF);
    for quote in [
        "Items stay until checkout.",
        "intends to buy. Items stay",
        "An order records the address and the total.",
    ] {
        let at_lf = super::locate(LF, quote);
        assert!(at_lf.is_some(), "{quote}");
        assert_eq!(super::locate(&text, quote), at_lf, "{quote}");
        let (b, e) = super::locate_bytes(&text, quote).unwrap();
        assert_eq!(
            text[b..e].split_whitespace().collect::<Vec<_>>(),
            quote.split_whitespace().collect::<Vec<_>>()
        );
    }
}

// Converting a document's line endings changes its content hash and nothing else:
// alignment matches every section unchanged, with no op, move, or proposal. An edit
// inside a CRLF document is an edit of that one section.
#[test]
fn a_line_ending_conversion_aligns_as_no_change() {
    let before = parsed(&[("shop.md", LF)]);
    let text = crlf(LF);
    let after = parsed(&[("shop.md", &text)]);
    assert_ne!(before["shop.md"].0, after["shop.md"].0);
    let plan = align(
        &stored(&before),
        &after,
        &crate::model::Graph::default(),
        &Thresholds::default(),
    );
    assert!(plan.ops.is_empty(), "{:?}", plan.ops);
    assert!(plan.exact_moves.is_empty() && plan.renames.is_empty());
    assert!(plan.proposals.is_empty());
    assert_eq!(plan.unchanged.len(), before["shop.md"].1.len());

    let edited = text.replace("until checkout", "until payment");
    let after = parsed(&[("shop.md", &edited)]);
    let plan = align(
        &stored(&before),
        &after,
        &crate::model::Graph::default(),
        &Thresholds::default(),
    );
    let edits: Vec<_> = plan.ops.iter().filter(|o| o.op == "edited").collect();
    assert_eq!(edits.len(), 1, "{:?}", plan.ops);
    assert_eq!(edits[0].to, vec!["shop.md#/shop/cart".to_string()]);
}

// A dual write into a CRLF document: a replacement whose old text wraps across a
// CRLF break lands in its section, a multi-line new text arrives in CRLF, and the
// result re-parses to the edited tree with no bare LF anywhere.
#[test]
fn a_dual_write_across_a_crlf_break_keeps_crlf_and_reparses() {
    let text = crlf(LF);
    let sections = super::parse_sections(&text);
    let cart = sections["/shop/cart"].raw.clone();
    let edit = ProseEdit::locate(
        "shop.md",
        "/shop/cart",
        Some(&cart),
        &text,
        "intends to buy. Items stay",
        "intends to buy.\nItems wait",
    )
    .unwrap();
    assert!(
        edit.full
            .contains("a customer intends to buy.\r\nItems wait until checkout."),
        "{:?}",
        edit.full
    );
    assert!(!edit.full.replace("\r\n", "").contains('\n'));
    let reparsed = super::parse_sections(&edit.full);
    assert_eq!(
        reparsed.keys().collect::<Vec<_>>(),
        sections.keys().collect::<Vec<_>>()
    );
    assert!(reparsed["/shop/cart"]
        .raw
        .contains("Items wait until checkout."));
    assert_eq!(reparsed["/shop/orders"].hash, sections["/shop/orders"].hash);
    // The preamble takes an append like any section.
    let pre = sections["/"].raw.clone();
    let edit = ProseEdit::locate("shop.md", "/", Some(&pre), &text, "", "A note.").unwrap();
    assert!(
        edit.full
            .starts_with("Intro before any heading.\r\n\r\nA note.\r\n\r\n# Shop"),
        "{:?}",
        edit.full
    );
}

// A document mixing line endings (one section saved by another editor) still takes
// a dual write in the section whose lines end differently from the rest.
#[test]
fn a_dual_write_into_a_mixed_line_ending_document_finds_its_section() {
    let mut text = crlf(LF);
    // The Orders section keeps LF endings; the rest of the document is CRLF.
    let at = text.find("## Orders").unwrap();
    let tail = text[at..].replace("\r\n", "\n");
    text.truncate(at);
    text.push_str(&tail);
    let sections = super::parse_sections(&text);
    let orders = sections["/shop/orders"].raw.clone();
    let edit = ProseEdit::locate(
        "shop.md",
        "/shop/orders",
        Some(&orders),
        &text,
        "",
        "An order ships within a day.",
    )
    .unwrap();
    assert!(
        edit.full.contains("and the total.")
            && edit
                .full
                .trim_end()
                .ends_with("An order ships within a day."),
        "{:?}",
        edit.full
    );
    assert!(
        edit.full.starts_with("Intro before any heading.\r\n"),
        "the CRLF part changed"
    );
    let replaced = ProseEdit::locate(
        "shop.md",
        "/shop/orders",
        Some(&orders),
        &text,
        "records the address",
        "records the street address",
    )
    .unwrap();
    assert!(replaced
        .full
        .contains("records the street address\nand the total."));
}
