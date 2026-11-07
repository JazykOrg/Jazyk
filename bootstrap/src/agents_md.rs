// The jazyk section of a project's AGENTS.md, and the marked `@AGENTS.md` import in
// an existing CLAUDE.md or GEMINI.md. Everything outside the markers is the
// project's own and stays byte for byte.
// Mirrors docs/frontends/agent-context.md#the-jazyk-section-of-agentsmd.
use crate::vfs::VPathExt;
use std::path::{Path, PathBuf};

pub const BEGIN: &str = "<!-- JAZYK:BEGIN -->";
pub const END: &str = "<!-- JAZYK:END -->";
const PAYLOAD: &str = include_str!("../../docs/compiler/goals/prompts/project-agents.md");
pub const AGENTS: &str = "AGENTS.md";
// The files that get the import, when they exist.
const IMPORTERS: &[&str] = &["CLAUDE.md", ".claude/CLAUDE.md", "GEMINI.md"];

// Where the markers are in a text.
#[derive(Debug, PartialEq, Eq)]
enum Markers {
    None,
    // Byte range covering the BEGIN line through the END line, without the final
    // line ending.
    One(usize, usize),
    Broken,
}

fn marker_lines(text: &str) -> Markers {
    let mut begins = Vec::new();
    let mut ends = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        match body.trim() {
            BEGIN => begins.push((at, at + body.len())),
            END => ends.push((at, at + body.len())),
            _ => {}
        }
        at += line.len();
    }
    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => Markers::None,
        ([(b, _)], [(_, e)]) if b < e => Markers::One(*b, *e),
        _ => Markers::Broken,
    }
}

// The text with any jazyk section removed, for readers that treat the rest as
// the project's own instructions. A broken section is left in.
pub fn strip(text: &str) -> String {
    match marker_lines(text) {
        Markers::One(b, e) => {
            let mut rest = text[e..].to_string();
            if rest.starts_with("\r\n") {
                rest.drain(..2);
            } else if rest.starts_with('\n') {
                rest.drain(..1);
            }
            format!("{}{}", &text[..b], rest)
        }
        _ => text.to_string(),
    }
}

fn eol(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn block(inner: &str, eol: &str) -> String {
    let body: Vec<&str> = inner.trim_end().lines().collect();
    format!("{BEGIN}{eol}{}{eol}{END}", body.join(eol))
}

// One line per set, from the project's configuration.
fn sets_text(workspace: &crate::document_sets::Workspace) -> String {
    if workspace.sets.is_empty() {
        return "No document sets yet. Add one in the GUI or with `update_project_settings`.".into();
    }
    let list = |patterns: &[String]| {
        let shown: Vec<String> = patterns
            .iter()
            .filter(|p| !p.starts_with('!'))
            .map(|p| format!("`{p}`"))
            .collect();
        let excluded = patterns.iter().filter(|p| p.starts_with('!')).count();
        let mut s = if shown.is_empty() {
            "none".to_string()
        } else {
            shown.join(", ")
        };
        if excluded > 0 {
            s.push_str(&format!(
                " ({excluded} exclusion{})",
                if excluded == 1 { "" } else { "s" }
            ));
        }
        s
    };
    let mut lines = vec!["Document sets:".to_string(), String::new()];
    for set in &workspace.sets {
        let mut line = format!("- `{}` ({}): docs {}", set.id, set.name.trim(), list(&set.docs));
        if !set.roots.is_empty() {
            line.push_str(&format!(", roots {}", list(&set.roots)));
        }
        line.push_str(&format!(
            ", source {}",
            if set.source.is_empty() {
                "none".to_string()
            } else {
                list(&set.source)
            }
        ));
        lines.push(line);
    }
    lines.join("\n")
}

// The section's text for this project, without the markers.
pub fn render(root: &Path) -> Result<String, String> {
    let workspace = crate::document_sets::Workspace::load(root).map_err(|e| e.1)?;
    Ok(PAYLOAD.replace("{SETS}", &sets_text(&workspace)))
}

// `[agents] block` in jazyk.toml; true when unset or unreadable.
pub fn enabled(root: &Path) -> bool {
    crate::document_sets::Workspace::load(root)
        .ok()
        .and_then(|w| w.settings.get("agents").and_then(|a| a.get("block")).and_then(|b| b.as_bool()))
        .unwrap_or(true)
}

// The path an importer uses to reach AGENTS.md at the root.
fn import_path(importer: &str) -> String {
    let depth = importer.matches('/').count();
    format!("@{}{AGENTS}", "../".repeat(depth))
}

// Whether a text imports `target` (an absolute path) outside code, relative to `dir`.
fn imports(text: &str, dir: &Path, target: &Path) -> bool {
    let target = normalize(target);
    let mut fence: Option<(char, usize)> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some((c, n)) = fence {
            if trimmed.starts_with(&c.to_string().repeat(n)) {
                fence = None;
            }
            continue;
        }
        for c in ['`', '~'] {
            let n = trimmed.chars().take_while(|x| *x == c).count();
            if n >= 3 {
                fence = Some((c, n));
            }
        }
        if fence.is_some() {
            continue;
        }
        for token in import_tokens(line) {
            if normalize(&dir.join(token)) == target {
                return true;
            }
        }
    }
    false
}

