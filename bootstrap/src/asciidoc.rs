//! AsciiDoc syntax projected into the shared, source-preserving section tree.
use crate::{md, model::Section};
use asciidoc_parser::{
    blocks::{Block, FindBlocks, IsBlock},
    inlines::InlineNode,
    HasSpan, Parser, SafeMode,
};
use std::{collections::BTreeMap, path::Path};

pub fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "adoc" | "asciidoc" | "asc"))
}

fn parser() -> Parser {
    Parser::default().with_safe_mode(SafeMode::Secure)
}

pub fn parse_sections(text: &str) -> BTreeMap<String, Section> {
    let doc = parser().parse(text);
    let mut heads = Vec::new();
    let mut blocks = Vec::new();
    if let Some(title) = doc.header().title_source() {
        heads.push(md::Head {
            level: 1,
            title: title.data().to_owned(),
            line: doc.origin_of(title).line - 1,
        });
    }
    for block in doc.descendant_blocks() {
        match block {
            Block::Section(section) => {
                let title = section.section_title_source();
                heads.push(md::Head {
                    level: section.level() + 1,
                    title: title.data().to_owned(),
                    line: doc.origin_of(title).line - 1,
                });
            }
            Block::RawDelimited(raw)
                if matches!(
                    block.resolved_context().as_ref(),
                    "listing" | "literal" | "plantuml" | "puml" | "uml"
                ) =>
            {
                let span = raw.span();
                let style = block.declared_style().unwrap_or("");
                let diagram = matches!(style, "plantuml" | "puml" | "uml")
                    || raw
                        .content()
                        .original()
                        .data()
                        .trim_start()
                        .starts_with("@start");
                blocks.push(md::Block {
                    start: doc.origin_of(span).line - 1,
                    end: doc
                        .source_map()
                        .origin_at(
                            span.line() + span.data().lines().count().saturating_sub(1),
                            1,
                        )
                        .line,
                    kind: if diagram { "diagram" } else { "code-block" },
                    title: style.to_owned(),
                });
            }
            _ => {}
        }
    }
    heads.sort_by_key(|h| h.line);
    md::section_tree(text, heads, blocks)
}

