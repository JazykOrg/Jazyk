use crate::vfs::VPathExt;
// Project discovery and settings. A project holds .jazyk/jazyk.toml,
// found by walking up from the working directory. Mirrors docs/compiler/project-settings.md.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// Project-level [llm] table. Every field is optional; unset fields fall through to the
// global config, then the built-in default (see cli::resolve_llm).
#[derive(Clone, Default)]
pub struct LlmSettings {
    pub base_url: Option<String>,
    pub api: Option<String>,
    pub reasoning_summary: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub temperature: Option<f64>,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Linting {
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

// One [acp.agents.<name>] profile: how to launch a downstream ACP agent.
// Mirrors docs/compiler/project-settings.md#acp.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct AcpAgentProfile {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    // Serve file and command tools into this agent's sessions (agents with no editor
    // of their own; the embedded agent sets it).
    pub serve_files: bool,
}

// The [acp] table: which agent performs AI work. `embedded` is built in and needs no
// profile. Mirrors docs/compiler/project-settings.md#acp.
#[derive(Clone, Default)]
pub struct AcpSettings {
    pub agent: Option<String>,
    pub agents: BTreeMap<String, AcpAgentProfile>,
}

// Parse the [acp] table out of a jazyk.toml (or the global config).
fn parse_acp(t: &Toml) -> AcpSettings {
    let mut s = AcpSettings {
        agent: t.string("acp.agent"),
        agents: BTreeMap::new(),
    };
    // Profile names are whatever appears as [acp.agents.<name>]; enumerate the keys.
    let mut names: Vec<String> = Vec::new();
    for key in t.strings.keys().chain(t.arrays.keys()) {
        if let Some(rest) = key.strip_prefix("acp.agents.") {
            if let Some((name, _)) = rest.split_once('.') {
                if !names.iter().any(|n| n == name) {
                    names.push(name.to_string());
                }
            }
        }
    }
    for name in names {
        let p = format!("acp.agents.{}", name);
        let env_prefix = format!("{}.env.", p);
        let mut env: Vec<(String, String)> = t
            .strings
            .iter()
            .filter_map(|(k, v)| {
                k.strip_prefix(&env_prefix)
                    .map(|name| (name.to_string(), v.clone()))
            })
            .collect();
        env.sort();
        s.agents.insert(
            name,
            AcpAgentProfile {
                command: t.string(&format!("{}.command", p)).unwrap_or_default(),
                args: t.array(&format!("{}.args", p)).unwrap_or_default(),
                env,
                serve_files: t
                    .string(&format!("{}.serve_files", p))
                    .map(|v| v == "true")
                    .unwrap_or(false),
            },
        );
    }
    s
}

// Read the global [acp] config if present (same file as the global LLM config).
pub fn load_global_acp() -> AcpSettings {
    let home = match crate::sys::env_var("HOME") {
        Ok(h) => h,
        Err(_) => return AcpSettings::default(),
    };
    let candidates = [
        PathBuf::from(&home).join(".jazyk").join("config.toml"),
        PathBuf::from(&home).join(".jazyk.toml"),
    ];
    for c in candidates {
        if let Ok(text) = crate::vfs::read_to_string(&c) {
            return parse_acp(&Toml::parse(&text));
        }
    }
    AcpSettings::default()
}

// The keys the [executors] table accepts: the goal kinds that run in a session and the
// two goal classes. Mirrors docs/compiler/project-settings.md#executors.
pub const EXECUTOR_KEYS: [&str; 17] = [
    "compile",
    "gc",
    "place-anchors",
    "reconcile-section",
    "rejudge-pair",
    "review-entity",
    "retrace",
    "conform-instance",
    "derive-criteria",
    "bind",
    "generate",
    "verify",
    "declare-edges",
    "dedupe-candidates",
    "curate-view",
    "split-view",
    "abstract-entity",
];

// The [executors] table: an ACP profile name per goal kind or goal class, overriding the
// [acp] agent for the sessions of that kind or class. Mirrors
// docs/compiler/project-settings.md#executors.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Executors {
    pub by_kind: BTreeMap<String, String>,
}