// `@path` tokens on one line: at the line start or after whitespace, outside code
// spans. Mirrors docs/frontends/agent-context.md#instructions.
pub fn import_tokens(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_code = false;
    let mut prev_space = true;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'`' {
            in_code = !in_code;
            prev_space = false;
            i += 1;
            continue;
        }
        if !in_code && b == b'@' && prev_space {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && !bytes[end].is_ascii_whitespace() && bytes[end] != b'`' {
                end += 1;
            }
            let token = line[start..end].trim_end_matches(['.', ',', ';', ':', ')', '!', '?']);
            if !token.is_empty() {
                out.push(token);
            }
            i = end;
            prev_space = false;
            continue;
        }
        prev_space = b.is_ascii_whitespace();
        i += 1;
    }
    out
}

// Lexical normalization: `.` and `..` resolved without touching the filesystem.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// The same file as AGENTS.md: a symbolic link to it, or a hard link.
fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let (Ok(x), Ok(y)) = (std::fs::metadata(a), std::fs::metadata(b)) {
                return x.dev() == y.dev() && x.ino() == y.ino();
            }
        }
    }
    match (a.vcanonicalize(), b.vcanonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

// Atomic, and through a symbolic link: an AGENTS.md linked to CLAUDE.md stays a link.
fn write(path: &Path, text: &str) -> Result<(), String> {
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut target = path.to_path_buf();
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() && std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        if let Ok(real) = std::fs::canonicalize(path) {
            target = real;
        }
    }
    crate::store::write_atomic(&target, text.as_bytes()).map_err(|e| format!("cannot write {}: {}", path.display(), e))
}

// Put `inner` between the markers of `text`, or append a new section. None when
// nothing changes or the file must stay as it is.
fn apply(text: Option<&str>, inner: &str) -> Result<Option<String>, String> {
    let Some(text) = text else {
        return Ok(Some(format!("{}\n", block(inner, "\n"))));
    };
    let eol = eol(text);
    let wanted = block(inner, eol);
    match marker_lines(text) {
        Markers::Broken => Err("its jazyk markers are duplicated, out of order, or unclosed; fix or delete them".into()),
        Markers::One(b, e) => Ok((text[b..e] != wanted).then(|| format!("{}{}{}", &text[..b], wanted, &text[e..]))),
        Markers::None => {
            let mut out = text.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push_str(eol);
            }
            if !out.is_empty() {
                out.push_str(eol);
            }
            out.push_str(&wanted);
            out.push_str(eol);
            Ok(Some(out))
        }
    }
}

// What a sync did: one line per changed file, and warnings.
#[derive(Debug, Default)]
pub struct Outcome {
    pub changed: Vec<String>,
    // What changed, as a noun phrase: "the jazyk section of AGENTS.md".
    pub items: Vec<String>,
    pub warnings: Vec<String>,
}

impl Outcome {
    pub fn report(&self) {
        for line in self.changed.iter().chain(&self.warnings) {
            eprintln!("jazyk: {line}");
        }
    }
}

