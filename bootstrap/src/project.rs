// Project discovery and settings. A Jazyk project is a directory holding a jazyk.toml,
// found by walking up from the working directory. Mirrors docs/compiler/project-settings.md.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// Project-level [llm] table. Every field is optional; unset fields fall through to the
// global config, then the built-in default (see cli::resolve_llm).
#[derive(Clone, Default)]
pub struct LlmSettings {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub temperature: Option<f64>,
}

#[derive(Clone, Default)]
pub struct Linting {
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

// Turn and build budgets, the [limits] table. Defaults per docs/compiler/project-settings.md.
#[derive(Clone)]
pub struct Limits {
    pub turn_rounds: u32,
    pub turn_mutations: usize,
    pub context_budget: usize,
    pub build_turn_factor: u32,
    pub max_section_chars: usize,
    pub max_doc_sections: usize,
    pub max_entity_requirements: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            turn_rounds: 24,
            turn_mutations: 64,
            context_budget: 24_000,
            build_turn_factor: 3,
            max_section_chars: 6_000,
            max_doc_sections: 40,
            max_entity_requirements: 50,
        }
    }
}

// [workflow] defaults for the control plane: auto acts on change, manual gates work
// behind a release. Live values sit in control.yaml in the out directory.
// Mirrors docs/compiler/project-settings.md#workflow.
#[derive(Clone)]
pub struct Workflow {
    pub compile: String,  // auto | manual
    pub generate: String, // auto | manual
    pub worker: String,   // internal | agent | any
}

impl Default for Workflow {
    fn default() -> Self {
        // Manual by default: a change queues, nothing spends LLM budget unprompted.
        // Explicit commands are their own approval.
        Workflow { compile: "manual".into(), generate: "manual".into(), worker: "any".into() }
    }
}

#[derive(Clone)]
pub struct Project {
    pub root: PathBuf,
    // Resolved output directory. Never doc input, even when moved with --out.
    pub out: PathBuf,
    pub docs_glob: Vec<String>,
    pub roots: Vec<String>,
    // [gen] settings: where the deliverable lives. Never what it is; the medium is a
    // fact the documents state.
    pub gen_deliverable: Option<String>,
    pub gen_worker: Option<String>,
    // [gen] code: globs scoping which deliverable files count as implementation for
    // the unclaimed report and decompilation. Empty means every file under the
    // deliverable minus the standard exclusions.
    // Mirrors docs/compiler/project-settings.md#generation.
    pub gen_code: Vec<String>,
    pub workflow: Workflow,
    pub llm: LlmSettings,
    pub linting: Linting,
    pub limits: Limits,
    // When set (ad-hoc `jazyk compile <paths>` with no jazyk.toml), these files are used
    // directly instead of resolving the docs glob.
    pub explicit_files: Option<Vec<PathBuf>>,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            root: PathBuf::from("."),
            out: PathBuf::from("./jazyk-out"),
            gen_deliverable: None,
            gen_worker: None,
            gen_code: Vec::new(),
            workflow: Workflow::default(),
            docs_glob: vec!["docs/**/*.md".to_string()],
            roots: vec![],
            llm: LlmSettings::default(),
            linting: Linting::default(),
            limits: Limits::default(),
            explicit_files: None,
        }
    }
}

// Machine-level LLM config, kept out of project settings. Loaded from ~/.jazyk/config.toml
// (or ~/.jazyk.toml). Every field is optional; unset fields fall through to lower-priority sources.
#[derive(Clone, Default)]
pub struct GlobalLlm {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub temperature: Option<f64>,
}

// Read the global LLM config if present.
pub fn load_global_llm() -> GlobalLlm {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return GlobalLlm::default(),
    };
    let candidates = [
        PathBuf::from(&home).join(".jazyk").join("config.toml"),
        PathBuf::from(&home).join(".jazyk.toml"),
    ];
    for c in candidates {
        if let Ok(text) = std::fs::read_to_string(&c) {
            let t = Toml::parse(&text);
            return GlobalLlm {
                base_url: t.string("llm.base_url"),
                model: t.string("llm.model"),
                api_key: t.string("llm.api_key"),
                api_key_env: t.string("llm.api_key_env"),
                temperature: t.string("llm.temperature").and_then(|s| s.parse::<f64>().ok()),
            };
        }
    }
    GlobalLlm::default()
}

