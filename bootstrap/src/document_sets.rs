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
pub const IGNORE: &str = "*\n!jazyk.toml\n!.gitignore\n";
pub type Result<T> = std::result::Result<T, (u16, String)>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentSet {
    pub id: String,
    pub name: String,
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
    settings: toml::Table,
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

    fn parse(root: &Path, text: &str, exists: bool) -> Result<Self> {
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
            if set.docs.is_empty() || !set.docs.iter().any(|p| !p.starts_with('!')) {
                return Err((
                    400,
                    format!("{} needs at least one document selection", set.name),
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
        let removed: Vec<Value> = crate::vfs::read_dir(self.root.join(".jazyk/removed-sets"))
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
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
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

pub fn valid_id(id: &str) -> bool {
    id.starts_with("ds_")
        && id.len() > 3
        && id.len() < 100
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
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

pub fn eligible(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '\0', ':'])
        && !path.split('/').any(|c| {
            c.is_empty()
                || c.starts_with('.')
                || c == "target"
                || c == "node_modules"
                || c.starts_with("jazyk-out")
        })
}

pub fn matched<'a>(patterns: &'a [String], path: &str) -> Option<&'a str> {
    patterns
        .iter()
        .rev()
        .find(|p| crate::project::glob_match(p.strip_prefix('!').unwrap_or(p), path))
        .filter(|p| !p.starts_with('!'))
        .map(String::as_str)
}
pub fn included(patterns: &[String], path: &str) -> bool {
    matched(patterns, path).is_some()
}

// Every eligible file under the project, project-relative, sorted. A full walk with
// a canonicalize per entry: call it when the files are needed, never per request.
pub fn files_under(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    walk(root, root, &mut BTreeSet::new(), &mut files);
    files.sort();
    files.dedup();
    files
}

fn walk(root: &Path, dir: &Path, seen: &mut BTreeSet<PathBuf>, files: &mut Vec<String>) {
    let canonical_root = root.vcanonicalize().unwrap_or_else(|_| root.to_path_buf());
    let Ok(canonical) = dir.vcanonicalize() else {
        return;
    };
    if !canonical.starts_with(&canonical_root) || !seen.insert(canonical) {
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
            walk(root, &path, seen, files);
        } else if path
            .vcanonicalize()
            .is_ok_and(|p| p.starts_with(&canonical_root))
        {
            files.push(relative);
        }
    }
}

pub fn mint_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("system randomness for a document set ID");
    format!(
        "ds_{}",
        bytes
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>()
    )
}

/// A short filesystem lock protects optimistic config replacement. Hosted callers
/// already hold their project lease; WASM requests are serialized by their worker.
pub struct ProjectLock {
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
                crate::vfs::create_dir_all(root.join(".jazyk"))
                    .map_err(|e| (500, e.to_string()))?;
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(root.join(".jazyk").join(name))
                    .map_err(|e| (500, e.to_string()))?;
                fs2::FileExt::try_lock_exclusive(&file)
                    .map_err(|_| (409, "another project operation is in progress".into()))?;
                return Ok(Self { _file: Some(file) });
            }
            Ok(Self { _file: None })
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (root, name);
            Ok(Self {})
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
    let ignore = root.join(".jazyk/.gitignore");
    if !ignore.vexists() {
        crate::vfs::write(ignore, IGNORE).map_err(|e| (500, e.to_string()))?;
    }
    Ok(())
}

/// Create/update/remove one set, or preview the exact same edit without writing.
pub fn edit(root: &Path, body: &Value, preview: bool) -> Result<Value> {
    let before = Workspace::load(root)?;
    if body["baseHash"].as_str() != Some(before.hash.as_str()) {
        return Err((409, "configuration changed; reload before saving".into()));
    }
    if let Some(id) = body["purge"].as_str() {
        if !valid_id(id) || before.sets.iter().any(|s| s.id == id) {
            return Err((400, "only a removed set can be purged".into()));
        }
        let saved = root
            .join(".jazyk/removed-sets")
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
            root.join(".jazyk/removed-sets")
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
        .unwrap_or_else(mint_id);
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
            .join(".jazyk/removed-sets")
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
                    toml_edit::value(
                        value
                            .as_str()
                            .ok_or_else(|| (400, "name must be text".into()))?
                            .trim(),
                    )
                } else {
                    let values: Vec<String> = serde_json::from_value(value.clone())
                        .map_err(|_| (400, format!("{} must be a list of patterns", field)))?;
                    toml_edit::value(values.into_iter().collect::<toml_edit::Array>())
                };
            }
        }
    }
    let next = document.to_string();
    let after = Workspace::parse(root, &next, true)?;
    let mut value = after.summary();
    value["setId"] = json!(id);
    value["baseHash"] = json!(before.hash);
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
            "removed": files.iter().filter(|f| !included(&set.docs, f) && old.is_some_and(|o| included(&o.docs, f))).collect::<Vec<_>>()})
    }).collect::<Vec<_>>());
    if !preview {
        let _build = ProjectLock::acquire(root, "project.lock")?;
        if removal.is_some() {
            let set = before.select(&id)?;
            let folder = root.join(".jazyk/removed-sets");
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
                root.join(".jazyk/removed-sets")
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
    let Ok(base) = root.vcanonicalize() else {
        return false;
    };
    let path = root.join(relative);
    for ancestor in path.ancestors().take_while(|p| *p != root) {
        if ancestor.vis_dir() && ancestor.join(CONFIG).vexists() {
            return false;
        }
        if ancestor.vexists() && !ancestor.vcanonicalize().is_ok_and(|p| p.starts_with(&base)) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let text = crate::vfs::read_to_string(path).unwrap();
        assert!(text.contains("# Owner comment"));
        assert!(text.contains("note = 'keep me'"));
        assert_eq!(
            Workspace::load(&f.0).unwrap().select("B").unwrap().docs,
            vec!["docs/b.md"]
        );
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
        assert!(f.0.join(".jazyk/removed-sets/ds_a.toml").vexists());
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
