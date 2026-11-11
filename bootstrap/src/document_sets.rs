//! Project configuration, document-set identity and shared path membership.
//! See docs/compiler/document-sets.md. No legacy configuration conversion.
use crate::project::Project;
use crate::vfs::VPathExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const CONFIG: &str = ".jazyk/jazyk.toml";
pub const EMPTY_CONFIG: &str = "version = 1\nsets = []\n";
// The generated `.jazyk/.gitignore`: everything ignored, then the configuration and
// each set's committed state kept by name. Mirrors docs/compiler/document-sets.md#persistence-and-identity
// and docs/compiler/jazyk-folder.md; `committed` is the same rule.
pub const IGNORE: &str = "*
!/jazyk.toml
!/.gitignore
!/.gitattributes
!/README.md
!/*/
/local/
/*/*
!/*/store.yaml
!/*/graph/
!/*/graph/**
!/*/sources/
!/*/sources/**
!/*/gen/
!/*/gen/**
.*.tmp
";

// The generated `.jazyk/.gitattributes`: the merge driver for every committed set file.
// Mirrors docs/compiler/jazyk-folder.md#gitattributes.
pub const ATTRIBUTES: &str = "/*/store.yaml merge=jazyk
/*/graph/** merge=jazyk
/*/sources/** merge=jazyk
/*/gen/** merge=jazyk
";

// The generated `.jazyk/README.md`: the layout in brief.
// Mirrors docs/compiler/jazyk-folder.md#readmemd.
pub const README: &str = "# .jazyk

Jazyk keeps this project's compiled state here. Jazyk writes every file; edit the
documents, not these files. See https://jazyk.org for the full reference.

- `jazyk.toml`: the project configuration. Committed.
- `<set>/` (one folder per document set, e.g. `my-set-ks8mf/`): the current graph.
  Committed: `store.yaml`, `graph/` (the nodes and the decisions), `sources/` (each
  document as the graph reflects it), and `gen/` (the ledger).
- `<set>/local/` and `local/`: what this machine's runs leave behind: build state, the
  journal, locks. Never committed.
- `<set>/derived/`: rebuilt from the committed files: diagrams, generated docs. Never
  committed, safe to delete.

Logs (transcripts, chats, feedback) live outside the project; `jazyk status` prints
where. `.gitignore` and `.gitattributes` here are generated: keep them committed.
";

// Ignore files an earlier Jazyk generated: replaced by `IGNORE` on the next write.
const EARLIER_IGNORES: [&str; 3] = ["*\n!jazyk.toml\n!.gitignore\n", IGNORE_V3, IGNORE_V4];

// The file store version 3 generated, before the status split into record files.
const IGNORE_V3: &str = "*
!/jazyk.toml
!/.gitignore
!/ds_*/
/ds_*.bak/
!/ds_*/status.yaml
!/ds_*/docs/
!/ds_*/docs/**
!/ds_*/graph/
!/ds_*/graph/entities/
!/ds_*/graph/entities/*.yaml
!/ds_*/graph/requirements/
!/ds_*/graph/requirements/*.yaml
!/ds_*/graph/criteria/
!/ds_*/graph/criteria/*.yaml
!/ds_*/graph/views/
!/ds_*/graph/views/*.yaml
!/ds_*/graph/diagnostics/
!/ds_*/graph/diagnostics/*.yaml
!/ds_*/graph/redirects/
!/ds_*/graph/redirects/*.yaml
!/ds_*/gen/
!/ds_*/gen/**
.*.tmp
";

// The file store version 4 generated, before the store committed only the current graph.
const IGNORE_V4: &str = "*
!/jazyk.toml
!/.gitignore
!/ds_*/
/ds_*.bak/
!/ds_*/status.yaml
!/ds_*/changes/
!/ds_*/changes/**
!/ds_*/parked/
!/ds_*/parked/*.yaml
!/ds_*/failed/
!/ds_*/failed/*.yaml
!/ds_*/alignment/
!/ds_*/alignment/*.yaml
!/ds_*/lookalikes/
!/ds_*/lookalikes/*.yaml
!/ds_*/docs/
!/ds_*/docs/**
!/ds_*/graph/
!/ds_*/graph/entities/
!/ds_*/graph/entities/*.yaml
!/ds_*/graph/requirements/
!/ds_*/graph/requirements/*.yaml
!/ds_*/graph/criteria/
!/ds_*/graph/criteria/*.yaml
!/ds_*/graph/views/
!/ds_*/graph/views/*.yaml
!/ds_*/graph/diagnostics/
!/ds_*/graph/diagnostics/*.yaml
!/ds_*/graph/redirects/
!/ds_*/graph/redirects/*.yaml
!/ds_*/gen/
!/ds_*/gen/**
.*.tmp
";

// Write one generated file when it is absent or still holds an earlier generated text.
// A file with other content is the owner's and stays. Returns whether it wrote.
fn ensure_generated(
    root: &Path,
    name: &str,
    text: &str,
    earlier: &[&str],
) -> std::io::Result<bool> {
    let path = root.join(".jazyk").join(name);
    match crate::vfs::read_to_string(&path) {
        Ok(current) if current == text || !earlier.contains(&current.as_str()) => Ok(false),
        Ok(_) => crate::vfs::write(&path, text).map(|()| true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            crate::vfs::create_dir_all(root.join(".jazyk"))?;
            crate::vfs::write(&path, text).map(|()| true)
        }
        Err(e) => Err(e),
    }
}

// Write `.jazyk/.gitignore`, `.jazyk/.gitattributes`, and `.jazyk/README.md` when each is
// absent or still an earlier generated one. Returns whether `.gitignore` was written.
pub fn ensure_ignore(root: &Path) -> std::io::Result<bool> {
    let wrote = ensure_generated(root, ".gitignore", IGNORE, &EARLIER_IGNORES)?;
    ensure_generated(root, ".gitattributes", ATTRIBUTES, &[])?;
    ensure_generated(root, "README.md", README, &[])?;
    Ok(wrote)
}

// Register the `jazyk` merge driver in the repository's local git configuration when the
// project sits in a git work tree and the driver is missing. A no-op without git, in a
// scoped file system, or outside a work tree. Mirrors docs/compiler/jazyk-folder.md#gitattributes.
pub fn register_merge_driver(root: &Path) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if crate::vfs::is_scoped() {
            return;
        }
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .ok()
        };
        let Some(inside) = git(&["rev-parse", "--is-inside-work-tree"]) else {
            return;
        };
        if !inside.status.success() || String::from_utf8_lossy(&inside.stdout).trim() != "true" {
            return;
        }
        let present = git(&["config", "--local", "--get", "merge.jazyk.driver"])
            .is_some_and(|o| o.status.success() && !o.stdout.is_empty());
        if present {
            return;
        }
        git(&[
            "config",
            "--local",
            "merge.jazyk.name",
            "Jazyk set files, merged field by field",
        ]);
        git(&["config", "--local", "merge.jazyk.driver", MERGE_DRIVER]);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = root;
}

// The command git runs for a set file both sides changed.
pub const MERGE_DRIVER: &str = "jazyk merge-file %O %A %B %P";

// Whether a project-relative path is committed state, by the rule `IGNORE` states.
pub fn committed(path: &str) -> bool {
    let Some(rest) = path.strip_prefix(".jazyk/") else {
        return false;
    };
    if matches!(
        rest,
        "jazyk.toml" | ".gitignore" | ".gitattributes" | "README.md"
    ) {
        return true;
    }
    let mut parts = rest.split('/');
    let Some(set) = parts.next() else {
        return false;
    };
    if set.is_empty() || set == "local" {
        return false;
    }
    let inner: Vec<&str> = parts.collect();
    if inner.is_empty()
        || inner
            .iter()
            .any(|c| c.is_empty() || (c.len() > 4 && c.starts_with('.') && c.ends_with(".tmp")))
    {
        return false;
    }
    matches!(
        inner.as_slice(),
        ["store.yaml"] | ["graph" | "sources" | "gen", _, ..]
    )
}
pub type Result<T> = std::result::Result<T, (u16, String)>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentSet {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub docs: Vec<String>,
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default)]
    pub source: Vec<String>,
    #[serde(flatten)]
    pub settings: toml::Table,
}

#[derive(Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub sets: Vec<DocumentSet>,
    pub hash: String,
    pub exists: bool,
    pub(crate) settings: toml::Table,
}

impl Workspace {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(CONFIG);
        let exists = path.vexists();
        let text = if exists {
            crate::vfs::read_to_string(path).map_err(|e| (400, e.to_string()))?
        } else {
            String::new()
        };
        Self::parse(root, &text, exists)
    }

    pub(crate) fn parse(root: &Path, text: &str, exists: bool) -> Result<Self> {
        let mut settings: toml::Table = toml::from_str(if exists { text } else { EMPTY_CONFIG })
            .map_err(|e| (400, format!("{}: {}", CONFIG, e)))?;
        if settings.remove("version").and_then(|v| v.as_integer()) != Some(1) {
            return Err((400, "configuration requires version = 1".into()));
        }
        let sets: Vec<DocumentSet> = settings
            .remove("sets")
            .unwrap_or(toml::Value::Array(vec![]))
            .try_into()
            .map_err(|e| (400, format!("invalid document sets: {}", e)))?;
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for set in &sets {
            if !valid_id(&set.id) || !ids.insert(set.id.clone()) {
                return Err((400, format!("invalid or duplicate set ID: {}", set.id)));
            }
            if set.name.trim().is_empty() || !names.insert(set.name.trim().to_lowercase()) {
                return Err((400, format!("empty or duplicate set name: {}", set.name)));
            }
            if !set.docs.is_empty() && !set.docs.iter().any(|p| !p.starts_with('!')) {
                return Err((
                    400,
                    format!("{} excludes documents but selects none", set.name),
                ));
            }
            for pattern in set.docs.iter().chain(&set.roots).chain(&set.source) {
                validate_pattern(pattern).map_err(|e| (400, format!("{}: {}", set.name, e)))?;
            }
        }
        Ok(Self {
            root: root.to_path_buf(),
            sets,
            hash: crate::model::hash_hex(text),
            exists,
            settings,
        })
    }

    pub fn select(&self, selector: &str) -> Result<&DocumentSet> {
        self.sets
            .iter()
            .find(|s| s.id == selector || s.name.to_lowercase() == selector.trim().to_lowercase())
            .ok_or_else(|| (404, format!("unknown document set: {}", selector)))
    }

    pub fn resolve(&self, selector: Option<&str>) -> Result<Project> {
        let set = match selector {
            Some(id) => self.select(id)?,
            None if self.sets.len() == 1 => &self.sets[0],
            None if self.sets.is_empty() => {
                return Err((
                    409,
                    "no-document-set: this project has no document set yet. Add one under Document sets to choose which documents Jazyk compiles; the files stay browsable meanwhile".into(),
                ))
            }
            None => return Err((400, "choose a document set explicitly".into())),
        };
        let mut settings = toml::Value::Table(self.settings.clone());
        merge(&mut settings, toml::Value::Table(set.settings.clone()));
        let mut p = Project::from_settings(&self.root, &settings);
        p.set_id = Some(set.id.clone());
        p.set_name = Some(set.name.clone());
        p.out = self.root.join(".jazyk").join(&set.id);
        p.docs_glob = set.docs.clone();
        p.roots = set.roots.clone();
        p.gen_deliverable = if set.source.is_empty() {
            None
        } else {
            Some(".".into())
        };
        p.gen_code = set.source.clone();
        p.config_hash = self.hash.clone();
        Ok(p)
    }

    /// File operations use a union, but graph operations require resolve().
    pub fn files_project(&self) -> Project {
        let mut p = Project::from_settings(&self.root, &toml::Value::Table(self.settings.clone()));
        p.out = self.root.join(".jazyk");
        p.gen_deliverable = self
            .sets
            .iter()
            .any(|s| !s.source.is_empty())
            .then(|| ".".into());
        p.gen_code = self
            .sets
            .iter()
            .flat_map(|s| s.source.iter().cloned())
            .collect();
        p.docs_glob.clear();
        // The documents are resolved when asked for, not here: loading the
        // workspace must stay cheap enough to run on every request.
        p.set_docs = Some(self.sets.iter().map(|s| s.docs.clone()).collect());
        p.config_hash = self.hash.clone();
        p
    }

    pub fn files(&self) -> Vec<String> {
        files_under(&self.root)
    }

    pub fn memberships(&self, path: &str) -> Vec<Value> {
        if !safe_relative(&self.root, path) {
            return vec![];
        }
        self.sets.iter().flat_map(|s| {
            [("docs", &s.docs), ("source", &s.source)].into_iter().filter_map(move |(role, patterns)| {
                matched(patterns, path).map(|pattern| json!({"setId": s.id, "name": s.name, "role": role, "matchedPattern": pattern}))
            })
        }).collect()
    }

    pub fn summary(&self) -> Value {
        let files = self.files();
        let sets: Vec<Value> = self.sets.iter().map(|set| {
            let p = self.resolve(Some(&set.id)).expect("validated set");
            let docs: Vec<_> = p.doc_files().iter().map(|f| crate::webapi::status::rel_doc(&self.root, f)).collect();
            let source: Vec<_> = files.iter().filter(|f| included(&set.source, f)).cloned().collect();
            let store = crate::store::Store::load(&p.out);
            let hashes = crate::control::current_doc_hashes(&p);
            let docs_dirty = hashes.len() != store.docs.len() || hashes.iter().any(|(name, hash)| store.docs.get(name).map(|r| &r.content_hash) != Some(hash));
            let board = crate::board::Board::compute(&p, &p.out);
            let counts = board.counts();
            let pending_answers = store.graph.diagnostics.values().any(|d| d.lifecycle == "open" && d.answer.as_ref().is_some_and(|a| a.status == "handling"));
            let dirty = docs_dirty || pending_answers || counts.open > 0 || counts.failed > 0;
            let warnings: Vec<_> = set.docs.iter().chain(&set.source).chain(&set.roots)
                .filter(|pattern| !pattern.starts_with('!') && !files.iter().any(|f| crate::project::glob_match(pattern, f)))
                .map(|pattern| format!("No files match {}", pattern)).collect();
            json!({"id": set.id, "name": set.name, "docs": set.docs, "roots": set.roots, "source": set.source,
                "documents": docs, "sourceFiles": source, "dirty": dirty,
                "state": if docs.is_empty() { "empty" } else if dirty { "dirty" } else { "clean" },
                "generation": store.status.generation, "verdict": store.status.verdict, "counts": counts, "warnings": warnings})
        }).collect();
        let removed: Vec<Value> = crate::vfs::read_dir(self.root.join(".jazyk/local/removed-sets"))
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| crate::vfs::read_to_string(entry.path()).ok())
            .filter_map(|text| toml::from_str::<DocumentSet>(&text).ok())
            .filter(|set| !self.sets.iter().any(|s| s.id == set.id))
            .map(|set| json!(set))
            .collect();
        let mut summary = json!({"sets": sets, "removed": removed, "hash": self.hash, "exists": self.exists, "root": self.root.to_string_lossy()});
        if !self.exists {
            if let Some(suggested) = legacy_suggestion(&self.root) {
                summary["suggested"] = suggested;
            }
        }
        summary
    }
}

// The paths a root jazyk.toml from before document sets still names, offered as the
// first set's suggestion; never read as configuration.
// Mirrors docs/compiler/document-sets.md#discovery-and-opening.
pub fn legacy_suggestion(root: &Path) -> Option<Value> {
    let text = crate::vfs::read_to_string(root.join("jazyk.toml")).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    let strings = |section: &str, key: &str| -> Vec<String> {
        table
            .get(section)
            .and_then(|s| s.get(key))
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let docs = strings("docs", "glob");
    if docs.is_empty() {
        return None;
    }
    Some(json!({"name": "Main", "docs": docs, "roots": strings("roots", "files")}))
}

fn merge(base: &mut toml::Value, overrides: toml::Value) {
    if let (Some(to), Some(from)) = (base.as_table_mut(), overrides.as_table()) {
        for (key, value) in from {
            if value
                .as_table()
                .and_then(|t| t.get("disabled"))
                .and_then(toml::Value::as_bool)
                == Some(true)
            {
                to.remove(key);
            } else if let Some(existing) = to.get_mut(key) {
                merge(existing, value.clone());
            } else {
                to.insert(key.clone(), value.clone());
            }
        }
    } else {
        *base = overrides;
    }
}

// A set id: an older `ds_` id, or the minted form, lowercase letters, digits, and `-`,
// starting with a letter or digit, never `local`.
// Mirrors docs/compiler/document-sets.md#persistence-and-identity.
pub fn valid_id(id: &str) -> bool {
    if id.len() >= 100 {
        return false;
    }
    if let Some(rest) = id.strip_prefix("ds_") {
        return !rest.is_empty()
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-');
    }
    id != "local"
        && id
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn validate_pattern(pattern: &str) -> std::result::Result<(), String> {
    let path = pattern.strip_prefix('!').unwrap_or(pattern);
    if path.is_empty()
        || path.starts_with('/')
        || path.contains(['\\', '\0', ':'])
        || path.split('/').any(|c| {
            c.is_empty()
                || c == "."
                || c == ".."
                || c == ".jazyk"
                || c == ".git"
                || c == "target"
                || c == "node_modules"
        })
    {
        return Err(format!("invalid project-relative pattern: {}", pattern));
    }
    globset::GlobBuilder::new(path)
        .literal_separator(true)
        .build()
        .map(|_| ())
        .map_err(|e| format!("invalid glob {}: {}", pattern, e))
}

// Shared lexical policy for authored project scaffolding. Build/output readers
// retain their own cache policy; every caller keeps these hidden-path exclusions.
pub(crate) fn allowed_components(path: &str) -> bool {
    if path.is_empty() || path.contains(['\\', '\0', ':']) {
        return false;
    }
    let mut components = path.split('/').peekable();
    while let Some(c) = components.next() {
        if c.is_empty()
            || (c.starts_with('.')
                && c != ".github"
                && !(c == ".gitignore" && components.peek().is_none()))
        {
            return false;
        }
    }
    true
}

pub fn eligible(path: &str) -> bool {
    allowed_components(path)
        && !path
            .split('/')
            .any(|c| c == "target" || c == "node_modules" || c.starts_with("jazyk-out"))
}

// Files agents read as instructions. Only a pattern naming one without a wildcard
// matches it. Mirrors docs/compiler/document-sets.md#project-configuration.
pub const AGENT_INSTRUCTION_FILES: &[&str] = &[
    "AGENTS.md",
    "AGENTS.override.md",
    "CLAUDE.md",
    "CLAUDE.local.md",
    "GEMINI.md",
];

pub fn agent_instructions(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    AGENT_INSTRUCTION_FILES.contains(&name)
}

// Whether a pattern can match an agent instruction file: only a literal one can.
pub fn pattern_reaches(pattern: &str, path: &str) -> bool {
    !agent_instructions(path) || !pattern.contains(['*', '?', '[', '{'])
}

pub fn matched<'a>(patterns: &'a [String], path: &str) -> Option<&'a str> {
    patterns
        .iter()
        .rev()
        .find(|p| {
            let raw = p.strip_prefix('!').unwrap_or(p);
            pattern_reaches(raw, path) && crate::project::glob_match(raw, path)
        })
        .filter(|p| !p.starts_with('!'))
        .map(String::as_str)
}
pub fn included(patterns: &[String], path: &str) -> bool {
    matched(patterns, path).is_some()
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum FileRole {
    Docs,
    Source,
}
impl FileRole {
    fn opposite(self) -> Self {
        match self {
            Self::Docs => Self::Source,
            Self::Source => Self::Docs,
        }
    }
    fn patterns(self, set: &DocumentSet) -> &[String] {
        match self {
            Self::Docs => &set.docs,
            Self::Source => &set.source,
        }
    }
    fn patterns_mut(self, set: &mut DocumentSet) -> &mut Vec<String> {
        match self {
            Self::Docs => &mut set.docs,
            Self::Source => &mut set.source,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleIntent {
    role: FileRole,
    patterns: Vec<String>,
}

// Existing paths and possible paths named by the rules prove ordinary overlaps,
// including empty selected folders. Complex future overlaps stay behind the
// source-write guard; this is not a glob subset or intersection parser.
fn known_role_overlap(left: &[String], right: &[String], files: &[String]) -> bool {
    known_role_overlap_in(left, right, &[], files)
}

fn known_role_overlap_in(
    left: &[String],
    right: &[String],
    scope: &[String],
    files: &[String],
) -> bool {
    let mut candidates = files.to_vec();
    for pattern in left
        .iter()
        .chain(right)
        .chain(scope)
        .filter(|p| !p.starts_with('!'))
    {
        candidates.push(pattern.clone());
        let folder = if pattern == "**" {
            Some("")
        } else {
            pattern.strip_suffix("/**")
        };
        if let Some(folder) = folder {
            for extension in [
                "md", "markdown", "txt", "adoc", "asciidoc", "asc", "rst", "rest", "docx", "pdf",
            ] {
                candidates.push(format!(
                    "{}__jazyk_role_probe__.{extension}",
                    if folder.is_empty() {
                        String::new()
                    } else {
                        format!("{folder}/")
                    }
                ));
            }
        }
    }
    candidates.iter().any(|p| {
        included(left, p) && included(right, p) && (scope.is_empty() || included(scope, p))
    })
}

fn positive_enabled(patterns: &[String], pattern: &str) -> bool {
    let direct = patterns
        .iter()
        .rposition(|p| p == pattern)
        .is_some_and(|index| {
            !patterns[index + 1..].iter().any(|p| {
                p.strip_prefix('!')
                    .is_some_and(|p| p == pattern || crate::project::glob_match(p, pattern))
            })
        });
    direct || included(patterns, pattern)
}

fn enabled_patterns(before: &[String], after: &[String]) -> Vec<String> {
    let mut patterns: Vec<_> = after
        .iter()
        .filter(|p| {
            !p.starts_with('!')
                && positive_enabled(after, p)
                && (!before.contains(p) || !positive_enabled(before, p))
        })
        .cloned()
        .collect();
    for excluded in before.iter().filter_map(|p| p.strip_prefix('!')) {
        if !included(before, excluded)
            && included(after, excluded)
            && !patterns.iter().any(|p| p == excluded)
        {
            patterns.push(excluded.into());
        }
    }
    patterns
}

fn append_exclusion(patterns: &mut Vec<String>, selected: &str) -> bool {
    let excluded = format!("!{selected}");
    // An exact exclusion still applies to every future path when no later
    // positive rule can re-enable part of it. Keep repeated saves byte-stable.
    if let Some(index) = patterns.iter().rposition(|p| p == &excluded) {
        if patterns[index + 1..].iter().all(|p| p.starts_with('!')) {
            return false;
        }
    }
    patterns.push(excluded);
    true
}

fn resolve_role_edits(
    before: &Workspace,
    after: &mut Workspace,
    id: &str,
    body: &Value,
) -> Result<Vec<Value>> {
    let Some(target) = after.sets.iter().position(|s| s.id == id) else {
        return Ok(vec![]);
    };
    let original = after.sets.clone();
    let requested = after.sets[target].clone();
    let files = after.files();
    let mut intents: Vec<RoleIntent> = if let Some(value) = body.get("roleIntents") {
        if body.get("roleIntent").is_some() {
            return Err((400, "use roleIntents or roleIntent, not both".into()));
        }
        serde_json::from_value(value.clone())
            .map_err(|e| (400, format!("invalid roleIntents: {e}")))?
    } else if let Some(value) = body.get("roleIntent") {
        vec![serde_json::from_value(value.clone())
            .map_err(|e| (400, format!("invalid roleIntent: {e}")))?]
    } else {
        vec![]
    };
    for intent in &intents {
        let selected = intent.role.patterns(&requested);
        for pattern in &intent.patterns {
            validate_pattern(pattern).map_err(|e| (400, e))?;
            let source_fallback =
                intent.role == FileRole::Source && pattern == "**" && selected.contains(pattern);
            if pattern.starts_with('!') || !(source_fallback || positive_enabled(selected, pattern))
            {
                return Err((
                    400,
                    format!(
                        "role intent must name a positive selection enabled by the edit: {pattern}"
                    ),
                ));
            }
        }
    }
    let explicit = !intents.is_empty();
    // Replaying the resolved form is a no-op, even when the client retains its
    // transient selection history for retry after a lost response.
    if explicit
        && before.sets.len() == after.sets.len()
        && before.sets.iter().all(|old| {
            after
                .sets
                .iter()
                .find(|set| set.id == old.id)
                .is_some_and(|set| old.docs == set.docs && old.source == set.source)
        })
        && !intents.iter().any(|intent| {
            intent.patterns.iter().any(|pattern| {
                let scope = [pattern.clone()];
                after.sets.iter().any(|opposing| {
                    known_role_overlap_in(
                        intent.role.patterns(&after.sets[target]),
                        intent.role.opposite().patterns(opposing),
                        &scope,
                        &files,
                    )
                })
            })
        })
    {
        return Ok(vec![]);
    }
    if intents.is_empty() {
        let old = before.sets.iter().find(|s| s.id == id);
        let docs = enabled_patterns(old.map_or(&[], |s| s.docs.as_slice()), &requested.docs);
        let old_source: &[String] = old.map_or(&[], |s| s.source.as_slice());
        let mut source = enabled_patterns(old_source, &requested.source);
        if requested.source.iter().any(|p| p == "**") && old_source != requested.source.as_slice() {
            // Removing an exclusion under Entire project chooses fallback
            // coverage, rather than reclassifying that document subtree.
            source.retain(|p| requested.source.contains(p));
            if !source.iter().any(|p| p == "**") {
                source.insert(0, "**".into());
            }
        }
        if !docs.is_empty() && !source.is_empty() && known_role_overlap(&docs, &source, &files) {
            // Broad documents with a specific source selection assign that
            // selection to source; broad source `**` is fallback coverage that
            // the pass below reconciles so documents are preserved.
            let broad_docs = docs.iter().any(|p| p == "**" || p.starts_with("**/"));
            if broad_docs || source.iter().any(|p| p == "**") {
                intents.push(RoleIntent {
                    role: FileRole::Source,
                    patterns: source,
                });
            } else {
                return Err((400, "document and source selections overlap; edit one role at a time or supply ordered roleIntents to choose their roles".into()));
            }
        } else {
            if !docs.is_empty() {
                intents.push(RoleIntent {
                    role: FileRole::Docs,
                    patterns: docs,
                });
            }
            if !source.is_empty() {
                intents.push(RoleIntent {
                    role: FileRole::Source,
                    patterns: source,
                });
            }
        }
    }
    // Entire-project source is fallback coverage. Specific assignments keep
    // their ordering and take precedence even if the broad checkbox came last.
    if intents
        .iter()
        .any(|i| i.role == FileRole::Source && i.patterns.iter().any(|p| p == "**"))
        && after
            .sets
            .iter()
            .any(|set| known_role_overlap(&after.sets[target].source, &set.docs, &files))
    {
        let prior = before
            .sets
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.source.clone())
            .unwrap_or_default();
        let current = after.sets[target].source.clone();
        let retained = if current.iter().any(|p| p != "**") {
            current
        } else {
            prior
        };
        let docs: Vec<_> = after
            .sets
            .iter()
            .flat_map(|s| s.docs.iter())
            .filter(|p| !p.starts_with('!'))
            .cloned()
            .collect();
        let mut patterns = vec!["**".to_string()];
        for pattern in docs {
            append_exclusion(&mut patterns, &pattern);
        }
        for pattern in retained.into_iter().filter(|p| p != "**") {
            if let Some(positive) = pattern.strip_prefix('!') {
                append_exclusion(&mut patterns, positive);
            } else if !patterns.contains(&pattern) || !included(&patterns, &pattern) {
                // A broad form edit keeps an already selected specific source
                // scope. It must not use a raw unresolved overlap to take prose.
                if !before.sets.iter().any(|set| included(&set.docs, &pattern)) {
                    for set in &mut after.sets {
                        if included(&set.docs, &pattern) {
                            append_exclusion(&mut set.docs, &pattern);
                        }
                    }
                }
                patterns.push(pattern);
            }
        }
        after.sets[target].source = patterns;
    }
    let mut automatic_exclusions: Vec<(FileRole, usize)> = Vec::new();
    for intent in intents {
        for pattern in intent.patterns {
            if intent.role == FileRole::Source && pattern == "**" {
                continue;
            }
            let selected = intent.role.patterns_mut(&mut after.sets[target]);
            let last_positive = selected.iter().rposition(|p| p == &pattern);
            if last_positive.is_none()
                || last_positive.is_some_and(|index| {
                    (selected[index + 1..].iter().any(|p| p.starts_with('!'))
                        && !included(selected, &pattern))
                        || automatic_exclusions
                            .iter()
                            .any(|(role, position)| *role == intent.role && *position > index)
                })
            {
                selected.push(pattern.clone());
            }
            for set in &mut after.sets {
                let opposite = intent.role.opposite();
                let opposing = opposite.patterns_mut(set);
                if opposing.is_empty()
                    || (!explicit
                        && !known_role_overlap(opposing, std::slice::from_ref(&pattern), &files))
                {
                    continue;
                }
                if append_exclusion(opposing, &pattern) {
                    let position = opposing.len() - 1;
                    if set.id == id {
                        automatic_exclusions.push((opposite, position));
                    }
                }
            }
        }
    }
    // Report only additions relative to the requested form, including a repeated
    // exclusion appended after a later inclusion. Group by affected set and role.
    let mut adjustments = Vec::new();
    for set in &after.sets {
        let old = original.iter().find(|s| s.id == set.id).expect("same sets");
        for role in [FileRole::Docs, FileRole::Source] {
            let mut excluded = Vec::new();
            for pattern in role.patterns(set).iter().filter(|p| p.starts_with('!')) {
                if !excluded.contains(pattern)
                    && role.patterns(set).iter().filter(|p| *p == pattern).count()
                        > role.patterns(old).iter().filter(|p| *p == pattern).count()
                {
                    excluded.push(pattern.clone());
                }
            }
            if !excluded.is_empty() {
                adjustments.push(json!({"setId":set.id,"role":role,"excluded":excluded}));
            }
        }
    }
    Ok(adjustments)
}