// Walk up from `start` to the nearest directory containing jazyk.toml.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_dirs_are_never_doc_input() {
        let dir = std::env::temp_dir().join(format!("jazyk-outdir-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        for d in ["docs", "jazyk-out/docsgen", "jazyk-out-backup-local/docsgen", "elsewhere"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        std::fs::write(dir.join("jazyk.toml"), "[docs]\nglob = [\"**/*.md\"]\n").unwrap();
        std::fs::write(dir.join("docs/a.md"), "# A\n").unwrap();
        std::fs::write(dir.join("jazyk-out/docsgen/x.md"), "# X\n").unwrap();
        std::fs::write(dir.join("jazyk-out-backup-local/docsgen/y.md"), "# Y\n").unwrap();
        std::fs::write(dir.join("elsewhere/z.md"), "# Z\n").unwrap();
        let mut p = Project::load(&dir);
        assert_eq!(p.doc_files(), vec![dir.join("docs/a.md"), dir.join("elsewhere/z.md")]);
        // A relocated out directory (--out) is skipped by path, whatever its name.
        p.out = dir.join("elsewhere");
        assert_eq!(p.doc_files(), vec![dir.join("docs/a.md")]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn default_deliverable_is_root_and_docs_glob_whitelists() {
        let dir = std::env::temp_dir().join(format!("jazyk-deliverable-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        // No [gen] and no [docs] glob: deliverable defaults to the root, docs to docs/.
        std::fs::write(dir.join("jazyk.toml"), "").unwrap();
        std::fs::write(dir.join("docs/a.md"), "# A\n").unwrap();
        std::fs::write(dir.join("README.md"), "# generated\n").unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        let p = Project::load(&dir);
        assert_eq!(p.deliverable_rel(), Some(String::new()));
        assert_eq!(crate::gen::GenSettings::resolve(&p).deliverable, dir);
        assert_eq!(p.doc_files(), vec![dir.join("docs/a.md")]);
        // A deliverable outside the root needs no implicit exclusion.
        std::fs::write(dir.join("jazyk.toml"), "[gen]\ndeliverable = \"../elsewhere\"\n").unwrap();
        assert_eq!(Project::load(&dir).deliverable_rel(), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn redirect_delegates_to_nested_project() {
        let dir = std::env::temp_dir().join(format!("jazyk-redirect-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("inner")).unwrap();
        std::fs::write(dir.join("jazyk.toml"), "redirect = \"inner\"\n").unwrap();
        std::fs::write(
            dir.join("inner/jazyk.toml"),
            "[docs]\nglob = [\"**/*.md\"]\n\n[roots]\nfiles = [\"main.md\"]\n",
        )
        .unwrap();
        let p = Project::load(&dir);
        assert_eq!(p.root, dir.join("inner"));
        assert_eq!(p.docs_glob, vec!["**/*.md".to_string()]);
        // A redirect to a directory without jazyk.toml stays at the original root.
        std::fs::write(dir.join("jazyk.toml"), "redirect = \"missing\"\n").unwrap();
        let p2 = Project::load(&dir);
        assert_eq!(p2.root, dir);
    }
}

// Does this jazyk.toml delegate elsewhere? Used by discovery: a redirect found above
// the starting directory is a boundary, not a capture.
fn redirects(toml_path: &Path) -> bool {
    std::fs::read_to_string(toml_path)
        .map(|t| Toml::parse(&t).string("redirect").is_some())
        .unwrap_or(false)
}

pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start.to_path_buf());
    let mut walked_up = false;
    while let Some(d) = dir {
        let toml = d.join("jazyk.toml");
        if toml.exists() {
            // A redirect applies where it stands: followed when discovery starts in
            // its directory, a boundary when reached from a subdirectory. The
            // subdirectory then stands alone as an ad hoc project.
            // Mirrors docs/compiler/project-settings.md#redirect.
            if walked_up && redirects(&toml) {
                return None;
            }
            return Some(d);
        }
        walked_up = true;
        dir = d.parent().map(|p| p.to_path_buf());
    }
    None
}

impl Project {
    // Load from a jazyk.toml at `root`. Missing keys keep their defaults. A file holding
    // a `redirect` delegates to a nested project directory; redirects do not chain.
    // Mirrors docs/compiler/project-settings.md#redirect.
    pub fn load(root: &Path) -> Project {
        Self::load_inner(root, true)
    }

    fn load_inner(root: &Path, follow_redirect: bool) -> Project {
        let mut p = Project::default();
        p.root = root.to_path_buf();
        p.out = root.join("jazyk-out");
        let toml_path = root.join("jazyk.toml");
        let text = match std::fs::read_to_string(&toml_path) {
            Ok(t) => t,
            Err(_) => return p,
        };
        let t = Toml::parse(&text);
        if follow_redirect {
            if let Some(dir) = t.string("redirect") {
                let target = root.join(dir);
                if target.join("jazyk.toml").exists() {
                    return Self::load_inner(&target, false);
                }
            }
        }
        if let Some(g) = t.array("docs.glob") {
            p.docs_glob = g;
        }
        if let Some(f) = t.array("roots.files") {
            p.roots = f;
        }
        if let Some(v) = t.string("gen.deliverable") {
            p.gen_deliverable = Some(v);
        }
        if let Some(v) = t.string("gen.worker") {
            p.gen_worker = Some(v);
        }
        if let Some(v) = t.array("gen.code") {
            p.gen_code = v;
        }
        if let Some(v) = t.string("workflow.compile") {
            p.workflow.compile = v;
        }
        if let Some(v) = t.string("workflow.generate") {
            p.workflow.generate = v;
        }
        if let Some(v) = t.string("workflow.worker") {
            p.workflow.worker = v;
        }
        p.llm.base_url = t.string("llm.base_url");
        p.llm.model = t.string("llm.model");
        p.llm.api_key = t.string("llm.api_key");
        p.llm.api_key_env = t.string("llm.api_key_env");
        p.llm.temperature = t.string("llm.temperature").and_then(|s| s.parse::<f64>().ok());
        if let Some(v) = t.array("docs.linting.rules.warnings") {
            p.linting.warnings = v;
        }
        if let Some(v) = t.array("docs.linting.rules.errors") {
            p.linting.errors = v;
        }
        if let Some(v) = t.integer("limits.turn_rounds") {
            p.limits.turn_rounds = v as u32;
        }
        if let Some(v) = t.integer("limits.turn_mutations") {
            p.limits.turn_mutations = v as usize;
        }
        if let Some(v) = t.integer("limits.context_budget") {
            p.limits.context_budget = v as usize;
        }
        if let Some(v) = t.integer("limits.build_turn_factor") {
            p.limits.build_turn_factor = v as u32;
        }
        if let Some(v) = t.integer("limits.max_section_chars") {
            p.limits.max_section_chars = v as usize;
        }
        if let Some(v) = t.integer("limits.max_doc_sections") {
            p.limits.max_doc_sections = v as usize;
        }
        if let Some(v) = t.integer("limits.max_entity_requirements") {
            p.limits.max_entity_requirements = v as usize;
        }
        p
    }

    // The deliverable directory as a root-relative glob prefix, when it lies inside
    // the project. Empty string means the root itself (the default `.`); None means it
    // is outside the root, so no doc input can land under it.
    fn deliverable_rel(&self) -> Option<String> {
        let d = self.gen_deliverable.as_deref().unwrap_or(".");
        let mut parts: Vec<String> = Vec::new();
        for c in Path::new(d).components() {
            match c {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    parts.pop()?;
                }
                std::path::Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
                _ => return None,
            }
        }
        Some(parts.join("/"))
    }

    // Resolve the documentation files: walk the tree under root, keep files whose
    // last-matching glob pattern is an inclusion. The deliverable directory is an
    // implicit exclusion evaluated before the configured patterns, so a later
    // inclusion whitelists paths back in (with the defaults, deliverable `.` excludes
    // everything and `docs/**/*.md` re-includes the docs tree).
    pub fn doc_files(&self) -> Vec<PathBuf> {
        if let Some(files) = &self.explicit_files {
            return files.clone();
        }
        let mut patterns: Vec<String> = Vec::new();
        if let Some(rel) = self.deliverable_rel() {
            patterns.push(if rel.is_empty() { "!**".to_string() } else { format!("!{}/**", rel) });
        }
        patterns.extend(self.docs_glob.iter().cloned());
        let mut all = Vec::new();
        collect_files(&self.root, &self.out, &mut all);
        let mut out: Vec<PathBuf> = Vec::new();
        for f in all {
            let rel = match f.strip_prefix(&self.root) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            let mut included = false;
            for pat in &patterns {
                let (neg, p) = match pat.strip_prefix('!') {
                    Some(rest) => (true, rest),
                    None => (false, pat.as_str()),
                };
                if glob_match(p, &rel) {
                    included = !neg;
                }
            }
            if included {
                out.push(f);
            }
        }
        out.sort();
        out.dedup();
        out
    }

    // Whether a file (relative path from root) is in a roots glob.
    pub fn is_root_file(&self, rel: &str) -> bool {
        let mut matched = false;
        for pat in &self.roots {
            let (neg, p) = match pat.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, pat.as_str()),
            };
            if glob_match(p, rel) {
                matched = !neg;
            }
        }
        matched
    }
}

fn collect_files(dir: &Path, out_dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            // Generated output is never doc input: the resolved out directory (wherever
            // --out put it) and anything named like it (e.g. a jazyk-out backup copy).
            if name.starts_with('.')
                || name == "target"
                || name == "node_modules"
                || name.starts_with("jazyk-out")
                || p == out_dir
            {
                continue;
            }
            if p.is_dir() {
                collect_files(&p, out_dir, out);
            } else {
                out.push(p);
            }
        }
    }
}

// Glob matcher supporting `**` (any number of path segments), `*` (within a segment),
// and `?` (one non-slash char). Patterns and paths use `/` separators.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let txt: Vec<&str> = path.split('/').collect();
    seg_match(&pat, &txt)
}