pub fn references(text: &str, images: bool) -> Vec<(String, std::ops::Range<usize>)> {
    fn visit(
        nodes: &[InlineNode<'_>],
        images: bool,
        out: &mut Vec<(String, std::ops::Range<usize>)>,
    ) {
        for node in nodes {
            let span = node.span();
            let range = span.byte_offset()..span.byte_offset() + span.data().len();
            match node {
                InlineNode::Ref(reference) => {
                    out.push((reference.target.to_string(), range));
                    visit(&reference.children, images, out);
                }
                InlineNode::Image(image) if images && !image.is_icon => {
                    out.push((image.target.to_string(), range))
                }
                InlineNode::Styled(styled) => visit(&styled.children, images, out),
                _ => {}
            }
        }
    }
    let doc = parser().parse(text);
    let mut out = Vec::new();
    for block in doc.descendant_blocks() {
        if !matches!(
            block.resolved_context().as_ref(),
            "listing" | "literal" | "pass" | "comment"
        ) {
            if let Some(nodes) = block.inlines() {
                visit(nodes, images, &mut out);
            }
            if images {
                if let Block::Media(media) = block {
                    if let Some(span) = media.target() {
                        out.push((
                            media.resolved_target().to_owned(),
                            span.byte_offset()..span.byte_offset() + span.data().len(),
                        ));
                    }
                }
            }
        }
    }
    // Preprocessing can remove conditional lines. Map occurrences back to the
    // original source before exposing editor ranges or attaching blob context.
    let projected = doc.span().data();
    out.into_iter()
        .filter_map(|(href, range)| {
            let offset = |byte: usize| {
                let before = projected.get(..byte)?;
                let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
                let col = before.rsplit('\n').next()?.chars().count() + 1;
                let origin = doc.source_map().origin_at(line, col);
                let col = origin.col?;
                let start: usize = text
                    .split_inclusive('\n')
                    .take(origin.line - 1)
                    .map(str::len)
                    .sum();
                let row = text.get(start..)?.split('\n').next()?;
                let column: usize = row.chars().take(col - 1).map(char::len_utf8).sum();
                Some(start + column)
            };
            Some((href, offset(range.start)?..offset(range.end)?))
        })
        .collect()
}

pub fn document_links(text: &str, from: &str) -> Vec<md::DocumentLink> {
    references(text, false)
        .into_iter()
        .filter_map(|(href, range)| {
            Some(md::DocumentLink {
                target: md::resolve_document_link(from, &href)?,
                href,
                range: md::LinkRange {
                    start: md::link_position(text, range.start),
                    end: md::link_position(text, range.end),
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_sections_preserve_source_and_ignore_literal_headings() {
        let text = "= Shop\n\n:product: Shop\n\n== Orders\nOrders must persist.\n\n[source,rust]\n----\n== Not a heading\n----\n\n=== Payment\nPayment must finish.\n\n== Orders\nRepeated.\n";
        let sections = parse_sections(text);
        assert_eq!(sections["/shop"].title, "Shop");
        assert_eq!(
            sections["/shop/orders/payment"].parent.as_deref(),
            Some("/shop/orders")
        );
        assert_eq!(sections["/shop/orders-2"].title, "Orders");
        assert_eq!(sections["/shop/orders/code-block-1"].kind, "code-block");
        assert!(!sections.values().any(|s| s.title == "Not a heading"));
        let mut prose: Vec<_> = sections
            .values()
            .filter(|s| s.kind != "code-block")
            .collect();
        prose.sort_by_key(|s| s.lines[0]);
        assert_eq!(
            prose
                .iter()
                .map(|s| s.raw.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            text.trim_end()
        );
    }

    #[test]
    fn diagrams_comments_crlf_and_incremental_hashes() {
        let text = "= Shop\n\n== Flow\n\n[plantuml]\n....\n@startuml\nA -> B\n@enduml\n....\n\n////\n== Hidden\n////\n\n== Policy\nKeep {name} verbatim.\n";
        let before = parse_sections(text);
        assert_eq!(before["/shop/flow/diagram-1"].kind, "diagram");
        assert!(!before.values().any(|s| s.title == "Hidden"));
        let crlf = parse_sections(&text.replace('\n', "\r\n"));
        for (key, section) in &before {
            assert_eq!(section.hash, crlf[key].hash);
            assert_eq!(section.lines, crlf[key].lines);
        }
        let after = parse_sections(&text.replace("Keep {name} verbatim.", "Keep orders forever."));
        assert_eq!(before["/shop/flow"].hash, after["/shop/flow"].hash);
        assert_ne!(before["/shop/policy"].hash, after["/shop/policy"].hash);
        assert!(before["/shop/policy"].raw.contains("{name}"));
    }

    #[test]
    fn conditional_lines_map_sections_and_links_to_original_source() {
        let text = "= Guide\n\nifdef::missing[]\n== Hidden\nlink:wrong.adoc[Wrong]\nendif::[]\n\n== Visible\nSee link:next.adoc[Next].\n";
        let sections = parse_sections(text);
        assert!(!sections.values().any(|s| s.title == "Hidden"));
        assert_eq!(sections["/guide/visible"].lines[0], 7);
        let links = document_links(text, "guide.adoc");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].range.start.line, 8);
        assert_eq!(links[0].range.start.character, 4);
    }

    #[test]
    fn native_links_exclude_listings_and_external_targets() {
        let text = "= Links\n\nSee link:other.adoc[Other] and xref:next.adoc#part[Next].\n\n----\nlink:ignored.adoc[Code]\n----\n";
        let links = document_links(text, "docs/main.adoc");
        assert_eq!(
            links.iter().map(|l| l.target.as_str()).collect::<Vec<_>>(),
            ["docs/other.adoc", "docs/next.adoc"]
        );
    }
}
