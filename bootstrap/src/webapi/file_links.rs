// Markdown destinations only. Parser ranges keep code and prose untouched.
use pulldown_cmark::{Event, LinkType, Parser, Tag};

fn destination(url: &str, source: &str, target: &str, from: &str, to: &str) -> Option<String> {
    if url.is_empty() || url.starts_with('#') || url.starts_with("//") || url.contains(':') {
        return None;
    }
    let split = url.find(['#', '?']).unwrap_or(url.len());
    let path = percent_encoding::percent_decode_str(&url[..split])
        .decode_utf8()
        .ok()?;
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = if absolute {
        vec![]
    } else {
        source.split('/').collect::<Vec<_>>()[..source.split('/').count() - 1].to_vec()
    };
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    let resolved = parts.join("/");
    let resolved = if resolved == from { to } else { &resolved };
    let result = if absolute {
        format!("/{resolved}")
    } else {
        let base: Vec<_> = target
            .rsplit_once('/')
            .map(|(d, _)| d.split('/').collect())
            .unwrap_or_default();
        let dest: Vec<_> = resolved.split('/').collect();
        let common = base.iter().zip(&dest).take_while(|(a, b)| a == b).count();
        let mut rel = vec![".."; base.len() - common];
        rel.extend_from_slice(&dest[common..]);
        let rel = rel.join("/");
        if url.starts_with("./") && !rel.starts_with("..") {
            format!("./{rel}")
        } else {
            rel
        }
    };
    // Keep original spelling when resolution did not change.
    if source == target && parts.join("/") != from {
        return None;
    }
    const URL_PATH: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
        .add(b' ')
        .add(b'(')
        .add(b')')
        .add(b'<')
        .add(b'>')
        .add(b'"')
        .add(b'%')
        .add(b'#')
        .add(b'?');
    let encoded = percent_encoding::utf8_percent_encode(&result, URL_PATH).to_string();
    Some(format!("{encoded}{}", &url[split..]))
}

pub fn rewrite(text: &str, source: &str, target: &str, from: &str, to: &str) -> String {
    let parser = Parser::new(text);
    let mut spans: Vec<_> = parser
        .reference_definitions()
        .iter()
        .map(|(_, def)| (def.span.clone(), def.dest.to_string()))
        .collect();
    for (event, range) in parser.into_offset_iter() {
        if let Event::Start(
            Tag::Link {
                link_type: LinkType::Inline,
                dest_url,
                ..
            }
            | Tag::Image {
                link_type: LinkType::Inline,
                dest_url,
                ..
            },
        ) = event
        {
            spans.push((range, dest_url.to_string()));
        }
    }
    let mut edits = Vec::new();
    for (range, url) in spans {
        if let Some(next) = destination(&url, source, target, from, to) {
            // Destinations follow the closing label, or the reference definition's colon.
            let raw = &text[range.clone()];
            let start = raw
                .rfind("](")
                .map(|i| i + 2)
                .or_else(|| raw.find("]:").map(|i| i + 2));
            if let Some(start) = start {
                let suffix = &raw[start..];
                let whitespace = suffix.len() - suffix.trim_start().len();
                let offset = start + whitespace + usize::from(suffix.trim_start().starts_with('<'));
                if raw[offset..].starts_with(&url) {
                    edits.push((range.start + offset, range.start + offset + url.len(), next));
                }
            }
        }
    }
    edits.sort_by_key(|e| e.0);
    edits.dedup_by_key(|e| e.0);
    let mut result = text.to_string();
    for (start, end, next) in edits.into_iter().rev() {
        result.replace_range(start..end, &next);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moves_incoming_outgoing_images_and_definitions_without_touching_code() {
        let text = "[in](../a.md#part) ![image](pic.png)\n\n[ref]: ../a.md?q=1#x \"Title\"\n`[code](../a.md)`\n```md\n[x](../a.md)\n```\n[web](https://example.com/a.md)";
        let got = rewrite(
            text,
            "docs/sub/b.md",
            "docs/sub/b.md",
            "docs/a.md",
            "other/a.md",
        );
        assert!(got.contains("[in](../../other/a.md#part)"));
        assert!(got.contains("[ref]: ../../other/a.md?q=1#x \"Title\""));
        assert!(got.contains("`[code](../a.md)`"));
        assert!(got.contains("```md\n[x](../a.md)"));
        assert!(got.contains("https://example.com/a.md"));
        let moved = rewrite(
            "[x](sub/c.md) ![i](a%20b.png) [self](a.md#s)",
            "docs/a.md",
            "other/a.md",
            "docs/a.md",
            "other/a.md",
        );
        assert_eq!(
            moved,
            "[x](../docs/sub/c.md) ![i](../docs/a%20b.png) [self](a.md#s)"
        );
    }
}
