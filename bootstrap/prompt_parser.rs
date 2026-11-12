//! Shared source preparation for embedded Markdown prompts and skills.
use pulldown_cmark::{Event, Parser};
use regex::Regex;
use std::path::{Path, PathBuf};

pub fn markdown(source: &str) -> Result<String, String> {
    let body = if let Some(rest) = source.strip_prefix("---\n") {
        let (yaml, body) = rest
            .split_once("\n---\n")
            .ok_or("unclosed prompt frontmatter")?;
        let metadata: serde_norway::Value =
            serde_norway::from_str(yaml).map_err(|e| e.to_string())?;
        if !metadata.is_mapping() {
            return Err("prompt frontmatter must be a mapping".into());
        }
        body
    } else {
        source
    };
    Ok(strip_comments(body))
}

fn strip_comments(body: &str) -> String {
    let mut out = String::new();
    let mut cursor = 0;
    let comments = Regex::new(r"(?s)<!--.*?-->").unwrap();
    let html: Vec<_> = Parser::new(body)
        .into_offset_iter()
        .filter_map(|(event, range)| {
            matches!(event, Event::Html(_) | Event::InlineHtml(_)).then_some(range)
        })
        .collect();
    for comment in comments.find_iter(body) {
        if html.iter().any(|range| range.contains(&comment.start())) {
            out.push_str(&body[cursor..comment.start()]);
            cursor = comment.end();
        }
    }
    out.push_str(&body[cursor..]);
    out
}

pub fn load(root: &Path, path: &Path) -> Result<String, String> {
    fn visit(root: &Path, path: &Path, stack: &mut Vec<PathBuf>) -> Result<String, String> {
        let path = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !path.starts_with(root) {
            return Err(format!("prompt include escapes root: {}", path.display()));
        }
        if stack.contains(&path) {
            return Err(format!(
                "prompt include cycle: {:?} -> {}",
                stack,
                path.display()
            ));
        }
        stack.push(path.clone());
        let source = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        // Fragment markers are framing syntax, not explanatory comments. Prepare
        // each body independently, then retain markers only for Fragments::get.
        let markers = Regex::new(r"(?m)^<!-- ([a-z][a-z0-9-]*) -->\n").unwrap();
        let fragment = matches!(
            path.file_name()
                .and_then(|p| p.to_str())
                .map(|p| p.strip_suffix(".mustache").unwrap_or(p)),
            Some(
                "gates.md"
                    | "session-blocks.md"
                    | "agent-context.md"
                    | "serving-instructions.md"
                    | "generation-steps.md"
                    | "judge-input.md"
                    | "decompile-worker.md"
                    | "answer-handling.md"
            )
        );
        let prepared = if fragment {
            let mut result = String::new();
            let mut cursor = 0;
            for marker in markers.find_iter(&source) {
                result.push_str(&strip_comments(&source[cursor..marker.start()]));
                result.push_str(marker.as_str());
                cursor = marker.end();
            }
            result.push_str(&strip_comments(&source[cursor..]));
            result
        } else {
            markdown(&source)?
        };
        let template = path.to_string_lossy().ends_with(".md.mustache");
        let result = if template {
            // Resolve the deliberately restricted, file-relative partial syntax
            // before Mustache compiles the portable, filesystem-free template.
            let comments = Regex::new(r"(?s)\{\{!.*?\}\}").unwrap();
            let prepared = comments.replace_all(&prepared, "");
            let includes = Regex::new(r"\{\{>\s*([^{}\s]+)\s*\}\}").unwrap();
            let mut result = String::new();
            let mut cursor = 0;
            for c in includes.captures_iter(&prepared) {
                let span = c.get(0).unwrap();
                result.push_str(&prepared[cursor..span.start()]);
                result.push_str(&visit(root, &path.parent().unwrap().join(&c[1]), stack)?);
                cursor = span.end();
            }
            result.push_str(&prepared[cursor..]);
            mustache::compile_str(&result).map_err(|e| format!("{}: {e}", path.display()))?;
            result
        } else {
            prepared
        };
        stack.pop();
        Ok(result)
    }
    visit(
        &root.canonicalize().map_err(|e| e.to_string())?,
        path,
        &mut Vec::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_and_comments_are_source_only_and_code_is_literal() {
        let source = "---\npurpose: example\n---\n# Role\n\n<!-- rationale\ncontinues -->\nText <!-- hidden -->visible.\n\n`<!-- literal -->`\n\n```html\n<!-- example -->\n```\n";
        let prepared = markdown(source).unwrap();
        assert!(!prepared.contains("purpose:"));
        assert!(!prepared.contains("rationale"));
        assert!(prepared.contains("Text visible."));
        assert!(prepared.contains("`<!-- literal -->`"));
        assert!(prepared.contains("<!-- example -->"));
        assert!(markdown("---\nbroken: [\n---\nbody").is_err());
        assert!(markdown("---\n- list\n---\nbody").is_err());
        assert!(markdown("---\nmissing close").is_err());
    }

    #[test]
    fn includes_are_nested_checked_and_render_values_once() {
        let dir = std::env::temp_dir().join(format!("jazyk-prompt-parser-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(
            dir.join("main.md.mustache"),
            "{{! private }}{{> nested/part.md.mustache }}",
        )
        .unwrap();
        std::fs::write(
            dir.join("nested/part.md.mustache"),
            "{{> ../rule.md }}\n{{{value}}}",
        )
        .unwrap();
        std::fs::write(
            dir.join("rule.md"),
            "---\npurpose: rule\n---\n# Rule\n<!-- rationale -->",
        )
        .unwrap();
        let prepared = load(&dir, &dir.join("main.md.mustache")).unwrap();
        assert!(prepared.contains("# Rule"));
        assert!(!prepared.contains("private"));
        assert!(!prepared.contains("purpose:"));
        assert!(!prepared.contains("rationale"));
        let rendered = crate::payload::render_mustache(
            &prepared,
            &serde_json::json!({"value":"<tag>& {{value}} <!-- user data -->"}),
        )
        .unwrap();
        assert!(rendered.contains("<tag>& {{value}} <!-- user data -->"));
        for invalid in [
            "{{> missing.md }}",
            "{{> main.md.mustache }}",
            "{{> /etc/hosts }}",
            "{{#broken}}",
        ] {
            std::fs::write(dir.join("main.md.mustache"), invalid).unwrap();
            assert!(
                load(&dir, &dir.join("main.md.mustache")).is_err(),
                "{invalid}"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