fn seg_match(pat: &[&str], txt: &[&str]) -> bool {
    if pat.is_empty() {
        return txt.is_empty();
    }
    if pat[0] == "**" {
        // ** matches zero or more segments.
        for i in 0..=txt.len() {
            if seg_match(&pat[1..], &txt[i..]) {
                return true;
            }
        }
        return false;
    }
    if txt.is_empty() {
        return false;
    }
    if star_match(pat[0], txt[0]) {
        return seg_match(&pat[1..], &txt[1..]);
    }
    false
}

// Match a single path segment against a pattern segment with `*` and `?`.
fn star_match(pat: &str, txt: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = txt.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

// Minimal TOML reader for the subset jazyk.toml uses: dotted section headers,
// `key = "string"`, `key = 42`, and `key = [ "a", "b" ]` (possibly spanning multiple lines).
struct Toml {
    strings: BTreeMap<String, String>,
    arrays: BTreeMap<String, Vec<String>>,
}

// Split a TOML array body on commas outside quotes, so a quoted item may contain
// commas (lint rules are plain English sentences).
fn split_array_items(inner: &str) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut quote = '"';
    for c in inner.chars() {
        match c {
            '"' | '\'' if !in_str => {
                in_str = true;
                quote = c;
            }
            c if in_str && c == quote => in_str = false,
            ',' if !in_str => {
                items.push(cur.trim().to_string());
                cur.clear();
            }
            c => cur.push(c),
        }
    }
    items.push(cur.trim().to_string());
    items.into_iter().filter(|s| !s.is_empty()).collect()
}

