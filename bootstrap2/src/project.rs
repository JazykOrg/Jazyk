// Project discovery and settings. A Jazyk project is a directory holding a jazyk.toml,
// found by walking up from the working directory. Mirrors docs2/compiler/project-settings.md.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Default)]
pub struct LlmSettings {
    pub base_url: String,
    pub model: String,
    pub api_key_env: String,
}

#[derive(Clone, Default)]
pub struct Linting {
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

// Turn and build budgets, the [limits] table. Defaults per docs2/compiler/project-settings.md.
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

#[derive(Clone)]
pub struct Project {
    pub root: PathBuf,
    pub docs_glob: Vec<String>,
    pub roots: Vec<String>,
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
            docs_glob: vec!["docs/**/*.md".to_string()],
            roots: vec![],
            llm: LlmSettings {
                base_url: "http://localhost:11434/v1".to_string(),
                model: "llama3.1".to_string(),
                api_key_env: "JAZYK_API_KEY".to_string(),
            },
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
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start.to_path_buf());
    while let Some(d) = dir {
        if d.join("jazyk.toml").exists() {
            return Some(d);
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
    None
}

impl Project {
    // Load from a jazyk.toml at `root`. Missing keys keep their defaults.
    pub fn load(root: &Path) -> Project {
        let mut p = Project::default();
        p.root = root.to_path_buf();
        let toml_path = root.join("jazyk.toml");
        let text = match std::fs::read_to_string(&toml_path) {
            Ok(t) => t,
            Err(_) => return p,
        };
        let t = Toml::parse(&text);
        if let Some(g) = t.array("docs.glob") {
            p.docs_glob = g;
        }
        if let Some(f) = t.array("roots.files") {
            p.roots = f;
        }
        if let Some(v) = t.string("llm.base_url") {
            p.llm.base_url = v;
        }
        if let Some(v) = t.string("llm.model") {
            p.llm.model = v;
        }
        if let Some(v) = t.string("llm.api_key_env") {
            p.llm.api_key_env = v;
        }
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

    // Resolve the documentation files: walk the tree under root, keep files whose
    // last-matching glob pattern is an inclusion.
    pub fn doc_files(&self) -> Vec<PathBuf> {
        if let Some(files) = &self.explicit_files {
            return files.clone();
        }
        let mut all = Vec::new();
        collect_files(&self.root, &mut all);
        let mut out: Vec<PathBuf> = Vec::new();
        for f in all {
            let rel = match f.strip_prefix(&self.root) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            let mut included = false;
            for pat in &self.docs_glob {
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

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "target" || name == "node_modules" || name == "jazyk-out" {
                continue;
            }
            if p.is_dir() {
                collect_files(&p, out);
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
                let items: Vec<String> = inner
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                arrays.insert(full, items);
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