impl Executors {
    // The profile for a batch: the kind's key, then the class's key. None defers to
    // the [acp] agent.
    pub fn resolve(&self, kind: &str, class: &str) -> Option<&str> {
        self.by_kind
            .get(kind)
            .or_else(|| self.by_kind.get(class))
            .map(String::as_str)
    }

    // Keys outside the accepted set: a settings error naming them.
    pub fn unknown_keys(&self) -> Vec<String> {
        self.by_kind
            .keys()
            .filter(|k| !EXECUTOR_KEYS.contains(&k.as_str()))
            .cloned()
            .collect()
    }
}

// Parse the [executors] table out of a jazyk.toml (or the global config).
fn parse_executors(t: &Toml) -> Executors {
    Executors {
        by_kind: t
            .strings
            .iter()
            .filter_map(|(k, v)| {
                k.strip_prefix("executors.")
                    .map(|kind| (kind.to_string(), v.clone()))
            })
            .collect(),
    }
}

// Read the global [executors] table if present (same file as the global LLM config).
pub fn load_global_executors() -> Executors {
    let home = match crate::sys::env_var("HOME") {
        Ok(h) => h,
        Err(_) => return Executors::default(),
    };
    let candidates = [
        PathBuf::from(&home).join(".jazyk").join("config.toml"),
        PathBuf::from(&home).join(".jazyk.toml"),
    ];
    for c in candidates {
        if let Ok(text) = crate::vfs::read_to_string(&c) {
            return parse_executors(&Toml::parse(&text));
        }
    }
    Executors::default()
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
        Workflow {
            compile: "manual".into(),
            generate: "manual".into(),
            worker: "any".into(),
        }
    }
}

#[derive(Clone)]
pub struct Project {
    pub root: PathBuf,
    pub set_id: Option<String>,
    pub set_name: Option<String>,
    pub config_hash: String,
    pub config_error: Option<String>,
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
    pub acp: AcpSettings,
    pub linting: Linting,
    pub executors: Executors,
    // When set (ad-hoc `jazyk compile <paths>` with no jazyk.toml), these files are used
    // directly instead of resolving the docs glob.
    pub explicit_files: Option<Vec<PathBuf>>,
    // The files project of a workspace: one docs pattern list per document set. The
    // documents are the union, walked when `doc_files` is asked, never at load.
    pub set_docs: Option<Vec<Vec<String>>>,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            root: PathBuf::from("."),
            set_id: None,
            set_name: None,
            config_hash: String::new(),
            config_error: None,
            out: PathBuf::from("./.jazyk"),
            gen_deliverable: None,
            gen_worker: None,
            gen_code: Vec::new(),
            workflow: Workflow::default(),
            docs_glob: vec!["docs/**/*.md".to_string()],
            roots: vec![],
            llm: LlmSettings::default(),
            acp: AcpSettings::default(),
            linting: Linting::default(),
            executors: Executors::default(),
            explicit_files: None,
            set_docs: None,
        }
    }
}

// Machine-level LLM config, kept out of project settings. Loaded from ~/.jazyk/config.toml
// (or ~/.jazyk.toml). Every field is optional; unset fields fall through to lower-priority sources.
#[derive(Clone, Default)]
pub struct GlobalLlm {
    pub base_url: Option<String>,
    pub api: Option<String>,
    pub reasoning_summary: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub temperature: Option<f64>,
}