// Create or refresh the section and the imports. Never fails: problems are warnings.
pub fn sync(root: &Path) -> Outcome {
    let mut outcome = Outcome::default();
    if !root.join(crate::document_sets::CONFIG).vis_file() || !enabled(root) {
        return outcome;
    }
    let inner = match render(root) {
        Ok(inner) => inner,
        Err(e) => {
            outcome.warnings.push(format!("the jazyk section of AGENTS.md was not refreshed: {e}"));
            return outcome;
        }
    };
    let agents = root.join(AGENTS);
    let existing = crate::vfs::read_to_string(&agents).ok();
    let created = existing.is_none();
    match apply(existing.as_deref(), &inner) {
        Ok(Some(text)) => match write(&agents, &text) {
            Ok(()) => outcome.items.push("the jazyk section of AGENTS.md".into()),
            Err(e) => outcome.warnings.push(e),
        },
        Ok(None) => {}
        Err(e) => outcome.warnings.push(format!("AGENTS.md left unchanged: {e}")),
    }
    if let Some(item) = outcome.items.first().cloned() {
        let _ = item;
        outcome.changed.push(if created {
                "created AGENTS.md with the jazyk section".into()
            } else if marker_lines(existing.as_deref().unwrap_or_default()) == Markers::None {
                "added the jazyk section to AGENTS.md".into()
            } else {
                "updated the jazyk section in AGENTS.md".into()
            });
    }
    if !agents.vis_file() {
        return outcome;
    }
    for rel in IMPORTERS {
        let path = root.join(rel);
        if !path.vis_file() || same_file(&path, &agents) {
            continue;
        }
        let Ok(text) = crate::vfs::read_to_string(&path) else { continue };
        let has_markers = marker_lines(&text) != Markers::None;
        let outside = strip(&text);
        let dir = path.parent().unwrap_or(root).to_path_buf();
        if !has_markers && imports(&outside, &dir, &agents) {
            continue;
        }
        if has_markers && imports(&outside, &dir, &agents) {
            // The file imports AGENTS.md on its own: the marked import is redundant
            // but harmless, and removing it is the person's call.
            continue;
        }
        match apply(Some(&text), &import_path(rel)) {
            Ok(Some(new)) => match write(&path, &new) {
                Ok(()) => {
                    outcome.items.push(format!("an AGENTS.md import in {rel}"));
                    outcome.changed.push(if has_markers {
                    format!("updated the AGENTS.md import in {rel}")
                } else {
                    format!("added an AGENTS.md import to {rel}")
                    })
                }
                Err(e) => outcome.warnings.push(e),
            },
            Ok(None) => {}
            Err(e) => outcome.warnings.push(format!("{rel} left unchanged: {e}")),
        }
    }
    outcome
}

// Working commands ensure the section and imports exist at every start.
// Mirrors docs/frontends/agent-context.md#when-it-is-written.
pub fn refresh(start: &Path) {
    let Some(root) = crate::project::find_root(start) else { return };
    sync(&root).report();
}

// The section's state for `jazyk setup --list`.
pub fn state(root: &Path) -> &'static str {
    if !enabled(root) {
        return "off";
    }
    let Ok(text) = crate::vfs::read_to_string(root.join(AGENTS)) else { return "missing" };
    let Ok(inner) = render(root) else { return "stale" };
    match marker_lines(&text) {
        Markers::One(b, e) if text[b..e] == block(&inner, eol(&text)) => "installed",
        Markers::One(..) | Markers::Broken => "stale",
        Markers::None => "missing",
    }
}

