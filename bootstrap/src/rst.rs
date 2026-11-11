//! Source-preserving reStructuredText projection using the upstream Pest grammar.
use crate::{md, model::Section};
use pest::{iterators::Pair, Parser};
use pest_derive::Parser;
use std::{collections::BTreeMap, ops::Range, path::Path};

#[derive(Parser)]
#[grammar = "vendor/rst-grammar/rst.pest"]
struct RstParser;

pub fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "rst" | "rest"))
}

fn source(text: &str) -> String {
    if text.ends_with('\n') {
        text.to_owned()
    } else {
        format!("{text}\n")
    }
}

pub fn parse_sections(text: &str) -> Result<BTreeMap<String, Section>, String> {
    if text.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let source = source(text);
    let pairs = RstParser::parse(Rule::document, &source)
        .map_err(|e| format!("invalid reStructuredText: {e}"))?;
    let mut heads = Vec::new();
    let mut blocks = Vec::new();
    let mut styles = Vec::new();
    for pair in pairs {
        match pair.as_rule() {
            Rule::title => {
                let start = pair.as_span().start_pos().line_col().0 - 1;
                let title = pair.into_inner().next().expect("title grammar");
                let overline = title.as_rule() == Rule::title_double;
                let mut name = String::new();
                let mut adornment = '=';
                for part in title.into_inner() {
                    match part.as_rule() {
                        Rule::line => name = part.as_str().trim().to_owned(),
                        Rule::adornments => adornment = part.as_str().chars().next().unwrap_or('='),
                        _ => {}
                    }
                }
                let style = (adornment, overline);
                let level = if let Some(i) = styles.iter().position(|s| *s == style) {
                    i + 1
                } else {
                    styles.push(style);
                    styles.len()
                };
                heads.push(md::Head {
                    level,
                    title: name,
                    line: start,
                });
            }
            Rule::code_directive | Rule::literal_block | Rule::raw_directive => {
                let span = pair.as_span();
                let diagram = pair.as_rule() == Rule::code_directive
                    && pair.clone().into_inner().any(|part| {
                        part.as_rule() == Rule::source
                            && matches!(part.as_str().trim(), "plantuml" | "puml" | "uml")
                    });
                blocks.push(md::Block {
                    start: span.start_pos().line_col().0 - 1,
                    end: (span.start_pos().line_col().0 - 1 + span.as_str().lines().count())
                        .min(text.lines().count()),
                    kind: if diagram { "diagram" } else { "code-block" },
                    title: if diagram {
                        "plantuml".into()
                    } else {
                        String::new()
                    },
                });
            }
            _ => {}
        }
    }
    Ok(md::section_tree(text, heads, blocks))
}

pub fn references(text: &str, images: bool) -> Vec<(String, Range<usize>)> {
    fn visit(
        pair: Pair<'_, Rule>,
        targets: &BTreeMap<String, String>,
        images: bool,
        out: &mut Vec<(String, Range<usize>)>,
    ) {
        let span = pair.as_span();
        let range = span.start()..span.end();
        match pair.as_rule() {
            Rule::code_directive
            | Rule::literal_block
            | Rule::raw_directive
            | Rule::block_comment => return,
            Rule::relative_reference | Rule::reference_source | Rule::link_target => {
                out.push((pair.as_str().trim().to_owned(), range))
            }
            Rule::reference_target_uq | Rule::reference_text => {
                if let Some(href) = targets.get(&pair.as_str().trim().to_lowercase()) {
                    out.push((href.clone(), range));
                }
            }
            Rule::image if images => {
                if let Some(line) = pair
                    .clone()
                    .into_inner()
                    .find(|p| p.as_rule() == Rule::line)
                {
                    out.push((line.as_str().trim().to_owned(), range));
                }
                return;
            }
            Rule::image => return,
            _ => {}
        }
        for child in pair.into_inner() {
            visit(child, targets, images, out);
        }
    }
    let source = source(text);
    let Ok(pairs) = RstParser::parse(Rule::document, &source) else {
        return Vec::new();
    };
    let mut targets = BTreeMap::new();
    for pair in pairs.clone().filter(|p| p.as_rule() == Rule::target) {
        let mut name = None;
        let mut href = None;
        for child in pair.into_inner() {
            match child.as_rule() {
                Rule::target_name_uq | Rule::target_name_qu => {
                    name = Some(child.as_str().trim().to_lowercase())
                }
                Rule::link_target => href = Some(child.as_str().trim().to_owned()),
                _ => {}
            }
        }
        if let (Some(name), Some(href)) = (name, href) {
            targets.insert(name, href);
        }
    }
    let mut out = Vec::new();
    for pair in pairs {
        visit(pair, &targets, images, &mut out);
    }
    out.retain(|(_, r)| r.end <= text.len());
    out
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
    fn nested_headings_preserve_source_and_ignore_literals() {
        let text = "Guide\n=====\n\nOrders\n------\n\nOrders must persist.\n\n.. code-block:: text\n\n   Fake\n   ====\n\nPayment\n~~~~~~~\n\nConfirm payment.\n\nOrders\n------\n\nRetain records.";
        let sections = parse_sections(text).unwrap();
        assert_eq!(
            sections["/guide/orders/payment"].parent.as_deref(),
            Some("/guide/orders")
        );
        assert!(sections.contains_key("/guide/orders-2"));
        assert!(sections.contains_key("/guide/orders/code-block-1"));
        assert!(!sections.values().any(|s| s.title == "Fake"));
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
            text
        );
    }
    #[test]
    fn literal_introductions_and_comments_are_not_prose_links() {
        let text = "Guide\n=====\n\nExample::\n\n   Fake\n   ====\n   `code <wrong.rst>`_\n\n.. A comment\n   `hidden <hidden.rst>`_\n\nSee `real <real.rst>`_.\n";
        let sections = parse_sections(text).unwrap();
        assert!(sections.contains_key("/guide/code-block-1"));
        assert!(!sections.values().any(|s| s.title == "Fake"));
        let links = document_links(text, "main.rst");
        assert_eq!(
            links.iter().map(|l| l.target.as_str()).collect::<Vec<_>>(),
            ["real.rst"]
        );
    }

    #[test]
    fn original_link_ranges_and_crlf_sections() {
        let text = "Guide\n=====\n\nSee `next <next.rst>`_.\n";
        let links = document_links(text, "docs/guide.rst");
        assert_eq!(links[0].target, "docs/next.rst");
        let lf = parse_sections(text).unwrap();
        let crlf = parse_sections(&text.replace('\n', "\r\n")).unwrap();
        assert_eq!(lf["/guide"].hash, crlf["/guide"].hash);
    }
}