// Every eligible file under the project, project-relative, sorted. A full walk with
// a canonicalize per entry: call it when the files are needed, never per request.
pub fn files_under(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let canonical_root = root.vcanonicalize().unwrap_or_else(|_| root.to_path_buf());
    walk(
        root,
        &canonical_root,
        root,
        &mut BTreeSet::new(),
        &mut files,
    );
    files.sort();
    files.dedup();
    files
}

fn walk(
    root: &Path,
    canonical_root: &Path,
    dir: &Path,
    seen: &mut BTreeSet<PathBuf>,
    files: &mut Vec<String>,
) {
    let Ok(canonical) = dir.vcanonicalize() else {
        return;
    };
    if !canonical.starts_with(canonical_root) || !seen.insert(canonical) {
        return;
    }
    if dir != root && dir.join(CONFIG).vexists() {
        return;
    }
    let Ok(entries) = crate::vfs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if !eligible(&relative) {
            continue;
        }
        if path.vis_dir() {
            walk(root, canonical_root, &path, seen, files);
        } else if path
            .vcanonicalize()
            .is_ok_and(|p| p.starts_with(canonical_root))
        {
            files.push(relative);
        }
    }
}

// A random token (lock names, temporary files, lease owners).
pub fn mint_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("system randomness for a token");
    format!(
        "ds_{}",
        bytes
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>()
    )
}