// Read the global LLM config if present.
pub fn load_global_llm() -> GlobalLlm {
    let home = match crate::sys::env_var("HOME") {
        Ok(h) => h,
        Err(_) => return GlobalLlm::default(),
    };
    let candidates = [
        PathBuf::from(&home).join(".jazyk").join("config.toml"),
        PathBuf::from(&home).join(".jazyk.toml"),
    ];
    for c in candidates {
        if let Ok(text) = crate::vfs::read_to_string(&c) {
            let t = Toml::parse(&text);
            return GlobalLlm {
                base_url: t.string("llm.base_url"),
                api: t.string("llm.api"),
                reasoning_summary: t.string("llm.reasoning_summary"),
                model: t.string("llm.model"),
                api_key: t.string("llm.api_key"),
                api_key_env: t.string("llm.api_key_env"),
                temperature: t
                    .string("llm.temperature")
                    .and_then(|s| s.parse::<f64>().ok()),
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
    fn recorded_criteria_files_are_never_doc_input() {
        // An llm row's criteria file under the deliverable is generation metadata,
        // even under a glob that matches it. Mirrors docs/compiler/project-settings.md#docs.
        let dir =
            std::env::temp_dir().join(format!("jazyk-critdoc-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        for d in ["docs", "project/tests", ".jazyk/ds_main/gen"] {
            crate::vfs::create_dir_all(dir.join(d)).unwrap();
        }
        crate::vfs::write({ let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path }, "version = 1\ngen = {}\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"**/*.md\"]\nroots = []\nsource = [\"project/**\"]\n").unwrap();
        crate::vfs::write(dir.join("docs/a.md"), "# A\n").unwrap();
        crate::vfs::write(
            dir.join("project/tests/crit_a.md"),
            "---\nrequirement: req:a-1\n---\n",
        )
        .unwrap();
        crate::vfs::write(dir.join("project/README.md"), "# Readme\n").unwrap();
        crate::vfs::write(
            dir.join(".jazyk/ds_main/gen/ledger.yaml"),
            "requirements:\n  req:a-1:\n    entity: ent:a\n    test: {kind: llm, label: judged, artifact: project/tests/crit_a.md, name: crit_a, run: \"\"}\n    hashes: {requirement: x, test: y, files: z}\n",
        )
        .unwrap();
        let p = Project::load(&dir);
        assert_eq!(
            crate::gen::Ledger::load(&p.out).requirements.len(),
            1,
            "the fixture ledger parses"
        );
        let docs = p.doc_files();
        assert!(docs.contains(&dir.join("docs/a.md")), "{:?}", docs);
        assert!(
            docs.contains(&dir.join("project/README.md")),
            "an unrecorded deliverable file follows the glob: {:?}",
            docs
        );
        assert!(
            !docs.contains(&dir.join("project/tests/crit_a.md")),
            "{:?}",
            docs
        );
        crate::vfs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn generated_dirs_are_never_doc_input() {
        let dir =
            std::env::temp_dir().join(format!("jazyk-outdir-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        for d in [
            "docs",
            ".jazyk/docsgen",
            ".jazyk-backup-local/docsgen",
            "jazyk-out/docsgen",
            "elsewhere",
        ] {
            crate::vfs::create_dir_all(dir.join(d)).unwrap();
        }
        crate::vfs::write({ let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path }, "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"**/*.md\"]\nroots = []\nsource = []\n").unwrap();
        crate::vfs::write(dir.join("docs/a.md"), "# A\n").unwrap();
        crate::vfs::write(dir.join(".jazyk/docsgen/x.md"), "# X\n").unwrap();
        crate::vfs::write(dir.join(".jazyk-backup-local/docsgen/y.md"), "# Y\n").unwrap();
        // The out directory's old name is left where it is and never compiled.
        crate::vfs::write(dir.join("jazyk-out/docsgen/w.md"), "# W\n").unwrap();
        crate::vfs::write(dir.join("elsewhere/z.md"), "# Z\n").unwrap();
        let mut p = Project::load(&dir);
        assert_eq!(
            p.doc_files(),
            vec![dir.join("docs/a.md"), dir.join("elsewhere/z.md")]
        );
        // A relocated out directory (--out) is skipped by path, whatever its name.
        p.out = dir.join("elsewhere");
        assert_eq!(p.doc_files(), vec![dir.join("docs/a.md")]);
        crate::vfs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn opening_an_unconfigured_folder_has_no_implicit_documents() {
        let dir = std::env::temp_dir().join(format!("jazyk-bare-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(dir.join("docs/main.md"), "# Main").unwrap();
        crate::vfs::write(dir.join("jazyk.toml"), "redirect = \"docs\"").unwrap();
        let p = Project::load(&dir);
        assert!(p.doc_files().is_empty());
        assert!(!crate::gen::GenSettings::resolve(&p).configured);
        assert!(!dir.join(".jazyk").vexists());
        assert!(find_root(&dir).is_none());
        crate::vfs::remove_dir_all(&dir).ok();
    }
}

pub fn find_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|p| p.join(crate::document_sets::CONFIG).vis_file())
        .map(Path::to_path_buf)
}

impl Project {
    pub fn load(root: &Path) -> Project {
        match crate::document_sets::Workspace::load(root) {
            Ok(workspace) => workspace
                .resolve(None)
                .unwrap_or_else(|_| workspace.files_project()),
            Err((_, error)) => Project {
                root: root.to_path_buf(),
                out: root.join(".jazyk/project"),
                docs_glob: vec![],
                config_error: Some(error),
                ..Project::default()
            },
        }
    }

    pub fn load_set(root: &Path, selector: &str) -> Result<Project, (u16, String)> {
        crate::document_sets::Workspace::load(root)?.resolve(Some(selector))
    }

    pub(crate) fn from_settings(root: &Path, settings: &toml::Value) -> Project {
        let mut p = Project {
            root: root.to_path_buf(),
            out: root.join(".jazyk/project"),
            ..Project::default()
        };
        let t = Toml::from_value(settings);
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
        p.llm.api = t.string("llm.api");
        p.llm.reasoning_summary = t.string("llm.reasoning_summary");
        p.llm.model = t.string("llm.model");
        p.llm.api_key = t.string("llm.api_key");
        p.llm.api_key_env = t.string("llm.api_key_env");
        p.llm.temperature = t
            .string("llm.temperature")
            .and_then(|s| s.parse::<f64>().ok());
        p.acp = parse_acp(&t);
        p.executors = parse_executors(&t);
        if let Some(v) = t.array("linting.rules.warnings") {
            p.linting.warnings = v;
        }
        if let Some(v) = t.array("linting.rules.errors") {
            p.linting.errors = v;
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
        if let Some(sets) = &self.set_docs {
            return crate::document_sets::files_under(&self.root)
                .into_iter()
                .filter(|f| !f.split('/').any(|c| c.starts_with('.')))
                .filter(|f| sets.iter().any(|docs| crate::document_sets::included(docs, f)))
                .map(|f| self.root.join(f))
                .collect();
        }
        let mut patterns: Vec<String> = Vec::new();
        if let Some(rel) = self.deliverable_rel() {
            patterns.push(if rel.is_empty() {
                "!**".to_string()
            } else {
                format!("!{}/**", rel)
            });
        }
        patterns.extend(self.docs_glob.iter().cloned());
        let recorded = self.recorded_test_artifacts();
        let mut all = Vec::new();
        collect_files(&self.root, &self.out, &mut all);
        let mut out: Vec<PathBuf> = Vec::new();
        for f in all {
            let rel = match f.strip_prefix(&self.root) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            // Project scaffolding is visible to Source inventories, but hidden
            // components remain outside document discovery.
            if rel.split('/').any(|c| c.starts_with('.')) {
                continue;
            }
            let mut included = false;
            for pat in &patterns {
                let (neg, p) = match pat.strip_prefix('!') {
                    Some(rest) => (true, rest),
                    None => (false, pat.as_str()),
                };
                if crate::document_sets::pattern_reaches(p, &rel) && glob_match(p, &rel) {
                    included = !neg;
                }
            }
            if included && !recorded.contains(&rel) {
                out.push(f);
            }
        }
        out.sort();
        out.dedup();
        out
    }

    // The deliverable files the ledger records as test artifacts, project-relative: an
    // llm row's criteria file under the deliverable is generation metadata, never doc
    // input. Mirrors docs/compiler/project-settings.md#docs.
    fn recorded_test_artifacts(&self) -> std::collections::BTreeSet<String> {
        let mut out = std::collections::BTreeSet::new();
        let Some(rel) = self.deliverable_rel() else {
            return out;
        };
        let ledger = crate::gen::Ledger::load(&self.out);
        let mut add = |t: &crate::gen::TestRef| {
            if t.kind == "llm" && !t.artifact.is_empty() {
                let a = t.artifact.trim_start_matches("./");
                out.insert(if rel.is_empty() {
                    a.to_string()
                } else {
                    format!("{}/{}", rel, a)
                });
            }
        };
        for row in ledger.requirements.values() {
            add(&row.test);
            for c in row.tests.values() {
                add(&c.test);
            }
        }
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
    let Ok(workspace) = crate::document_sets::Workspace::load(dir) else {
        return;
    };
    out.extend(
        workspace
            .files()
            .into_iter()
            .map(|p| dir.join(p))
            .filter(|p| !p.starts_with(out_dir)),
    );
}

// Glob matcher supporting `**` (any number of path segments), `*` (within a segment),
// and `?` (one non-slash char). Patterns and paths use `/` separators.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map(|g| g.compile_matcher().is_match(path))
        .unwrap_or(false)
}

// Flatten crate-parsed settings for the existing per-field resolution helpers.
struct Toml {
    strings: BTreeMap<String, String>,
    arrays: BTreeMap<String, Vec<String>>,
}
impl Toml {
    fn string(&self, key: &str) -> Option<String> {
        self.strings.get(key).cloned()
    }
    fn array(&self, key: &str) -> Option<Vec<String>> {
        self.arrays.get(key).cloned()
    }
    fn parse(text: &str) -> Self {
        Self::from_value(
            &text
                .parse::<toml::Value>()
                .unwrap_or_else(|_| toml::Value::Table(Default::default())),
        )
    }
    fn from_value(value: &toml::Value) -> Self {
        fn flatten(value: &toml::Value, prefix: &str, out: &mut Toml) {
            match value {
                toml::Value::Table(table) => {
                    for (k, v) in table {
                        flatten(
                            v,
                            &if prefix.is_empty() {
                                k.clone()
                            } else {
                                format!("{}.{}", prefix, k)
                            },
                            out,
                        );
                    }
                }
                toml::Value::Array(a) => {
                    out.arrays.insert(
                        prefix.into(),
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect(),
                    );
                }
                toml::Value::String(s) => {
                    out.strings.insert(prefix.into(), s.clone());
                }
                v => {
                    out.strings.insert(prefix.into(), v.to_string());
                }
            }
        }
        let mut out = Self {
            strings: BTreeMap::new(),
            arrays: BTreeMap::new(),
        };
        flatten(value, "", &mut out);
        out
    }
}

#[cfg(test)]
mod discovery_tests {
    use super::*;

    #[test]
    fn closest_project_configuration_wins() {
        let dir = std::env::temp_dir().join(format!(
            "jazyk-redirect-boundary-{}",
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("inner")).unwrap();
        crate::vfs::create_dir_all(dir.join("inner/deep")).unwrap();
        crate::vfs::create_dir_all(dir.join("side")).unwrap();
        crate::vfs::write(
            {
                let path = dir.join(".jazyk/jazyk.toml");
                crate::vfs::create_dir_all(path.parent().unwrap()).unwrap();
                path
            },
            crate::document_sets::EMPTY_CONFIG,
        )
        .unwrap();
        crate::vfs::write(
            { let path = dir.join("inner/.jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();

        // Starting at the redirecting directory follows it (via Project::load).
        assert_eq!(find_root(&dir), Some(dir.clone()));
        // Starting inside the target finds the target's own file.
        assert_eq!(find_root(&dir.join("inner/deep")), Some(dir.join("inner")));
        // A sibling subdirectory is its own place: the redirect above is a boundary.
        assert_eq!(find_root(&dir.join("side")), Some(dir.clone()));

        crate::vfs::remove_dir_all(&dir).ok();
    }
}

// ---- settings io for the GUI ----
// Read jazyk.toml into a form-friendly shape, and write it back in canonical form.
// Mirrors docs/frontends/gui.md#api (settings) and docs/compiler/project-settings.md.

pub fn settings_read(root: &Path) -> serde_json::Value {
    use serde_json::json;
    let text =
        crate::vfs::read_to_string(root.join(crate::document_sets::CONFIG)).unwrap_or_default();
    let t = Toml::parse(&text);
    let workspace = crate::document_sets::Workspace::load(root);
    json!({
        "exists": root.join(crate::document_sets::CONFIG).vexists(),
        "text": redact_api_key(&text),
        "hash": crate::model::hash_hex(&text), "unknown": [],
        "error": workspace.as_ref().err().map(|e| &e.1),
        "sets": workspace.as_ref().map(|w| &w.sets).ok(),
        "settings": {
            "llm": { "api": t.string("llm.api"), "reasoningSummary": t.string("llm.reasoning_summary"), "baseUrl": t.string("llm.base_url"), "model": t.string("llm.model"),
                "apiKeyEnv": t.string("llm.api_key_env"), "apiKeySet": t.string("llm.api_key").is_some(),
                "temperature": t.string("llm.temperature").and_then(|s| s.parse::<f64>().ok()) },
            "linting": { "warnings": t.array("linting.rules.warnings").unwrap_or_default(),
                "errors": t.array("linting.rules.errors").unwrap_or_default() },
            "executors": parse_executors(&t).by_kind,
        },
        "executorKeys": EXECUTOR_KEYS, "unknownExecutors": parse_executors(&t).unknown_keys(),
        "defaults": { "docsGlob": [], "deliverable": "." }
    })
}

pub fn settings_write(
    root: &Path,
    body: &serde_json::Value,
) -> Result<serde_json::Value, (u16, String)> {
    let base = body["baseHash"]
        .as_str()
        .ok_or_else(|| (400, "baseHash is required".into()))?;
    // The text mode writes the file as given, the stored API key kept.
    if let Some(given) = body["text"].as_str() {
        let current = crate::vfs::read_to_string(root.join(crate::document_sets::CONFIG)).unwrap_or_default();
        let text = restore_api_key(given, &current).map_err(|e| (400, e))?;
        crate::document_sets::atomic_config(root, &text, base)?;
        return Ok(settings_read(root));
    }
    let text = settings_render(root, &body["settings"]).map_err(|e| (400, e))?;
    crate::document_sets::atomic_config(root, &text, base)?;
    Ok(settings_read(root))
}

// The settings file as the text mode shows it: a stored `llm.api_key` reads as
// `"(stored)"`, so the key never reaches the client (docs/frontends/gui.md#api).
const STORED_KEY: &str = "(stored)";

fn redact_api_key(text: &str) -> String {
    let Ok(mut document) = text.parse::<toml_edit::DocumentMut>() else { return text.to_string() };
    let Some(llm) = document.get_mut("llm").and_then(|i| i.as_table_like_mut()) else { return text.to_string() };
    match llm.get_mut("api_key") {
        Some(item) if item.is_value() => {
            let decor = item.as_value().map(|v| v.decor().clone());
            *item = toml_edit::value(STORED_KEY);
            if let (Some(decor), Some(v)) = (decor, item.as_value_mut()) {
                *v.decor_mut() = decor;
            }
            document.to_string()
        }
        _ => text.to_string(),
    }
}

// The text mode's write: `"(stored)"` keeps the key the file holds now.
fn restore_api_key(given: &str, current: &str) -> Result<String, String> {
    let mut document: toml_edit::DocumentMut = given.parse().map_err(|e| format!("{e}"))?;
    let stored = document.get("llm").and_then(|i| i.get("api_key")).and_then(|v| v.as_str()) == Some(STORED_KEY);
    if !stored {
        return Ok(given.to_string());
    }
    let original = current
        .parse::<toml_edit::DocumentMut>()
        .ok()
        .and_then(|d| d.get("llm").and_then(|i| i.get("api_key")).and_then(|v| v.as_str()).map(str::to_string));
    let llm = document.get_mut("llm").and_then(|i| i.as_table_like_mut()).expect("llm holds api_key");
    match original {
        Some(key) => {
            let item = llm.get_mut("api_key").expect("api_key present");
            let decor = item.as_value().map(|v| v.decor().clone());
            *item = toml_edit::value(key);
            if let (Some(decor), Some(v)) = (decor, item.as_value_mut()) {
                *v.decor_mut() = decor;
            }
        }
        None => {
            llm.remove("api_key");
        }
    }
    Ok(document.to_string())
}

pub fn settings_render(root: &Path, settings: &serde_json::Value) -> Result<String, String> {
    let text = crate::vfs::read_to_string(root.join(crate::document_sets::CONFIG))
        .unwrap_or_else(|_| crate::document_sets::EMPTY_CONFIG.into());
    let mut document: toml_edit::DocumentMut = text.parse().map_err(|e| format!("{}", e))?;
    for (key, field) in [
        ("base_url", "baseUrl"),
        ("api", "api"),
        ("reasoning_summary", "reasoningSummary"),
        ("model", "model"),
        ("api_key_env", "apiKeyEnv"),
        ("temperature", "temperature"),
    ] {
        if let Some(value) = settings["llm"].get(field) {
            if value.is_null() || value.as_str() == Some("") {
                if let Some(table) = document.get_mut("llm").and_then(|i| i.as_table_mut()) {
                    table.remove(key);
                }
            } else {
                document["llm"][key] = if key == "temperature" {
                    toml_edit::value(value.as_f64().ok_or("temperature must be a number")?)
                } else {
                    toml_edit::value(value.as_str().ok_or("LLM setting must be text")?)
                };
            }
        }
    }
    for field in ["warnings", "errors"] {
        if let Some(value) = settings["linting"].get(field) {
            let items: Vec<String> =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            document["linting"]["rules"][field] =
                toml_edit::value(items.into_iter().collect::<toml_edit::Array>());
        }
    }
    if let Some(executors) = settings["executors"].as_object() {
        for (key, value) in executors {
            if !EXECUTOR_KEYS.contains(&key.as_str()) {
                return Err(format!("unknown executor: {}", key));
            }
            document["executors"][key] =
                toml_edit::value(value.as_str().ok_or("executor must be text")?);
        }
    }
    Ok(document.to_string())
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn settings_write_creates_the_file_and_refuses_a_stale_hash() {
        let dir =
            std::env::temp_dir().join(format!("jazyk-settings-write-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(&dir).unwrap();
        // A folder with no jazyk.toml: the wizard's case.
        let before = settings_read(&dir);
        assert_eq!(before["exists"], false);
        let body = serde_json::json!({
            "baseHash": before["hash"],
            "settings": {"docsGlob": ["docs/**/*.md"], "roots": [], "deliverable": null,
                "llm": {"baseUrl": null, "model": null, "apiKeyEnv": null, "temperature": null},
                "linting": {"warnings": [], "errors": []}, "executors": {}}
        });
        let after = settings_write(&dir, &body).expect("written");
        assert_eq!(after["exists"], true);
        assert!(
            crate::vfs::read_to_string(dir.join(crate::document_sets::CONFIG))
                .unwrap()
                .contains("version = 1")
        );
        // The file moved since the read: the write is refused with a conflict.
        let (code, msg) = settings_write(&dir, &body).unwrap_err();
        assert_eq!(code, 409);
        assert!(msg.contains("changed"), "{}", msg);
        crate::vfs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn settings_roundtrip_preserves_secret_and_unknown_fields() {
        let dir =
            std::env::temp_dir().join(format!("jazyk-settings-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(&dir).unwrap();
        crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\nllm = {\"model\" = \"m1\", \"api_key\" = \"sekret\"}\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"**/*.md\"]\nroots = []\nsource = []\n",
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
                "executors": { "gc": "claude", "reconcile-section": "embedded" },
            }),
        )
        .unwrap();
        assert!(text.contains("docs = [\"**/*.md\"]"));
        assert_eq!(
            text.parse::<toml::Value>().unwrap()["llm"]["api_key"].as_str(),
            Some("sekret")
        );
        assert_eq!(
            text.parse::<toml::Value>().unwrap()["llm"]["model"].as_str(),
            Some("m2")
        );
        assert!(text.contains("gc = \"claude\""), "{}", text);
        crate::vfs::write(
            {
                let path = dir.join(".jazyk/jazyk.toml");
                crate::vfs::create_dir_all(path.parent().unwrap()).unwrap();
                path
            },
            &text,
        )
        .unwrap();
        let p = Project::load(&dir);
        assert_eq!(p.llm.model.as_deref(), Some("m2"));
        assert_eq!(
            p.executors.resolve("reconcile-section", "compile"),
            Some("embedded")
        );
        assert_eq!(p.executors.resolve("split-view", "gc"), Some("claude"));
        assert_eq!(p.executors.resolve("retrace", "compile"), None);
        let read3 = settings_read(&dir);
        assert_eq!(read3["settings"]["executors"]["gc"], "claude");
        assert!(read3["unknown"].as_array().unwrap().is_empty());
        // A key outside the goal kinds and classes is a settings error.
        let err = settings_render(&dir, &serde_json::json!({"executors": {"chat": "claude"}}))
            .unwrap_err();
        assert!(err.contains("chat"), "{}", err);
        // Unknown keys are surfaced so the form refuses instead of dropping them.
        crate::vfs::write(
            {
                let path = dir.join(".jazyk/jazyk.toml");
                crate::vfs::create_dir_all(path.parent().unwrap()).unwrap();
                path
            },
            "[custom]\nthing = \"x\"\n",
        )
        .unwrap();
        let read2 = settings_read(&dir);
        assert!(read2["error"].is_string());
        assert!(settings_render(&dir, &serde_json::json!({}))
            .unwrap()
            .contains("[custom]"));
        crate::vfs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod toml_array_tests {
    use super::*;

    #[test]
    fn quoted_array_items_keep_their_commas() {
        let t = Toml::parse(
            "[linting.rules]\nwarnings = [\"An em dash appears in prose. Use commas, periods, or colons instead.\", \"Second rule\"]\n",
        );
        let rules = t.array("linting.rules.warnings").unwrap();
        assert_eq!(rules.len(), 2);
        assert!(rules[0].contains("commas, periods, or colons"));
        assert_eq!(rules[1], "Second rule");
    }
}

#[cfg(test)]
mod settings_text_tests {
    use super::*;

    #[test]
    fn the_text_mode_never_shows_the_api_key_and_keeps_it_on_write() {
        let root = std::env::temp_dir().join(format!("jz-settings-text-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".jazyk")).unwrap();
        struct Dir(std::path::PathBuf);
        impl Dir { fn path(&self) -> &Path { &self.0 } }
        impl Drop for Dir { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
        let dir = Dir(root);
        let file = "version = 1\n\n[llm]\nmodel = \"m\" # the model\napi_key = \"sk-secret\"\n";
        std::fs::write(dir.path().join(".jazyk/jazyk.toml"), file).unwrap();
        let read = settings_read(dir.path());
        let text = read["text"].as_str().unwrap();
        assert!(!text.contains("sk-secret"));
        assert!(text.contains("api_key = \"(stored)\""));
        assert!(text.contains("# the model"));
        let edited = text.replace("\"m\"", "\"other\"");
        settings_write(dir.path(), &serde_json::json!({"baseHash": read["hash"], "text": edited})).unwrap();
        let written = std::fs::read_to_string(dir.path().join(".jazyk/jazyk.toml")).unwrap();
        assert!(written.contains("model = \"other\" # the model"));
        assert!(written.contains("api_key = \"sk-secret\""));
        assert_eq!(settings_write(dir.path(), &serde_json::json!({"baseHash": read["hash"], "text": edited})).unwrap_err().0, 409);
        let fresh = settings_read(dir.path());
        assert_eq!(settings_write(dir.path(), &serde_json::json!({"baseHash": fresh["hash"], "text": "version = [unclosed"})).unwrap_err().0, 400);
    }
}