impl Toml {
    fn string(&self, key: &str) -> Option<String> {
        self.strings.get(key).cloned()
    }
    fn integer(&self, key: &str) -> Option<i64> {
        self.strings.get(key).and_then(|s| s.parse::<i64>().ok())
    }
    fn array(&self, key: &str) -> Option<Vec<String>> {
        self.arrays.get(key).cloned()
    }

    fn parse(text: &str) -> Toml {
        let mut strings = BTreeMap::new();
        let mut arrays = BTreeMap::new();
        let mut prefix = String::new();
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let raw = lines[i];
            let line = strip_comment(raw).trim().to_string();
            i += 1;
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                prefix = line[1..line.len() - 1].trim().to_string();
                continue;
            }
            let (key, val) = match line.split_once('=') {
                Some((k, v)) => (k.trim().to_string(), v.trim().to_string()),
                None => continue,
            };
            let full = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", prefix, key)
            };
            if val.starts_with('[') {
                // Gather until the closing ']'.
                let mut buf = val.clone();
                while !buf.contains(']') && i < lines.len() {
                    buf.push(' ');
                    buf.push_str(strip_comment(lines[i]).trim());
                    i += 1;
                }
                let inner = buf.trim_start_matches('[');
                let inner = inner.rsplit_once(']').map(|(a, _)| a).unwrap_or(inner);
                arrays.insert(full, split_array_items(inner));
            } else {
                let v = val.trim_matches('"').trim_matches('\'').to_string();
                strings.insert(full, v);
            }
        }
        Toml { strings, arrays }
    }
}