// Whether a path segment reads as a set id where a plain directory name could stand
// instead (an out path's first segment): an older `ds_` id or the minted shape, a slug,
// `-`, and five Crockford base 32 characters.
pub fn looks_like_set_id(segment: &str) -> bool {
    if segment.starts_with("ds_") {
        return valid_id(segment);
    }
    match segment.rsplit_once('-') {
        Some((slug, suffix)) => {
            !slug.is_empty()
                && suffix.len() == 5
                && suffix.bytes().all(|c| CROCKFORD.contains(&c))
                && valid_id(segment)
        }
        None => false,
    }
}

// The longest slug part of a set id.
const SET_SLUG_MAX: usize = 24;
const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

// A new set's id: the id slug of its name cut to 24 characters, `-`, and five random
// lowercase Crockford base 32 characters, drawn again while `taken` holds it. E.g.
// `My set` -> `my-set-ks8mf`. Mirrors docs/compiler/document-sets.md#persistence-and-identity.
pub fn mint_set_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut slug = crate::ids::slug(name);
    if slug == "x" && !name.chars().any(|c| c.is_alphanumeric()) {
        slug = "set".to_string();
    }
    if slug.len() > SET_SLUG_MAX {
        let head = &slug[..SET_SLUG_MAX];
        let cut = match head.rfind('-') {
            Some(i) if i > 0 && slug.as_bytes()[SET_SLUG_MAX] != b'-' => i,
            _ => SET_SLUG_MAX,
        };
        slug = slug[..cut].trim_end_matches('-').to_string();
    }
    loop {
        let mut bytes = [0u8; 5];
        getrandom::fill(&mut bytes).expect("system randomness for a document set ID");
        let suffix: String = bytes
            .iter()
            .map(|b| CROCKFORD[(*b & 31) as usize] as char)
            .collect();
        let id = format!("{}-{}", slug, suffix);
        if valid_id(&id) && !taken(&id) {
            return id;
        }
    }
}