// Delete the section and the imports. A file left empty is deleted.
pub fn remove(root: &Path) -> Result<bool, String> {
    let mut removed = false;
    for rel in std::iter::once(AGENTS).chain(IMPORTERS.iter().copied()) {
        let path = root.join(rel);
        if rel != AGENTS && same_file(&path, &root.join(AGENTS)) {
            continue;
        }
        let Ok(text) = crate::vfs::read_to_string(&path) else { continue };
        if !matches!(marker_lines(&text), Markers::One(..)) {
            continue;
        }
        let rest = strip(&text);
        if rest.trim().is_empty() {
            crate::vfs::remove_file(&path).map_err(|e| format!("cannot delete {}: {}", path.display(), e))?;
        } else {
            let trimmed = rest.trim_end().to_string() + eol(&text);
            write(&path, &trimmed)?;
        }
        removed = true;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".jazyk")).unwrap();
        std::fs::write(
            dir.path().join(".jazyk/jazyk.toml"),
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\", \"!docs/drafts/**\"]\nroots = [\"docs/main.md\"]\nsource = [\"src/**\"]\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn sync_creates_a_missing_file_and_is_idempotent() {
        let dir = project();
        let root = dir.path();
        let out = sync(root);
        assert_eq!(out.changed, vec!["created AGENTS.md with the jazyk section"]);
        let text = std::fs::read_to_string(root.join(AGENTS)).unwrap();
        assert!(text.starts_with(BEGIN));
        assert!(text.trim_end().ends_with(END));
        assert!(text.contains("- `ds_main` (Main): docs `docs/**/*.md` (1 exclusion), roots `docs/main.md`, source `src/**`"));
        assert!(!text.contains("{SETS}"));
        assert_eq!(state(root), "installed");
        // Unchanged: no write.
        assert!(sync(root).changed.is_empty());
    }

    #[test]
    fn the_projects_own_text_stays_byte_for_byte_including_crlf() {
        let dir = project();
        let root = dir.path();
        let mine = "# Rules\r\n\r\nKeep it short.\r\n";
        std::fs::write(root.join(AGENTS), mine).unwrap();
        assert_eq!(sync(root).changed, vec!["added the jazyk section to AGENTS.md"]);
        let text = std::fs::read_to_string(root.join(AGENTS)).unwrap();
        assert!(text.starts_with(mine));
        assert!(!text.replace("\r\n", "").contains('\n'), "line endings follow the file");
        // A changed configuration refreshes only the inside.
        let config = root.join(".jazyk/jazyk.toml");
        let edited = std::fs::read_to_string(&config).unwrap().replace("src/**", "lib/**");
        std::fs::write(&config, edited).unwrap();
        let tail = "\r\nAfter the section.\r\n";
        std::fs::write(root.join(AGENTS), format!("{text}{tail}")).unwrap();
        assert_eq!(sync(root).changed, vec!["updated the jazyk section in AGENTS.md"]);
        let after = std::fs::read_to_string(root.join(AGENTS)).unwrap();
        assert!(after.starts_with(mine));
        assert!(after.ends_with(tail));
        assert!(after.contains("source `lib/**`"));
        assert_eq!(strip(&after), format!("{mine}\r\n{tail}"));
    }

    #[test]
    fn deleting_the_section_restores_it_and_broken_markers_are_refused() {
        let dir = project();
        let root = dir.path();
        sync(root);
        std::fs::write(root.join(AGENTS), "# Ours\n").unwrap();
        assert_eq!(sync(root).changed, vec!["added the jazyk section to AGENTS.md"]);
        assert!(std::fs::read_to_string(root.join(AGENTS)).unwrap().starts_with("# Ours\n"));
        let broken = format!("{BEGIN}\nx\n{BEGIN}\n{END}\n");
        std::fs::write(root.join(AGENTS), &broken).unwrap();
        let out = sync(root);
        assert!(out.changed.is_empty());
        assert_eq!(out.warnings.len(), 1);
        assert_eq!(std::fs::read_to_string(root.join(AGENTS)).unwrap(), broken);
        assert_eq!(state(root), "stale");
    }

    #[test]
    fn the_setting_turns_it_off() {
        let dir = project();
        let root = dir.path();
        let config = root.join(".jazyk/jazyk.toml");
        let mut text = std::fs::read_to_string(&config).unwrap();
        text.push_str("\n[agents]\nblock = false\n");
        std::fs::write(&config, text).unwrap();
        std::fs::write(root.join("CLAUDE.md"), "# Mine\n").unwrap();
        refresh(root);
        assert!(sync(root).changed.is_empty());
        assert!(!root.join(AGENTS).exists());
        assert_eq!(std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(), "# Mine\n");
        std::fs::write(root.join(AGENTS), format!("{BEGIN}\nKeep me\n{END}\n")).unwrap();
        let before = std::fs::read_to_string(root.join(AGENTS)).unwrap();
        refresh(root);
        assert_eq!(std::fs::read_to_string(root.join(AGENTS)).unwrap(), before);
        assert_eq!(state(root), "off");
    }

    #[test]
    fn claude_md_gets_the_import_once_and_respects_its_own() {
        let dir = project();
        let root = dir.path();
        std::fs::write(root.join("CLAUDE.md"), "# Claude\n").unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join(".claude/CLAUDE.md"), "Nested\n").unwrap();
        std::fs::write(root.join("GEMINI.md"), "See @./AGENTS.md for more.\n").unwrap();
        let out = sync(root);
        assert!(out.changed.contains(&"added an AGENTS.md import to CLAUDE.md".to_string()), "{out:?}");
        assert!(out.changed.contains(&"added an AGENTS.md import to .claude/CLAUDE.md".to_string()));
        assert_eq!(
            std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
            format!("# Claude\n\n{BEGIN}\n@AGENTS.md\n{END}\n")
        );
        assert!(std::fs::read_to_string(root.join(".claude/CLAUDE.md")).unwrap().contains("\n@../AGENTS.md\n"));
        assert_eq!(std::fs::read_to_string(root.join("GEMINI.md")).unwrap(), "See @./AGENTS.md for more.\n");
        assert!(sync(root).changed.is_empty());
        // An import inside code does not count.
        std::fs::write(root.join("GEMINI.md"), "```\n@AGENTS.md\n```\n").unwrap();
        assert_eq!(sync(root).changed, vec!["added an AGENTS.md import to GEMINI.md"]);
        // Removal takes every marked part and deletes files left empty.
        assert!(remove(root).unwrap());
        assert!(!root.join(AGENTS).exists());
        assert_eq!(std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(), "# Claude\n");
        assert_eq!(state(root), "missing");
    }

    #[cfg(unix)]
    #[test]
    fn a_claude_md_linked_to_agents_md_is_left_alone() {
        let dir = project();
        let root = dir.path();
        sync(root);
        std::os::unix::fs::symlink("AGENTS.md", root.join("CLAUDE.md")).unwrap();
        let before = std::fs::read_to_string(root.join(AGENTS)).unwrap();
        assert!(sync(root).changed.is_empty());
        assert_eq!(std::fs::read_to_string(root.join(AGENTS)).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn an_agents_md_linked_to_claude_md_stays_a_link() {
        let dir = project();
        let root = dir.path();
        std::fs::write(root.join("CLAUDE.md"), "# Mine\n").unwrap();
        std::os::unix::fs::symlink("CLAUDE.md", root.join(AGENTS)).unwrap();
        let out = sync(root);
        assert_eq!(out.changed, vec!["added the jazyk section to AGENTS.md"], "{out:?}");
        assert!(std::fs::symlink_metadata(root.join(AGENTS)).unwrap().file_type().is_symlink());
        let text = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
        assert!(text.starts_with("# Mine\n\n") && text.contains(BEGIN) && !text.contains("@AGENTS.md"));
    }

    #[test]
    fn refresh_restores_missing_files_sections_and_imports_despite_an_old_stamp() {
        let dir = project();
        let root = dir.path();
        std::fs::write(root.join(".jazyk/agents-md.stamp"), "old stamp\n").unwrap();
        std::fs::write(root.join("CLAUDE.md"), "# Mine\n").unwrap();
        refresh(&root.join("docs"));
        assert_eq!(state(root), "installed");
        assert!(std::fs::read_to_string(root.join("CLAUDE.md")).unwrap().contains("@AGENTS.md"));
        std::fs::remove_file(root.join(AGENTS)).unwrap();
        refresh(root);
        assert_eq!(state(root), "installed");
        std::fs::write(root.join(AGENTS), "# Rules\n").unwrap();
        std::fs::write(root.join("CLAUDE.md"), "# Mine\n").unwrap();
        refresh(root);
        assert_eq!(state(root), "installed");
        assert!(std::fs::read_to_string(root.join(AGENTS)).unwrap().starts_with("# Rules\n"));
        assert!(std::fs::read_to_string(root.join("CLAUDE.md")).unwrap().starts_with("# Mine\n"));
        assert!(std::fs::read_to_string(root.join("CLAUDE.md")).unwrap().contains("@AGENTS.md"));
        assert!(sync(root).changed.is_empty());
    }

    #[test]
    fn import_tokens_skip_code_spans_and_emails() {
        assert_eq!(import_tokens("@AGENTS.md and @docs/x.md."), vec!["AGENTS.md", "docs/x.md"]);
        assert!(import_tokens("mail matus@matus.io or `@AGENTS.md`").is_empty());
    }
}