fn strip_comment(line: &str) -> String {
    // Drop a `#` comment that is not inside a string literal.
    let mut in_str = false;
    let mut out = String::new();
    for c in line.chars() {
        if c == '"' {
            in_str = !in_str;
        }
        if c == '#' && !in_str {
            break;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod discovery_tests {
    use super::*;

    #[test]
    fn redirect_above_is_a_boundary_not_a_capture() {
        let dir = std::env::temp_dir().join(format!("jazyk-redirect-boundary-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("inner")).unwrap();
        std::fs::create_dir_all(dir.join("inner/deep")).unwrap();
        std::fs::create_dir_all(dir.join("side")).unwrap();
        std::fs::write(dir.join("jazyk.toml"), "redirect = \"inner\"\n").unwrap();
        std::fs::write(dir.join("inner/jazyk.toml"), "[docs]\nglob = [\"**/*.md\"]\n").unwrap();

        // Starting at the redirecting directory follows it (via Project::load).
        assert_eq!(find_root(&dir), Some(dir.clone()));
        // Starting inside the target finds the target's own file.
        assert_eq!(find_root(&dir.join("inner/deep")), Some(dir.join("inner")));
        // A sibling subdirectory is its own place: the redirect above is a boundary.
        assert_eq!(find_root(&dir.join("side")), None);

        std::fs::remove_dir_all(&dir).ok();
    }
}

// ---- settings io for the GUI ----
// Read jazyk.toml into a form-friendly shape, and write it back in canonical form.
// Mirrors docs/frontends/gui.md#api (settings) and docs/compiler/project-settings.md.

const KNOWN_STRINGS: &[&str] = &[
    "redirect",
    "gen.deliverable",
    "llm.base_url",
    "llm.model",
    "llm.api_key",
    "llm.api_key_env",
    "llm.temperature",
    "limits.turn_rounds",
    "limits.turn_mutations",
    "limits.context_budget",
    "limits.build_turn_factor",
    "limits.max_section_chars",
    "limits.max_doc_sections",
    "limits.max_entity_requirements",
];
const KNOWN_ARRAYS: &[&str] =
    &["docs.glob", "roots.files", "docs.linting.rules.warnings", "docs.linting.rules.errors"];

// The parsed file for the settings form: set keys, effective defaults, unknown keys
// (which make the form refuse to write), and the file hash for the conditional write.
// The api key is reported as set or unset, never its value.
pub fn settings_read(root: &Path) -> serde_json::Value {
    use serde_json::json;
    let path = root.join("jazyk.toml");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let t = Toml::parse(&text);
    let unknown: Vec<String> = t
        .strings
        .keys()
        .filter(|k| !KNOWN_STRINGS.contains(&k.as_str()))
        .chain(t.arrays.keys().filter(|k| !KNOWN_ARRAYS.contains(&k.as_str())))
        .cloned()
        .collect();
    let d = Limits::default();
    json!({
        "exists": path.exists(),
        "hash": crate::model::hash_hex(&text),
        "redirect": t.string("redirect"),
        "unknown": unknown,
        "settings": {
            "docsGlob": t.array("docs.glob"),
            "roots": t.array("roots.files"),
            "deliverable": t.string("gen.deliverable"),
            "llm": {
                "baseUrl": t.string("llm.base_url"),
                "model": t.string("llm.model"),
                "apiKeyEnv": t.string("llm.api_key_env"),
                "temperature": t.string("llm.temperature").and_then(|s| s.parse::<f64>().ok()),
                "apiKeySet": t.string("llm.api_key").is_some(),
            },
            "linting": {
                "warnings": t.array("docs.linting.rules.warnings").unwrap_or_default(),
                "errors": t.array("docs.linting.rules.errors").unwrap_or_default(),
            },
            "limits": {
                "turnRounds": t.integer("limits.turn_rounds"),
                "turnMutations": t.integer("limits.turn_mutations"),
                "contextBudget": t.integer("limits.context_budget"),
                "buildTurnFactor": t.integer("limits.build_turn_factor"),
                "maxSectionChars": t.integer("limits.max_section_chars"),
                "maxDocSections": t.integer("limits.max_doc_sections"),
                "maxEntityRequirements": t.integer("limits.max_entity_requirements"),
            },
        },
        "defaults": {
            "docsGlob": Project::default().docs_glob,
            "deliverable": ".",
            "limits": {
                "turnRounds": d.turn_rounds,
                "turnMutations": d.turn_mutations,
                "contextBudget": d.context_budget,
                "buildTurnFactor": d.build_turn_factor,
                "maxSectionChars": d.max_section_chars,
                "maxDocSections": d.max_doc_sections,
                "maxEntityRequirements": d.max_entity_requirements,
            },
        },
    })
}

fn toml_str(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

fn toml_list(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| toml_str(s)).collect();
    format!("[{}]", inner.join(", "))
}

// Regenerate jazyk.toml from form values, in canonical section order. Comments do
// not survive; a redirect or an api_key already in the file is carried over
// untouched. Returns the new file text.
pub fn settings_render(root: &Path, s: &serde_json::Value) -> Result<String, String> {
    let old = Toml::parse(&std::fs::read_to_string(root.join("jazyk.toml")).unwrap_or_default());
    let list = |v: &serde_json::Value| -> Option<Vec<String>> {
        v.as_array().map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
    };
    let mut out = String::new();
    if let Some(r) = old.string("redirect") {
        out.push_str(&format!("redirect = {}\n\n", toml_str(&r)));
    }
    let glob = list(&s["docsGlob"]).unwrap_or_default();
    if !glob.is_empty() {
        out.push_str(&format!("[docs]\nglob = {}\n\n", toml_list(&glob)));
    }
    let warnings = list(&s["linting"]["warnings"]).unwrap_or_default();
    let errors = list(&s["linting"]["errors"]).unwrap_or_default();
    if !warnings.is_empty() || !errors.is_empty() {
        out.push_str("[docs.linting.rules]\n");
        if !warnings.is_empty() {
            out.push_str(&format!("warnings = {}\n", toml_list(&warnings)));
        }
        if !errors.is_empty() {
            out.push_str(&format!("errors = {}\n", toml_list(&errors)));
        }
        out.push('\n');
    }
    let roots = list(&s["roots"]).unwrap_or_default();
    if !roots.is_empty() {
        out.push_str(&format!("[roots]\nfiles = {}\n\n", toml_list(&roots)));
    }
    if let Some(d) = s["deliverable"].as_str().map(str::trim).filter(|d| !d.is_empty()) {
        out.push_str(&format!("[gen]\ndeliverable = {}\n\n", toml_str(d)));
    }
    let llm = &s["llm"];
    let mut llm_lines: Vec<String> = Vec::new();
    for (key, field) in [("base_url", "baseUrl"), ("model", "model"), ("api_key_env", "apiKeyEnv")] {
        if let Some(v) = llm[field].as_str().map(str::trim).filter(|v| !v.is_empty()) {
            llm_lines.push(format!("{} = {}", key, toml_str(v)));
        }
    }
    if let Some(k) = old.string("llm.api_key") {
        llm_lines.push(format!("api_key = {}", toml_str(&k)));
    }
    if !llm["temperature"].is_null() {
        let t = llm["temperature"].as_f64().ok_or("temperature must be a number")?;
        llm_lines.push(format!("temperature = {}", t));
    }
    if !llm_lines.is_empty() {
        out.push_str(&format!("[llm]\n{}\n\n", llm_lines.join("\n")));
    }
    let limits = &s["limits"];
    let mut limit_lines: Vec<String> = Vec::new();
    for (key, field) in [
        ("turn_rounds", "turnRounds"),
        ("turn_mutations", "turnMutations"),
        ("context_budget", "contextBudget"),
        ("build_turn_factor", "buildTurnFactor"),
        ("max_section_chars", "maxSectionChars"),
        ("max_doc_sections", "maxDocSections"),
        ("max_entity_requirements", "maxEntityRequirements"),
    ] {
        if !limits[field].is_null() {
            let v = limits[field].as_u64().filter(|v| *v > 0).ok_or_else(|| {
                format!("limits.{} must be a positive integer", key)
            })?;
            limit_lines.push(format!("{} = {}", key, v));
        }
    }
    if !limit_lines.is_empty() {
        out.push_str(&format!("[limits]\n{}\n\n", limit_lines.join("\n")));
    }
    Ok(out.trim_end().to_string() + "\n")
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn settings_roundtrip_preserves_secret_and_redirect() {
        let dir = std::env::temp_dir().join(format!("jazyk-settings-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("jazyk.toml"),
            "[docs]\nglob = [\"**/*.md\"]\n\n[llm]\nmodel = \"m1\"\napi_key = \"sekret\"\n",
        )
        .unwrap();
        let read = settings_read(&dir);
        assert_eq!(read["settings"]["llm"]["apiKeySet"], true);
        assert!(read["unknown"].as_array().unwrap().is_empty());
        // The form never sees the key; the writer carries it over.
        let text = settings_render(
            &dir,
            &serde_json::json!({
                "docsGlob": ["**/*.md", "!drafts/**"],
                "llm": { "model": "m2" },
                "limits": { "turnRounds": 30 },
            }),
        )
        .unwrap();
        assert!(text.contains("glob = [\"**/*.md\", \"!drafts/**\"]"));
        assert!(text.contains("api_key = \"sekret\""));
        assert!(text.contains("model = \"m2\""));
        assert!(text.contains("turn_rounds = 30"));
        std::fs::write(dir.join("jazyk.toml"), &text).unwrap();
        let p = Project::load(&dir);
        assert_eq!(p.llm.model.as_deref(), Some("m2"));
        assert_eq!(p.limits.turn_rounds, 30);
        // Unknown keys are surfaced so the form refuses instead of dropping them.
        std::fs::write(dir.join("jazyk.toml"), "[custom]\nthing = \"x\"\n").unwrap();
        let read2 = settings_read(&dir);
        assert_eq!(read2["unknown"].as_array().unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod toml_array_tests {
    use super::*;

    #[test]
    fn quoted_array_items_keep_their_commas() {
        let t = Toml::parse(
            "[docs.linting.rules]\nwarnings = [\"An em dash appears in prose. Use commas, periods, or colons instead.\", \"Second rule\"]\n",
        );
        let rules = t.array("docs.linting.rules.warnings").unwrap();
        assert_eq!(rules.len(), 2);
        assert!(rules[0].contains("commas, periods, or colons"));
        assert_eq!(rules[1], "Second rule");
    }
}