/// A short filesystem lock protects optimistic config replacement. Hosted callers
/// already hold their project lease; WASM requests are serialized by their worker.
pub struct ProjectLock {
    root: PathBuf,
    name: String,
    #[cfg(not(target_arch = "wasm32"))]
    _file: Option<std::fs::File>,
}
impl Drop for ProjectLock {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(file) = &self._file {
            // Closing only this descriptor can leave the lock held by a concurrent
            // fork until the child execs. End the operation's lock explicitly.
            let _ = fs2::FileExt::unlock(file);
        }
    }
}
impl ProjectLock {
    pub fn acquire(root: &Path, name: &str) -> Result<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if !crate::vfs::is_scoped() {
                let local = crate::layout::project_local_dir(&root.join(".jazyk"));
                crate::vfs::create_dir_all(&local).map_err(|e| (500, e.to_string()))?;
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(local.join(name))
                    .map_err(|e| (500, e.to_string()))?;
                fs2::FileExt::try_lock_exclusive(&file)
                    .map_err(|_| (409, "another project operation is in progress".into()))?;
                return Ok(Self {
                    root: root.into(),
                    name: name.into(),
                    _file: Some(file),
                });
            }
            Ok(Self {
                root: root.into(),
                name: name.into(),
                _file: None,
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            Ok(Self {
                root: root.into(),
                name: name.into(),
            })
        }
    }
}

pub fn atomic_config(root: &Path, text: &str, base_hash: &str) -> Result<()> {
    Workspace::parse(root, text, true)?;
    let _lock = ProjectLock::acquire(root, "config.lock")?;
    if Workspace::load(root)?.hash != base_hash {
        return Err((409, "configuration changed; reload before saving".into()));
    }
    crate::vfs::create_dir_all(root.join(".jazyk")).map_err(|e| (500, e.to_string()))?;
    let path = root.join(CONFIG);
    let tmp = root.join(".jazyk").join(format!("{}.tmp", mint_id()));
    crate::vfs::write(&tmp, text).map_err(|e| (500, e.to_string()))?;
    if let Err(e) = crate::vfs::rename(&tmp, &path) {
        crate::vfs::remove_file(&tmp).ok();
        return Err((500, e.to_string()));
    }
    ensure_ignore(root).map_err(|e| (500, e.to_string()))?;
    drop(_lock);
    // Every jazyk write to jazyk.toml refreshes the AGENTS.md section it describes
    // (docs/frontends/agent-context.md#when-it-is-written).
    crate::agents_md::sync(root).report();
    Ok(())
}

/// Create/update/remove one set, or preview the exact same edit without writing.
pub fn edit(root: &Path, body: &Value, preview: bool) -> Result<Value> {
    edit_locked(root, body, preview, false, None)
}

pub(crate) fn edit_current_set(
    root: &Path,
    body: &Value,
    preview: bool,
    id: &str,
) -> Result<Value> {
    edit_locked(root, body, preview, false, Some(id))
}

pub(crate) fn edit_current_set_with_lock(
    root: &Path,
    body: &Value,
    preview: bool,
    id: &str,
    lock: &ProjectLock,
) -> Result<Value> {
    if lock.root != root || lock.name != "project.lock" {
        return Err((
            409,
            "the matcher edit does not hold this project's lock".into(),
        ));
    }
    edit_locked(root, body, preview, true, Some(id))
}

/// An internal build already holds project.lock. Only its current lease owner
/// can reuse it; optimistic replacement still takes config.lock.
pub(crate) fn edit_in_build(
    root: &Path,
    out: &Path,
    token: &str,
    body: &Value,
    preview: bool,
    id: &str,
) -> Result<Value> {
    if !crate::control::build_lease(out).is_some_and(|lease| lease.worker == token) {
        return Err((
            409,
            "the document-set edit does not own the running build lease".into(),
        ));
    }
    edit_locked(root, body, preview, true, Some(id))
}

fn edit_locked(
    root: &Path,
    body: &Value,
    preview: bool,
    owns_project_lock: bool,
    only_set: Option<&str>,
) -> Result<Value> {
    let before = Workspace::load(root)?;
    if body["baseHash"].as_str() != Some(before.hash.as_str()) {
        return Err((409, "configuration changed; reload before saving".into()));
    }
    if let Some(id) = body["purge"].as_str() {
        if !valid_id(id) || before.sets.iter().any(|s| s.id == id) {
            return Err((400, "only a removed set can be purged".into()));
        }
        let saved = root
            .join(".jazyk/local/removed-sets")
            .join(format!("{}.toml", id));
        if !saved.vexists() {
            return Err((404, "unknown removed set".into()));
        }
        if !preview {
            let _lock = ProjectLock::acquire(root, "project.lock")?;
            if Workspace::load(root)?.hash != before.hash {
                return Err((409, "configuration changed; reload before saving".into()));
            }
            let out = root.join(".jazyk").join(id);
            if out.vexists() {
                crate::vfs::remove_dir_all(out).map_err(|e| (500, e.to_string()))?;
            }
            crate::vfs::remove_file(saved).map_err(|e| (500, e.to_string()))?;
        }
        return Ok(Workspace::load(root)?.summary());
    }
    let text =
        crate::vfs::read_to_string(root.join(CONFIG)).unwrap_or_else(|_| EMPTY_CONFIG.into());
    let mut document: toml_edit::DocumentMut = text.parse().map_err(|e| (400, format!("{}", e)))?;
    if document
        .get("sets")
        .and_then(|s| s.as_array())
        .is_some_and(|a| a.is_empty())
    {
        document.remove("sets");
    }
    if document.get("sets").is_none() {
        document["sets"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }
    let tables = document["sets"]
        .as_array_of_tables_mut()
        .ok_or_else(|| (400, "sets must be an array of tables".into()))?;
    let removal = body["remove"].as_str();
    let restore = body["restore"].as_str();
    let mut restored = None;
    let mut data = body["set"].clone();
    if let Some(id) = restore {
        if !valid_id(id) || before.sets.iter().any(|s| s.id == id) {
            return Err((400, "invalid removed set".into()));
        }
        let saved = crate::vfs::read_to_string(
            root.join(".jazyk/local/removed-sets")
                .join(format!("{}.toml", id)),
        )
        .map_err(|e| (404, e.to_string()))?;
        let set: DocumentSet = toml::from_str(&saved).map_err(|e| (400, e.to_string()))?;
        data = json!(set);
        restored = Some(
            saved
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| (400, e.to_string()))?,
        );
    }
    let id = removal
        .map(str::to_string)
        .or_else(|| data["id"].as_str().map(str::to_string))
        .unwrap_or_else(|| {
            let name = data["name"].as_str().unwrap_or("");
            let taken: BTreeSet<String> = before.sets.iter().map(|s| s.id.clone()).collect();
            mint_set_id(name, |id| {
                taken.contains(id)
                    || root.join(".jazyk").join(id).vexists()
                    || root
                        .join(".jazyk/removed-sets")
                        .join(format!("{}.toml", id))
                        .vexists()
            })
        });
    if !valid_id(&id) {
        return Err((400, "invalid document set ID".into()));
    }
    let position = tables
        .iter()
        .position(|t| t.get("id").and_then(|i| i.as_str()) == Some(&id));
    if removal.is_none()
        && restore.is_none()
        && position.is_none()
        && (root
            .join(".jazyk/local/removed-sets")
            .join(format!("{}.toml", id))
            .vexists()
            || root.join(".jazyk").join(&id).vexists())
    {
        return Err((
            409,
            "this ID belongs to retained state; restore that set or use a new ID".into(),
        ));
    }
    if removal.is_some() {
        let Some(index) = position else {
            return Err((404, "unknown document set".into()));
        };
        tables.remove(index);
    } else {
        if !data.is_object() {
            return Err((400, "set must be an object".into()));
        }
        data["id"] = json!(id);
        let table = match position {
            Some(i) => tables.get_mut(i).unwrap(),
            None => {
                tables.push(toml_edit::Table::new());
                tables.iter_mut().last().unwrap()
            }
        };
        if let Some(saved) = restored {
            *table = saved.as_table().clone();
        }
        table["id"] = toml_edit::value(id.clone());
        for field in ["name", "docs", "roots", "source"] {
            if let Some(value) = data.get(field) {
                table[field] = if field == "name" {
                    let name = value
                        .as_str()
                        .ok_or_else(|| (400, "name must be text".into()))?
                        .trim();
                    if table.get(field).and_then(|v| v.as_str()) == Some(name) {
                        continue;
                    }
                    toml_edit::value(name)
                } else {
                    let values: Vec<String> = serde_json::from_value(value.clone())
                        .map_err(|_| (400, format!("{} must be a list of patterns", field)))?;
                    if table
                        .get(field)
                        .and_then(|v| v.as_array())
                        .is_some_and(|array| {
                            array
                                .iter()
                                .filter_map(|v| v.as_str())
                                .eq(values.iter().map(String::as_str))
                        })
                    {
                        continue;
                    }
                    toml_edit::value(values.into_iter().collect::<toml_edit::Array>())
                };
            }
        }
    }
    let mut after = Workspace::parse(root, &document.to_string(), true)?;
    let role_adjustments = if removal.is_none() && restore.is_none() {
        resolve_role_edits(&before, &mut after, &id, body)?
    } else {
        vec![]
    };
    for table in document["sets"]
        .as_array_of_tables_mut()
        .expect("validated sets")
        .iter_mut()
    {
        let set = after
            .sets
            .iter()
            .find(|s| Some(s.id.as_str()) == table.get("id").and_then(|v| v.as_str()))
            .expect("validated set");
        for (field, patterns) in [("docs", &set.docs), ("source", &set.source)] {
            let existing: Vec<String> = table
                .get(field)
                .and_then(|v| v.as_array())
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if &existing != patterns {
                if patterns.starts_with(&existing)
                    && table.get(field).and_then(|v| v.as_array()).is_some()
                {
                    let array = table[field].as_array_mut().expect("existing array");
                    for pattern in &patterns[existing.len()..] {
                        array.push(pattern.as_str());
                    }
                } else {
                    table[field] =
                        toml_edit::value(patterns.iter().cloned().collect::<toml_edit::Array>());
                }
            }
        }
    }
    let next = document.to_string();
    let after = Workspace::parse(root, &next, true)?;
    let mut value = after.summary();
    value["setId"] = json!(id);
    value["baseHash"] = json!(before.hash);
    value["roleAdjustments"] = json!(role_adjustments);
    value["affectedSetIds"] = json!(after
        .sets
        .iter()
        .filter(|set| {
            before
                .sets
                .iter()
                .find(|s| s.id == set.id)
                .is_none_or(|old| {
                    old.docs != set.docs || old.source != set.source || old.roots != set.roots
                })
        })
        .map(|set| &set.id)
        .collect::<Vec<_>>());
    if let Some(id) = only_set {
        if value["affectedSetIds"]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|affected| affected.as_str() != Some(id)))
        {
            return Err((409, "this matcher repair would change another document set; resolve that assignment through the project settings".into()));
        }
    }
    let files = after.files();
    value["overlaps"] = json!(files
        .iter()
        .filter_map(|path| {
            let memberships = after.memberships(path);
            (memberships.iter().filter(|m| m["role"] == "docs").count() > 1)
                .then(|| json!({"path": path, "memberships": memberships}))
        })
        .collect::<Vec<_>>());
    value["changes"] = json!(after.sets.iter().map(|set| {
        let old = before.sets.iter().find(|s| s.id == set.id);
        json!({"setId": set.id,
            "added": files.iter().filter(|f| included(&set.docs, f) && !old.is_some_and(|o| included(&o.docs, f))).collect::<Vec<_>>(),
            "removed": files.iter().filter(|f| !included(&set.docs, f) && old.is_some_and(|o| included(&o.docs, f))).collect::<Vec<_>>(),
            "sourceAdded": files.iter().filter(|f| included(&set.source, f) && !old.is_some_and(|o| included(&o.source, f))).collect::<Vec<_>>(),
            "sourceRemoved": files.iter().filter(|f| !included(&set.source, f) && old.is_some_and(|o| included(&o.source, f))).collect::<Vec<_>>()})
    }).collect::<Vec<_>>());
    if !preview {
        let _build = if owns_project_lock {
            None
        } else {
            Some(ProjectLock::acquire(root, "project.lock")?)
        };
        if removal.is_some() {
            let set = before.select(&id)?;
            let folder = root.join(".jazyk/local/removed-sets");
            crate::vfs::create_dir_all(&folder).map_err(|e| (500, e.to_string()))?;
            crate::vfs::write(
                folder.join(format!("{}.toml", id)),
                toml::to_string(set).map_err(|e| (500, e.to_string()))?,
            )
            .map_err(|e| (500, e.to_string()))?;
        }
        atomic_config(root, &next, &before.hash)?;
        if restore.is_some() {
            let _ = crate::vfs::remove_file(
                root.join(".jazyk/local/removed-sets")
                    .join(format!("{}.toml", id)),
            );
        }
    }
    Ok(value)
}

/// Check the existing ancestry too, including when the final file is new.
pub fn safe_relative(root: &Path, relative: &str) -> bool {
    if !eligible(relative) {
        return false;
    }
    contained_relative(root, relative)
}

// Readers of recorded outputs need containment without treating build caches as
// authored inputs. Callers still apply their credential and role-specific policy.
pub(crate) fn contained_relative(root: &Path, relative: &str) -> bool {
    contained_relative_with_policy(root, relative, allowed_components)
}

// Task products may explicitly include build caches. Canonical ancestry and
// nested project boundaries stay shared; each caller keeps its path policy.
pub(crate) fn contained_relative_with_policy(
    root: &Path,
    relative: &str,
    allowed: impl Fn(&str) -> bool,
) -> bool {
    if !allowed(relative) {
        return false;
    }
    let Ok(base) = root.vcanonicalize() else {
        return false;
    };
    let path = root.join(relative);
    let mut checked_destination = false;
    for ancestor in path.ancestors().take_while(|p| *p != root) {
        if ancestor.vis_dir() && ancestor.join(CONFIG).vexists() {
            return false;
        }
        if ancestor.vexists() {
            let Ok(resolved) = ancestor.vcanonicalize() else {
                return false;
            };
            if !resolved.starts_with(&base) {
                return false;
            }
            if !checked_destination {
                let relative = resolved.strip_prefix(&base).expect("contained destination");
                if !relative.as_os_str().is_empty()
                    && !allowed(&relative.to_string_lossy().replace('\\', "/"))
                {
                    return false;
                }
                // The deepest existing destination also owns files reached through
                // a public alias. Its real project boundary survives the alias.
                if resolved
                    .ancestors()
                    .take_while(|p| *p != base.as_path())
                    .any(|ancestor| ancestor.vis_dir() && ancestor.join(CONFIG).vexists())
                {
                    return false;
                }
                checked_destination = true;
            }
        }
    }
    true
}

#[cfg(test)]
mod symlink_tests;

#[cfg(test)]
mod tests {
    use super::*;

    // The generated ignore file is the block the docs state, byte for byte.
    // Mirrors docs/compiler/document-sets.md#persistence-and-identity.
    // Agent instruction files are reached only by a literal pattern.
    // Mirrors docs/compiler/document-sets.md#project-configuration.
    #[test]
    fn wildcards_skip_agent_instruction_files() {
        let all = vec!["**/*.md".to_string()];
        for path in [
            "AGENTS.md",
            "docs/CLAUDE.md",
            "GEMINI.md",
            "a/AGENTS.override.md",
            "CLAUDE.local.md",
        ] {
            assert!(!included(&all, path), "{path}");
        }
        assert!(included(&all, "docs/agents.md"));
        assert!(included(&all, "docs/main.md"));
        assert!(
            included(&["docs/AGENTS.md".to_string()], "docs/AGENTS.md"),
            "a literal pattern names it"
        );
        assert!(!included(
            &["docs/**".to_string(), "!AGENTS.md".to_string()],
            "docs/AGENTS.md"
        ));
        assert!(
            !included(&["src/**".to_string()], "src/AGENTS.md"),
            "source wildcards skip it too"
        );
    }

    #[test]
    fn the_generated_ignore_file_is_the_documented_one() {
        let doc = include_str!("../../docs/compiler/document-sets.md");
        assert!(
            doc.contains(&format!("```\n{}```\n", IGNORE)),
            "docs and IGNORE disagree"
        );
        let folder = include_str!("../../docs/compiler/jazyk-folder.md");
        assert!(
            folder.contains(&format!("```\n{}```\n", IGNORE)),
            "jazyk-folder.md and IGNORE disagree"
        );
        assert!(
            folder.contains(&format!("```\n{}```\n", ATTRIBUTES)),
            "jazyk-folder.md and ATTRIBUTES disagree"
        );
        for kind in crate::store::DERIVED_KINDS {
            assert!(!IGNORE.contains(kind), "{kind}");
        }
    }

    // Written when absent or still an earlier generated file; an owner's file stays.
    // The attributes and the README are written beside it on the same terms.
    #[test]
    fn ensure_ignore_writes_absent_and_earlier_files_and_keeps_the_owners() {
        let f = Fixture::new();
        let path = f.0.join(".jazyk/.gitignore");
        assert!(ensure_ignore(&f.0).unwrap());
        assert_eq!(crate::vfs::read_to_string(&path).unwrap(), IGNORE);
        assert_eq!(
            crate::vfs::read_to_string(f.0.join(".jazyk/.gitattributes")).unwrap(),
            ATTRIBUTES
        );
        assert_eq!(
            crate::vfs::read_to_string(f.0.join(".jazyk/README.md")).unwrap(),
            README
        );
        assert!(!ensure_ignore(&f.0).unwrap());
        for earlier in EARLIER_IGNORES {
            crate::vfs::write(&path, earlier).unwrap();
            assert!(ensure_ignore(&f.0).unwrap());
            assert_eq!(crate::vfs::read_to_string(&path).unwrap(), IGNORE);
        }
        crate::vfs::write(&path, "*\n!jazyk.toml\n").unwrap();
        assert!(!ensure_ignore(&f.0).unwrap());
        assert_eq!(
            crate::vfs::read_to_string(&path).unwrap(),
            "*\n!jazyk.toml\n"
        );
        crate::vfs::write(f.0.join(".jazyk/README.md"), "mine\n").unwrap();
        ensure_ignore(&f.0).unwrap();
        assert_eq!(
            crate::vfs::read_to_string(f.0.join(".jazyk/README.md")).unwrap(),
            "mine\n"
        );
    }

    // A new set's id is its name as a slug and five random characters; older ids stay
    // valid; `local` is never an id.
    #[test]
    fn set_ids_are_minted_from_the_name() {
        let id = mint_set_id("My set", |_| false);
        assert!(
            id.starts_with("my-set-") && id.len() == "my-set-".len() + 5,
            "{id}"
        );
        assert!(valid_id(&id));
        assert!(mint_set_id("!!!", |_| false).starts_with("set-"));
        let long = mint_set_id("The Very Long Name Of A Document Set", |_| false);
        assert!(long.len() <= 24 + 6 && !long.contains("--"), "{long}");
        let mut seen = std::collections::BTreeSet::new();
        let first = mint_set_id("Shop", |_| false);
        seen.insert(first.clone());
        let second = mint_set_id("Shop", |id| seen.contains(id));
        assert_ne!(first, second);
        assert!(valid_id("ds_main") && valid_id("ds_7b91362a262947c0b363a30768ad419b"));
        assert!(!valid_id("local") && !valid_id("My-Set") && !valid_id("-x") && !valid_id(""));
        assert!(looks_like_set_id(&id) && looks_like_set_id("ds_main"));
        assert!(!looks_like_set_id("diagrams") && !looks_like_set_id("derived"));
    }

    const CHECKOUT_PATHS: [(&str, bool); 34] = [
        (".jazyk/jazyk.toml", true),
        (".jazyk/.gitignore", true),
        (".jazyk/.gitattributes", true),
        (".jazyk/README.md", true),
        (".jazyk/my-set-ks8mf/store.yaml", true),
        (".jazyk/my-set-ks8mf/graph/entities/ent~order.yaml", true),
        (
            ".jazyk/my-set-ks8mf/graph/requirements/req~main-4k7q.yaml",
            true,
        ),
        (
            ".jazyk/my-set-ks8mf/graph/criteria/crit~main-4k7q-m2.yaml",
            true,
        ),
        (".jazyk/my-set-ks8mf/graph/views/view~class+shop.yaml", true),
        (
            ".jazyk/my-set-ks8mf/graph/diagnostics/diag~lint-7hq2.yaml",
            true,
        ),
        (".jazyk/my-set-ks8mf/graph/redirects/ent~old.yaml", true),
        (
            ".jazyk/my-set-ks8mf/graph/decisions/pairs/req~a%7Ereq~b.yaml",
            true,
        ),
        (
            ".jazyk/my-set-ks8mf/graph/decisions/lookalikes/ent~a%7Eent~b.yaml",
            true,
        ),
        (".jazyk/my-set-ks8mf/sources/docs/main.md.yaml", true),
        (".jazyk/my-set-ks8mf/gen/ledger.yaml", true),
        (
            ".jazyk/my-set-ks8mf/gen/requirements/req~main-4k7q.yaml",
            true,
        ),
        (".jazyk/my-set-ks8mf/gen/criteria/crit-main-1.md", true),
        (".jazyk/ds_main/store.yaml", true),
        (".jazyk/ds_main/graph/entities/e.yaml", true),
        (
            ".jazyk/my-set-ks8mf/graph/entities/.ent~order.yaml.12.tmp",
            false,
        ),
        (".jazyk/my-set-ks8mf/stray.txt", false),
        (".jazyk/my-set-ks8mf/local/build.yaml", false),
        (".jazyk/my-set-ks8mf/local/run.yaml", false),
        (".jazyk/my-set-ks8mf/local/journal/g1.yaml", false),
        (".jazyk/my-set-ks8mf/local/.lock", false),
        (".jazyk/my-set-ks8mf/local/merged.yaml", false),
        (
            ".jazyk/my-set-ks8mf/derived/graph/relationships/rel~a%7Eb.yaml",
            false,
        ),
        (".jazyk/my-set-ks8mf/derived/diagrams/class/shop.svg", false),
        (".jazyk/my-set-ks8mf/derived/docsgen/order.md", false),
        (".jazyk/local/project.lock", false),
        (".jazyk/local/ide.json", false),
        (".jazyk/local/archive/my-set-ks8mf-v4/store.yaml", false),
        (".jazyk/local/records/s/feedback.jsonl", false),
        (".jazyk/my-set-ks8mf/status.yaml", false),
    ];

    // `committed` answers what the ignore file answers. With a git binary at hand the
    // test asks git itself which files a checkout would add.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn committed_state_follows_the_ignore_file() {
        for (path, want) in CHECKOUT_PATHS {
            assert_eq!(committed(path), want, "{path}");
        }
        let f = Fixture::new();
        ensure_ignore(&f.0).unwrap();
        crate::vfs::write(f.0.join(CONFIG), EMPTY_CONFIG).unwrap();
        for (path, _) in CHECKOUT_PATHS {
            let p = f.0.join(path);
            crate::vfs::create_dir_all(p.parent().unwrap()).unwrap();
            if !p.vexists() {
                crate::vfs::write(&p, "x: 1\n").unwrap();
            }
        }
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&f.0)
                .output()
        };
        if !run(&["init", "-q"]).is_ok_and(|o| o.status.success()) {
            eprintln!("no git binary; the ignore file was checked by `committed` alone");
            return;
        }
        let listed = run(&["ls-files", "--others", "--exclude-standard", "--", ".jazyk"]).unwrap();
        let listed: BTreeSet<String> = String::from_utf8_lossy(&listed.stdout)
            .lines()
            .map(String::from)
            .collect();
        let want: BTreeSet<String> = CHECKOUT_PATHS
            .iter()
            .filter(|(_, c)| *c)
            .map(|(p, _)| p.to_string())
            .collect();
        assert_eq!(listed, want);
    }

    #[cfg(unix)]
    #[test]
    fn project_lock_ends_with_its_guard_even_with_a_duplicate_descriptor() {
        let f = Fixture::new();
        let lock = ProjectLock::acquire(&f.0, "documents.lock").unwrap();
        // A concurrently forked child can temporarily retain the same descriptor.
        let duplicate = lock._file.as_ref().unwrap().try_clone().unwrap();
        assert!(ProjectLock::acquire(&f.0, "documents.lock").is_err());
        drop(lock);
        let next = ProjectLock::acquire(&f.0, "documents.lock").unwrap();
        drop(duplicate);
        assert!(ProjectLock::acquire(&f.0, "documents.lock").is_err());
        drop(next);
        assert!(ProjectLock::acquire(&f.0, "documents.lock").is_ok());
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("jazyk-sets-{}", mint_id()));
            crate::vfs::create_dir_all(root.join("docs")).unwrap();
            crate::vfs::create_dir_all(root.join("src")).unwrap();
            crate::vfs::write(root.join("docs/a.md"), "# A\n\nA does something.\n").unwrap();
            crate::vfs::write(root.join("docs/b.md"), "# B\n\nB does something else.\n").unwrap();
            crate::vfs::write(root.join("src/shared.rs"), "fn shared() {}\n").unwrap();
            Self(root)
        }
        fn add(&self, id: &str, name: &str, docs: &[&str]) -> Value {
            edit(
                &self.0,
                &json!({"baseHash":Workspace::load(&self.0).unwrap().hash,
                "set":{"id":id,"name":name,"docs":docs,"source":["src/**"]}}),
                false,
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            crate::vfs::remove_dir_all(&self.0).ok();
        }
    }

    #[test]
    fn opening_and_preview_do_not_initialize_a_folder() {
        let f = Fixture::new();
        let w = Workspace::load(&f.0).unwrap();
        assert!(!w.exists);
        assert!(w.sets.is_empty());
        let preview = edit(
            &f.0,
            &json!({"baseHash":w.hash,"set":{"name":"Feature A","docs":["docs/a.md"]}}),
            true,
        )
        .unwrap();
        assert_eq!(preview["sets"][0]["documents"], json!(["docs/a.md"]));
        assert!(!f.0.join(".jazyk").vexists());
    }

    #[test]
    fn a_set_can_start_with_no_documents() {
        let f = Fixture::new();
        let hash = Workspace::load(&f.0).unwrap().hash;
        let saved = edit(
            &f.0,
            &json!({"baseHash":hash,"set":{"id":"ds_a","name":"Later","docs":[]}}),
            false,
        )
        .unwrap();
        assert_eq!(saved["sets"][0]["documents"], json!([]));
        assert_eq!(saved["sets"][0]["state"], "empty");
        let hash = Workspace::load(&f.0).unwrap().hash;
        assert!(edit(&f.0, &json!({"baseHash":hash,"set":{"id":"ds_b","name":"Only exclusions","docs":["!docs/a.md"]}}), false).is_err());
    }

    #[test]
    fn sets_are_independent_and_rename_preserves_store_identity() {
        let f = Fixture::new();
        f.add("ds_a", "A", &["docs/a.md"]);
        f.add("ds_b", "B", &["docs/b.md"]);
        let w = Workspace::load(&f.0).unwrap();
        assert!(w.resolve(None).is_err());
        let a = w.resolve(Some("A")).unwrap();
        let b = w.resolve(Some("B")).unwrap();
        assert_eq!(a.out, f.0.join(".jazyk/ds_a"));
        assert_ne!(a.out, b.out);
        assert_eq!(a.doc_files(), vec![f.0.join("docs/a.md")]);
        crate::vfs::create_dir_all(a.out.join("journal")).unwrap();
        crate::vfs::write(a.out.join("journal/keep.yaml"), "keep").unwrap();
        edit(
            &f.0,
            &json!({"baseHash":w.hash,"set":{"id":"ds_a","name":"Renamed"}}),
            false,
        )
        .unwrap();
        let next = Workspace::load(&f.0)
            .unwrap()
            .resolve(Some("Renamed"))
            .unwrap();
        assert_eq!(a.out, next.out);
        assert!(next.out.join("journal/keep.yaml").vexists());
        assert_eq!(b.doc_files(), vec![f.0.join("docs/b.md")]);
    }

    #[test]
    fn removed_sets_restore_their_identity_and_purge_only_retained_state() {
        let f = Fixture::new();
        f.add("ds_a", "A", &["docs/a.md"]);
        let out = f.0.join(".jazyk/ds_a");
        crate::vfs::create_dir_all(&out).unwrap();
        crate::vfs::write(out.join("retained.txt"), "history").unwrap();
        let apply = |body: Value| {
            let mut body = body;
            body["baseHash"] = json!(Workspace::load(&f.0).unwrap().hash);
            edit(&f.0, &body, false)
        };
        apply(json!({"remove":"ds_a"})).unwrap();
        assert_eq!(
            Workspace::load(&f.0).unwrap().summary()["removed"][0]["id"],
            "ds_a"
        );
        assert!(apply(json!({"set":{"id":"ds_a","name":"Other","docs":["docs/b.md"]}})).is_err());
        apply(json!({"restore":"ds_a"})).unwrap();
        assert_eq!(Workspace::load(&f.0).unwrap().sets[0].name, "A");
        assert!(out.join("retained.txt").vexists());
        assert!(apply(json!({"purge":"ds_a"})).is_err());
        apply(json!({"remove":"ds_a"})).unwrap();
        apply(json!({"purge":"ds_a"})).unwrap();
        assert!(!out.vexists());
        assert!(f.0.join("docs/a.md").vexists());
    }

    #[test]
    fn shared_memberships_and_exclusions_use_the_same_resolver() {
        let f = Fixture::new();
        f.add("ds_a", "A", &["docs/**/*.md", "!docs/b.md"]);
        f.add("ds_b", "B", &["docs/a.md"]);
        let w = Workspace::load(&f.0).unwrap();
        assert_eq!(w.memberships("docs/a.md").len(), 2);
        assert!(w.memberships("docs/b.md").is_empty());
        assert_eq!(w.memberships("src/shared.rs").len(), 2);
        assert_eq!(
            w.resolve(Some("ds_a")).unwrap().doc_files(),
            vec![f.0.join("docs/a.md")]
        );
        assert_eq!(w.summary()["sets"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn project_scaffolding_keeps_the_other_hidden_and_path_boundaries() {
        let f = Fixture::new();
        crate::vfs::create_dir_all(f.0.join(".jazyk")).unwrap();
        crate::vfs::write(f.0.join(CONFIG), "version = 1\n[[sets]]\nid = 'ds_a'\nname = 'A'\ndocs = ['**', '!deliverable/**']\nsource = ['deliverable/**']\n").unwrap();
        for path in [
            ".github/workflows/check.yml",
            ".github/README.md",
            ".gitignore",
            "deliverable/.github/workflows/deploy.yml",
            "deliverable/.gitignore",
            "deliverable/.github/.gitignore",
            "deliverable/.github/README.md",
            "deliverable/.env",
            "deliverable/.ssh/config",
            "deliverable/.aws/config",
            "deliverable/.git/config",
            "deliverable/.jazyk/state.yaml",
            "deliverable/.cache/output",
            "deliverable/.github/.env",
        ] {
            let path = f.0.join(path);
            crate::vfs::create_dir_all(path.parent().unwrap()).unwrap();
            crate::vfs::write(path, "fixture\n").unwrap();
        }
        let workspace = Workspace::load(&f.0).unwrap();
        let project = workspace.resolve(Some("ds_a")).unwrap();
        let gs = crate::gen::GenSettings::resolve(&project);
        let files = workspace.files();
        for path in [
            "deliverable/.github/workflows/deploy.yml",
            "deliverable/.gitignore",
            "deliverable/.github/.gitignore",
        ] {
            assert!(eligible(path), "{path}");
            assert!(safe_relative(&f.0, path), "{path}");
            assert!(gs.allows_source(path), "{path}");
            assert!(files.contains(&path.to_string()), "{path}: {files:?}");
            assert_eq!(workspace.memberships(path)[0]["role"], "source");
        }
        assert!(files.contains(&".github/workflows/check.yml".into()));
        assert!(files.contains(&".gitignore".into()));
        for path in [
            ".github/README.md",
            ".github/workflows/check.yml",
            ".gitignore",
        ] {
            assert!(!project.doc_files().contains(&f.0.join(path)), "{path}");
            assert!(
                !workspace
                    .files_project()
                    .doc_files()
                    .contains(&f.0.join(path)),
                "{path}"
            );
        }
        assert!(
            !gs.allows_source(".github/workflows/check.yml"),
            "scaffolding outside Source stays excluded"
        );
        assert!(!project
            .doc_files()
            .contains(&f.0.join("deliverable/.github/README.md")));
        assert!(
            crate::webapi::docs::safe_doc_path(&project, "deliverable/.github/README.md").is_none()
        );
        for path in [
            "deliverable/.env",
            "deliverable/.ssh/config",
            "deliverable/.aws/config",
            "deliverable/.git/config",
            "deliverable/.jazyk/state.yaml",
            "deliverable/.cache/output",
            "deliverable/.github/.env",
            "deliverable/.github/.ssh/config",
            "deliverable/.github/../escape",
            "deliverable/.gitignore/child",
            "../.github/a.yml",
            "/deliverable/.github/a.yml",
            "deliverable//.github/a.yml",
            "deliverable/.github/a:b.yml",
            "deliverable/target/.github/a.yml",
            "deliverable/node_modules/.gitignore",
        ] {
            assert!(!eligible(path), "{path}");
            assert!(!gs.allows_source(path), "{path}");
            assert!(!files.contains(&path.to_string()), "{path}");
        }
        // Even a broad raw Source rule cannot overwrite another set's Documents.
        let mut config = crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap();
        config.push_str("\n[[sets]]\nid = 'ds_b'\nname = 'B'\ndocs = ['deliverable/.github/README.md']\nsource = []\n");
        crate::vfs::write(f.0.join(CONFIG), config).unwrap();
        assert!(!gs.allows_source("deliverable/.github/README.md"));
        // A nested project owns even a newly created scaffolding path below it.
        crate::vfs::create_dir_all(f.0.join("deliverable/nested/.jazyk")).unwrap();
        crate::vfs::write(
            f.0.join("deliverable/nested/.jazyk/jazyk.toml"),
            "version = 1\n",
        )
        .unwrap();
        assert!(!safe_relative(
            &f.0,
            "deliverable/nested/.github/workflows/new.yml"
        ));
        assert!(!gs.allows_source("deliverable/nested/.github/workflows/new.yml"));
        #[cfg(unix)]
        {
            let outside = Fixture::new();
            std::os::unix::fs::symlink(&outside.0, f.0.join("deliverable/.github/escape")).unwrap();
            assert!(!safe_relative(&f.0, "deliverable/.github/escape/new.yml"));
            assert!(!gs.allows_source("deliverable/.github/escape/new.yml"));
            assert!(!Workspace::load(&f.0)
                .unwrap()
                .files()
                .iter()
                .any(|p| p.contains("/escape/")));
        }
    }

    #[cfg(unix)]
    #[test]
    fn public_aliases_keep_nested_project_source_boundaries_for_existing_and_new_files() {
        let f = Fixture::new();
        crate::vfs::create_dir_all(f.0.join(".jazyk")).unwrap();
        crate::vfs::write(f.0.join(CONFIG), "version = 1\n[[sets]]\nid = 'ds_a'\nname = 'A'\ndocs = ['docs/**']\nsource = ['deliverable/**']\n").unwrap();
        let nested = f.0.join("deliverable/nested");
        crate::vfs::create_dir_all(nested.join(".jazyk")).unwrap();
        crate::vfs::write(nested.join(CONFIG), "version = 1\nsets = []\n").unwrap();
        crate::vfs::write(nested.join("logo.svg"), "NESTED_PROJECT_INPUT").unwrap();
        std::os::unix::fs::symlink(nested.join("logo.svg"), f.0.join("deliverable/public.svg"))
            .unwrap();
        std::os::unix::fs::symlink(&nested, f.0.join("deliverable/alias")).unwrap();
        crate::vfs::write(f.0.join("deliverable/.env"), "PRIVATE_INPUT").unwrap();
        std::os::unix::fs::symlink(
            f.0.join("deliverable/.env"),
            f.0.join("deliverable/private.svg"),
        )
        .unwrap();
        let project = Workspace::load(&f.0)
            .unwrap()
            .resolve(Some("ds_a"))
            .unwrap();
        let gs = crate::gen::GenSettings::resolve(&project);
        for path in [
            "deliverable/nested/logo.svg",
            "deliverable/nested/new.svg",
            "deliverable/public.svg",
            "deliverable/alias/logo.svg",
            "deliverable/alias/new.svg",
            "deliverable/private.svg",
        ] {
            assert!(!contained_relative(&f.0, path), "{path}");
            assert!(!safe_relative(&f.0, path), "{path}");
            assert!(!gs.allows_source(path), "{path}");
        }
    }

    fn edit_set(f: &Fixture, set: Value, intents: Value, preview: bool) -> Result<Value> {
        let mut body = json!({"baseHash":Workspace::load(&f.0).unwrap().hash,"set":set});
        if !intents.is_null() {
            body["roleIntents"] = intents;
        }
        edit(&f.0, &body, preview)
    }

    #[test]
    fn empty_source_folder_is_excluded_from_broad_documents_before_files_exist() {
        let f = Fixture::new();
        f.add("ds_a", "A", &["**"]);
        crate::vfs::create_dir_all(f.0.join("deliverable")).unwrap();
        let original = crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap();
        let body = json!({"baseHash":Workspace::load(&f.0).unwrap().hash,
            "set":{"id":"ds_a","source":["src/**","deliverable/**"]}});
        let preview = edit(&f.0, &body, true).unwrap();
        assert_eq!(
            preview["sets"][0]["docs"],
            json!(["**", "!src/**", "!deliverable/**"])
        );
        assert_eq!(
            preview["roleAdjustments"],
            json!([{"setId":"ds_a","role":"docs","excluded":["!deliverable/**"]}])
        );
        assert_eq!(preview["affectedSetIds"], json!(["ds_a"]));
        assert_eq!(
            crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap(),
            original
        );
        assert_eq!(preview, edit(&f.0, &body, false).unwrap());
        crate::vfs::write(f.0.join("deliverable/README.md"), "# Product\n").unwrap();
        let workspace = Workspace::load(&f.0).unwrap();
        let project = workspace.resolve(Some("ds_a")).unwrap();
        assert!(!project
            .doc_files()
            .contains(&f.0.join("deliverable/README.md")));
        assert!(crate::gen::GenSettings::resolve(&project).allows_source("deliverable/README.md"));
        let text = crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap();
        let resolved = &workspace.sets[0];
        let repeat = edit_set(
            &f,
            json!({"id":"ds_a","docs":resolved.docs,"source":resolved.source}),
            Value::Null,
            false,
        )
        .unwrap();
        assert_eq!(repeat["roleAdjustments"], json!([]));
        assert_eq!(repeat["affectedSetIds"], json!([]));
        assert_eq!(crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap(), text);
        for pattern in [
            "**/*.md",
            "**/*.markdown",
            "**/*.txt",
            "**/*.docx",
            "**/*.pdf",
        ] {
            let f = Fixture::new();
            f.add("ds_a", "A", &[pattern]);
            edit_set(
                &f,
                json!({"id":"ds_a","source":["src/**","deliverable/**"]}),
                Value::Null,
                false,
            )
            .unwrap();
            let workspace = Workspace::load(&f.0).unwrap();
            assert!(
                workspace.sets[0].docs.contains(&"!deliverable/**".into()),
                "{pattern}"
            );
        }
    }

    #[test]
    fn specific_roles_resolve_across_sets_and_keep_same_role_sharing() {
        let f = Fixture::new();
        f.add("ds_a", "A", &["docs/a.md"]);
        f.add("ds_b", "B", &["docs/a.md", "docs/b.md"]);
        let preview = edit_set(
            &f,
            json!({"id":"ds_a","source":["src/**","docs/**"]}),
            Value::Null,
            true,
        )
        .unwrap();
        assert_eq!(preview["affectedSetIds"], json!(["ds_a", "ds_b"]));
        assert_eq!(
            preview["changes"][1]["removed"],
            json!(["docs/a.md", "docs/b.md"])
        );
        edit_set(
            &f,
            json!({"id":"ds_a","source":["src/**","docs/**"]}),
            Value::Null,
            false,
        )
        .unwrap();
        let old_source = crate::gen::GenSettings::resolve(
            &Workspace::load(&f.0)
                .unwrap()
                .resolve(Some("ds_a"))
                .unwrap(),
        );
        assert!(old_source.allows_source("docs/a.md"));
        let saved = edit_set(
            &f,
            json!({"id":"ds_b","docs":["docs/a.md","docs/b.md","!docs/**","docs/a.md"]}),
            json!([{"role":"docs","patterns":["docs/a.md"]}]),
            false,
        )
        .unwrap();
        assert_eq!(saved["changes"][0]["sourceRemoved"], json!(["docs/a.md"]));
        assert!(saved["sets"][1]["dirty"].as_bool().unwrap());
        assert!(
            !old_source.allows_source("docs/a.md"),
            "an existing source scope must honor the new document claim"
        );
        edit_set(
            &f,
            json!({"id":"ds_a","docs":["docs/a.md"]}),
            Value::Null,
            false,
        )
        .unwrap();
        let workspace = Workspace::load(&f.0).unwrap();
        assert_eq!(
            workspace
                .memberships("docs/a.md")
                .iter()
                .filter(|m| m["role"] == "docs")
                .count(),
            2
        );
        assert_eq!(
            workspace
                .memberships("src/shared.rs")
                .iter()
                .filter(|m| m["role"] == "source")
                .count(),
            2
        );
        assert_eq!(
            workspace
                .memberships("docs/a.md")
                .iter()
                .filter(|m| m["role"] == "source")
                .count(),
            0
        );
    }

    #[test]
    fn ordered_specific_assignments_override_broad_source_and_keep_child_documents() {
        let f = Fixture::new();
        let intents = json!([
            {"role":"source","patterns":["deliverable/**"]},
            {"role":"docs","patterns":["deliverable/README.md"]},
            {"role":"source","patterns":["**"]}
        ]);
        let result = edit_set(
            &f,
            json!({"id":"ds_a","name":"A","docs":["**"],"source":["**","deliverable/**"]}),
            intents.clone(),
            false,
        )
        .unwrap();
        let workspace = Workspace::load(&f.0).unwrap();
        let set = &workspace.sets[0];
        assert!(included(&set.source, "deliverable/main.rs"));
        assert!(!included(&set.docs, "deliverable/main.rs"));
        assert!(included(&set.docs, "deliverable/README.md"));
        assert!(!included(&set.source, "deliverable/README.md"));
        assert!(included(&set.docs, "docs/a.md"));
        assert!(!included(&set.source, "docs/a.md"));
        // An unrelated raw overlap must not make a resolved retry accumulate
        // another copy of each ordered assignment's temporary exclusions.
        crate::vfs::create_dir_all(f.0.join("elsewhere")).unwrap();
        crate::vfs::write(f.0.join("elsewhere/claim.md"), "# Unrelated\n").unwrap();
        let config = format!("{}\n[[sets]]\nid='ds_other'\nname='Other'\ndocs=['elsewhere/claim.md']\nsource=['elsewhere/**']\n",
            crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap());
        crate::vfs::write(f.0.join(CONFIG), config).unwrap();
        let text = crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap();
        let repeat = edit_set(
            &f,
            json!({"id":"ds_a","docs":set.docs,"source":set.source}),
            intents,
            false,
        )
        .unwrap();
        assert_eq!(repeat["roleAdjustments"], json!([]));
        assert_eq!(repeat["sets"][0]["docs"], result["sets"][0]["docs"]);
        assert_eq!(repeat["sets"][0]["source"], result["sets"][0]["source"]);
        assert_eq!(crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap(), text);
    }

    #[test]
    fn later_source_folder_assignment_reclaims_an_earlier_document_child() {
        for intents in [
            json!([
                {"role":"docs","patterns":["deliverable/README.md"]},
                {"role":"source","patterns":["deliverable/**"]}
            ]),
            json!([
                {"role":"source","patterns":["deliverable/**"]},
                {"role":"docs","patterns":["deliverable/README.md"]},
                {"role":"source","patterns":["deliverable/**"]}
            ]),
        ] {
            let f = Fixture::new();
            edit_set(&f, json!({"id":"ds_a","name":"A","docs":["**","deliverable/README.md"],"source":["deliverable/**"]}), intents, false).unwrap();
            let workspace = Workspace::load(&f.0).unwrap();
            assert!(included(&workspace.sets[0].source, "deliverable/README.md"));
            assert!(!included(&workspace.sets[0].docs, "deliverable/README.md"));
        }
    }

    #[test]
    fn broad_source_preserves_documents_and_unpartitioned_broad_roles_keep_docs_protected() {
        let f = Fixture::new();
        edit_set(
            &f,
            json!({"id":"ds_a","name":"A","docs":["docs/**"],"source":["**"]}),
            json!([{"role":"source","patterns":["**"]}]),
            false,
        )
        .unwrap();
        let workspace = Workspace::load(&f.0).unwrap();
        assert_eq!(workspace.sets[0].source, vec!["**", "!docs/**"]);
        assert!(included(&workspace.sets[0].source, "src/shared.rs"));
        assert!(!included(&workspace.sets[0].source, "docs/a.md"));
        let result = edit_set(
            &f,
            json!({"id":"ds_a","docs":["**"],"source":["**"]}),
            Value::Null,
            false,
        )
        .unwrap();
        assert_eq!(result["sets"][0]["sourceFiles"], json!([]));
        assert!(included(
            &Workspace::load(&f.0).unwrap().sets[0].docs,
            "docs/a.md"
        ));
    }

    #[test]
    fn broader_form_preserves_an_existing_specific_source_partition() {
        let f = Fixture::new();
        f.add("ds_a", "A", &["docs/**"]);
        edit_set(
            &f,
            json!({"id":"ds_a","docs":["**"],"source":["**"]}),
            Value::Null,
            false,
        )
        .unwrap();
        let workspace = Workspace::load(&f.0).unwrap();
        let set = &workspace.sets[0];
        assert!(included(&set.source, "src/shared.rs"));
        assert!(!included(&set.docs, "src/shared.rs"));
        assert!(included(&set.docs, "docs/a.md"));
        assert!(!included(&set.source, "docs/a.md"));
    }

    #[test]
    fn ambiguous_advanced_overlap_requires_intent_but_disjoint_edits_and_negatives_work() {
        let f = Fixture::new();
        let bad = json!({"id":"ds_a","name":"A","docs":["docs/**"],"source":["docs/*.md"]});
        assert!(edit_set(&f, bad.clone(), Value::Null, true)
            .unwrap_err()
            .1
            .contains("roleIntents"));
        assert!(!f.0.join(CONFIG).vexists());
        edit_set(
            &f,
            bad,
            json!([{"role":"source","patterns":["docs/*.md"]}]),
            false,
        )
        .unwrap();
        let workspace = Workspace::load(&f.0).unwrap();
        assert!(!included(&workspace.sets[0].docs, "docs/a.md"));
        assert!(included(&workspace.sets[0].source, "docs/a.md"));
        edit_set(&f, json!({"id":"ds_b","name":"B","docs":["docs/b.md","!docs/a.md"],"source":["src/**","!src/private/**"]}), Value::Null, false).unwrap();
        let workspace = Workspace::load(&f.0).unwrap();
        let b = workspace.select("ds_b").unwrap();
        assert!(b.docs.contains(&"!docs/a.md".into()));
        assert!(b.source.contains(&"!src/private/**".into()));
        assert!(!included(&b.source, "src/private/key.txt"));
    }

    #[test]
    fn disabled_positive_globs_do_not_infer_an_assignment_or_override_exclusions() {
        for role in [FileRole::Docs, FileRole::Source] {
            let f = Fixture::new();
            f.add("ds_a", "A", &["docs/a.md"]);
            let (field, patterns) = match role {
                FileRole::Docs => ("docs", json!(["docs/*.md", "!docs/**"])),
                FileRole::Source => ("source", json!(["src/*.rs", "!src/**"])),
            };
            let mut set = json!({"id":"ds_a"});
            set[field] = patterns.clone();
            let body =
                json!({"baseHash":Workspace::load(&f.0).unwrap().hash,"set":set,"roleIntents":[]});
            let preview = edit(&f.0, &body, true).unwrap();
            assert_eq!(preview["roleAdjustments"], json!([]));
            assert_eq!(preview["sets"][0][field], patterns);
            let files = if role == FileRole::Docs {
                "documents"
            } else {
                "sourceFiles"
            };
            assert_eq!(preview["sets"][0][files], json!([]));
            assert_eq!(preview, edit(&f.0, &body, false).unwrap());
            let workspace = Workspace::load(&f.0).unwrap();
            assert_eq!(json!(role.patterns(&workspace.sets[0])), patterns);
        }
    }

    #[test]
    fn invalid_role_intents_do_not_change_configuration_or_files() {
        let f = Fixture::new();
        f.add("ds_a", "A", &["docs/a.md"]);
        let before = crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap();
        for intent in [
            json!({"role":"source","patterns":["!src/**"]}),
            json!({"role":"source","patterns":["../outside/**"]}),
            json!({"role":"source","patterns":["docs/b.md"]}),
            json!({"role":"other","patterns":["src/**"]}),
        ] {
            assert!(edit_set(
                &f,
                json!({"id":"ds_a","source":["src/**"]}),
                json!([intent]),
                false
            )
            .is_err());
            assert_eq!(
                crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap(),
                before
            );
        }
        assert!(edit_set(
            &f,
            json!({"id":"ds_a","source":["src/**","!src/**"]}),
            json!([{"role":"source","patterns":["src/**"]}]),
            false
        )
        .is_err());
        assert_eq!(
            crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap(),
            before
        );
        assert_eq!(
            crate::vfs::read_to_string(f.0.join("docs/a.md")).unwrap(),
            "# A\n\nA does something.\n"
        );
    }

    #[test]
    fn raw_toml_overlap_remains_readable_but_cannot_authorize_source_writes() {
        let f = Fixture::new();
        crate::vfs::create_dir_all(f.0.join(".jazyk")).unwrap();
        crate::vfs::write(f.0.join(CONFIG), "version = 1\n[[sets]]\nid = 'ds_a'\nname = 'A'\ndocs = ['**']\nsource = ['deliverable/**']\n").unwrap();
        let workspace = Workspace::load(&f.0).unwrap();
        let project = workspace.resolve(Some("ds_a")).unwrap();
        assert!(!crate::gen::GenSettings::resolve(&project).allows_source("deliverable/README.md"));
        edit_set(
            &f,
            json!({"id":"ds_a","name":"Renamed"}),
            Value::Null,
            false,
        )
        .unwrap();
        assert_eq!(Workspace::load(&f.0).unwrap().sets[0].docs, vec!["**"]);
        assert!(!crate::gen::GenSettings::resolve(&project).allows_source("deliverable/README.md"));
    }

    #[test]
    fn rechecking_an_unchanged_source_selection_resolves_another_sets_document_claim() {
        let f = Fixture::new();
        crate::vfs::create_dir_all(f.0.join(".jazyk")).unwrap();
        crate::vfs::write(f.0.join("src/a.md"), "# Source\n").unwrap();
        crate::vfs::write(f.0.join(CONFIG), "version = 1\n[[sets]]\nid = 'ds_a'\nname = 'A'\ndocs = ['docs/a.md']\nsource = ['src/**']\n[[sets]]\nid = 'ds_b'\nname = 'B'\ndocs = ['src/a.md']\nsource = []\n").unwrap();
        let intent = json!([{"role":"source","patterns":["src/a.md"]}]);
        let request = json!({"id":"ds_a","source":["src/**"]});
        let preview = edit_set(&f, request.clone(), intent.clone(), true).unwrap();
        assert!(preview["roleAdjustments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|change| change["setId"] == "ds_b" && change["excluded"] == json!(["!src/a.md"])));
        assert_eq!(
            preview,
            edit_set(&f, request, intent.clone(), false).unwrap()
        );
        let workspace = Workspace::load(&f.0).unwrap();
        let project = workspace.resolve(Some("ds_a")).unwrap();
        assert!(crate::gen::GenSettings::resolve(&project).allows_source("src/a.md"));
        assert!(!included(
            &workspace.select("ds_b").unwrap().docs,
            "src/a.md"
        ));
        let text = crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap();
        let set = workspace.select("ds_a").unwrap();
        let repeat = edit_set(
            &f,
            json!({"id":"ds_a","docs":set.docs,"source":set.source}),
            intent,
            false,
        )
        .unwrap();
        assert_eq!(repeat["roleAdjustments"], json!([]));
        assert_eq!(crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap(), text);
    }

    #[test]
    fn stale_edits_and_invalid_patterns_leave_config_unchanged() {
        let f = Fixture::new();
        f.add("ds_a", "A", &["docs/a.md"]);
        let before = crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap();
        let hash = Workspace::load(&f.0).unwrap().hash;
        for body in [
            json!({"baseHash":"stale","set":{"id":"ds_a","name":"Lost"}}),
            json!({"baseHash":hash,"set":{"id":"ds_a","docs":["../elsewhere.md"]}}),
        ] {
            assert!(edit(&f.0, &body, false).is_err());
            assert_eq!(
                crate::vfs::read_to_string(f.0.join(CONFIG)).unwrap(),
                before
            );
        }
    }

    #[test]
    fn comments_unknown_settings_and_other_sets_survive_an_edit() {
        let f = Fixture::new();
        f.add("ds_a", "A", &["docs/a.md"]);
        f.add("ds_b", "B", &["docs/b.md"]);
        let path = f.0.join(CONFIG);
        let text = format!(
            "# Owner comment\n{}\n[custom]\nnote = 'keep me'\n",
            crate::vfs::read_to_string(&path).unwrap()
        );
        crate::vfs::write(&path, text).unwrap();
        let w = Workspace::load(&f.0).unwrap();
        edit(
            &f.0,
            &json!({"baseHash":w.hash,"set":{"id":"ds_a","name":"Renamed"}}),
            false,
        )
        .unwrap();
        let text = crate::vfs::read_to_string(&path).unwrap();
        assert!(text.contains("# Owner comment"));
        assert!(text.contains("note = 'keep me'"));
        assert_eq!(
            Workspace::load(&f.0).unwrap().select("B").unwrap().docs,
            vec!["docs/b.md"]
        );
        let text = text.replace(
            "docs = [\"docs/b.md\"]",
            "docs = [\n  # Keep the owner's selection note.\n  \"docs/b.md\",\n]",
        );
        assert!(text.contains("# Keep the owner's selection note."));
        crate::vfs::write(&path, text).unwrap();
        edit_set(
            &f,
            json!({"id":"ds_a","source":["src/**","docs/**"]}),
            Value::Null,
            false,
        )
        .unwrap();
        let text = crate::vfs::read_to_string(&path).unwrap();
        assert!(
            text.contains("# Keep the owner's selection note."),
            "{text}"
        );
        assert!(text.contains("# Owner comment"));
        assert!(text.contains("note = 'keep me'"));
    }

    #[test]
    fn removal_keeps_documents_source_config_and_other_stores() {
        let f = Fixture::new();
        f.add("ds_a", "A", &["docs/a.md"]);
        f.add("ds_b", "B", &["docs/b.md"]);
        let w = Workspace::load(&f.0).unwrap();
        edit(&f.0, &json!({"baseHash":w.hash,"remove":"ds_a"}), false).unwrap();
        assert!(f.0.join("docs/a.md").vexists());
        assert!(f.0.join("src/shared.rs").vexists());
        assert!(f.0.join(".jazyk/local/removed-sets/ds_a.toml").vexists());
        assert_eq!(Workspace::load(&f.0).unwrap().sets.len(), 1);
    }

    #[test]
    fn nested_projects_and_escaping_symlinks_are_not_input() {
        let f = Fixture::new();
        f.add("ds_a", "A", &["**/*.md"]);
        crate::vfs::create_dir_all(f.0.join("nested/.jazyk")).unwrap();
        crate::vfs::write(f.0.join("nested/.jazyk/jazyk.toml"), EMPTY_CONFIG).unwrap();
        crate::vfs::write(f.0.join("nested/private.md"), "private").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(std::env::temp_dir(), f.0.join("outside")).unwrap();
        let files = Workspace::load(&f.0).unwrap().files();
        assert!(!files
            .iter()
            .any(|f| f.starts_with("nested/") || f.starts_with("outside/")));
    }
}
