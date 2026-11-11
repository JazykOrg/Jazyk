//! `jazyk merge-file`: the git merge driver for committed set files. It merges one YAML
//! file field by field instead of line by line, so two places that changed different
//! fields of one node meet without a conflict. Mirrors
//! docs/compiler/jazyk-folder.md#merging-through-git.
use serde_norway::{Mapping, Value};
use std::path::{Path, PathBuf};

// Lists whose order carries no meaning: a merge takes the union.
const SET_LISTS: [&str; 3] = ["aliases", "entities", "members"];

/// What one merge produced.
#[derive(Debug, PartialEq)]
pub struct Merged {
    /// The merged file's text, the current branch's header comments first.
    pub text: String,
    /// The fields both sides changed differently, kept at the current branch's value,
    /// as dotted paths.
    pub conflicts: Vec<String>,
    /// The source sections (`doc#section`) of a node with conflicting fields, which the
    /// next build reconciles again.
    pub dirty_sections: Vec<String>,
}

/// Merge three versions of one committed set file. `path` is the file's path in the
/// repository, which decides the ledger and node rules. An error names a version that
/// does not parse.
pub fn merge(base: &str, ours: &str, theirs: &str, path: &str) -> Result<Merged, String> {
    let parse = |name: &str, text: &str| -> Result<Value, String> {
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_norway::from_str(text).map_err(|e| format!("{} does not parse: {}", name, e))
    };
    let b = parse("base", base)?;
    let o = parse("ours", ours)?;
    let t = parse("theirs", theirs)?;
    let mut conflicts = Vec::new();
    let value = if path.contains("/gen/requirements/") {
        merge_ledger_row(&b, &o, &t)
    } else {
        merge_value(Some(&b), Some(&o), Some(&t), "", &mut conflicts).unwrap_or(Value::Null)
    };
    let dirty_sections = if conflicts.is_empty() {
        Vec::new()
    } else if let Some(doc) = source_doc(path) {
        // A source's sections both sides changed differently: each reconciles again.
        let mut out: Vec<String> = Vec::new();
        for c in &conflicts {
            if let Some(rest) = c.strip_prefix("sections.") {
                let section = rest.split('.').next().unwrap_or(rest);
                let at = format!("{}#{}", doc, section);
                if !out.contains(&at) {
                    out.push(at);
                }
            }
        }
        out
    } else if is_node(path) {
        source_sections(&value)
    } else {
        Vec::new()
    };
    let header: String = ours
        .lines()
        .take_while(|l| l.starts_with('#'))
        .map(|l| format!("{}\n", l))
        .collect();
    let body = if value.is_null() {
        String::new()
    } else {
        serde_norway::to_string(&value).map_err(|e| e.to_string())?
    };
    Ok(Merged {
        text: format!("{}{}", header, body),
        conflicts,
        dirty_sections,
    })
}

// The document a `sources/<doc path>.yaml` file mirrors.
fn source_doc(path: &str) -> Option<String> {
    let (_, rest) = path.split_once("/sources/")?;
    rest.strip_suffix(".yaml").map(String::from)
}

// Whether a path names a graph node file, whose conflicting fields re-open its sources.
fn is_node(path: &str) -> bool {
    [
        "/graph/requirements/",
        "/graph/entities/",
        "/graph/criteria/",
    ]
    .iter()
    .any(|k| path.contains(k))
}

// The three-way merge of one value; `None` is absent. Returns the merged value, or
// `None` for a deletion.
fn merge_value(
    base: Option<&Value>,
    ours: Option<&Value>,
    theirs: Option<&Value>,
    at: &str,
    conflicts: &mut Vec<String>,
) -> Option<Value> {
    if ours == theirs {
        return ours.cloned();
    }
    if base == ours {
        return theirs.cloned();
    }
    if base == theirs {
        return ours.cloned();
    }
    let key = at.rsplit('.').next().unwrap_or(at);
    match (ours, theirs) {
        (Some(Value::Mapping(o)), Some(Value::Mapping(t))) => {
            let b = match base {
                Some(Value::Mapping(b)) => Some(b),
                _ => None,
            };
            Some(Value::Mapping(merge_mapping(b, o, t, at, conflicts)))
        }
        (Some(Value::Sequence(o)), Some(Value::Sequence(t))) if SET_LISTS.contains(&key) => {
            let b: Vec<Value> = match base {
                Some(Value::Sequence(b)) => b.clone(),
                _ => Vec::new(),
            };
            let removed = |v: &Value| b.contains(v) && (!o.contains(v) || !t.contains(v));
            let mut out: Vec<Value> = Vec::new();
            for v in o.iter().chain(t.iter()) {
                if !out.contains(v) && !removed(v) {
                    out.push(v.clone());
                }
            }
            Some(Value::Sequence(out))
        }
        _ => {
            conflicts.push(if at.is_empty() {
                "(file)".to_string()
            } else {
                at.to_string()
            });
            ours.cloned()
        }
    }
}

fn merge_mapping(
    base: Option<&Mapping>,
    ours: &Mapping,
    theirs: &Mapping,
    at: &str,
    conflicts: &mut Vec<String>,
) -> Mapping {
    let mut keys: Vec<&Value> = ours.keys().collect();
    for k in theirs.keys() {
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    let mut out = Mapping::new();
    for k in keys {
        let name = k
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| format!("{:?}", k));
        let path = if at.is_empty() {
            name
        } else {
            format!("{}.{}", at, name)
        };
        let merged = merge_value(
            base.and_then(|b| b.get(k)),
            ours.get(k),
            theirs.get(k),
            &path,
            conflicts,
        );
        if let Some(v) = merged {
            out.insert(k.clone(), v);
        }
    }
    out
}

// A ledger row: a field-level merge when the sides touched different fields, else the
// side whose `lastRun` is newer (the current branch's when neither or both say).
fn merge_ledger_row(base: &Value, ours: &Value, theirs: &Value) -> Value {
    let mut conflicts = Vec::new();
    let merged = merge_value(Some(base), Some(ours), Some(theirs), "", &mut conflicts);
    if conflicts.is_empty() {
        return merged.unwrap_or(Value::Null);
    }
    let last = |v: &Value| v.get("lastRun").and_then(Value::as_str).map(String::from);
    match (last(ours), last(theirs)) {
        (Some(o), Some(t)) if t > o => theirs.clone(),
        (None, Some(_)) => theirs.clone(),
        _ => ours.clone(),
    }
}

// The `doc#section` references a node's facts quote: a requirement's or criterion's
// `source`, an entity's `mentions`.
fn source_sections(node: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |r: &Value| {
        if let (Some(d), Some(s)) = (
            r.get("doc").and_then(Value::as_str),
            r.get("section").and_then(Value::as_str),
        ) {
            let at = format!("{}#{}", d, s);
            if !out.contains(&at) {
                out.push(at);
            }
        }
    };
    if let Some(src) = node.get("source") {
        add(src);
    }
    if let Some(Value::Sequence(ms)) = node.get("mentions") {
        for m in ms {
            add(m);
        }
    }
    out
}

/// The set directory a committed set file belongs to: the path up to the component
/// before `graph`, `sources`, or `gen`.
pub fn set_dir_of(path: &Path) -> Option<PathBuf> {
    let parts: Vec<_> = path.components().collect();
    let at = parts
        .iter()
        .position(|c| matches!(c.as_os_str().to_str(), Some("graph" | "sources" | "gen")))?;
    Some(parts[..at].iter().collect())
}

/// The file in the set's `local/` folder that lists the sections the next load treats
/// as changed: a merge cannot edit `sources/` safely, since git writes the merged tree
/// after the driver runs. Mirrors docs/compiler/jazyk-folder.md#merging-through-git.
pub fn merged_sections_file(set_dir: &Path) -> PathBuf {
    crate::layout::local_dir(set_dir).join("merged.yaml")
}

/// Add sections to the set's merged-sections file.
pub fn record_dirty_sections(set_dir: &Path, sections: &[String]) -> std::io::Result<()> {
    if sections.is_empty() {
        return Ok(());
    }
    let path = merged_sections_file(set_dir);
    let mut all: Vec<String> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_norway::from_str(&t).ok())
        .unwrap_or_default();
    for s in sections {
        if !all.contains(s) {
            all.push(s.clone());
        }
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&path, serde_norway::to_string(&all).unwrap_or_default())
}

/// `jazyk merge-file <base> <ours> <theirs> <path>`: merge, write the result over
/// `<ours>`, and exit 0; exit 1 without touching `<ours>` when a version does not
/// parse, so git leaves its own conflict.
pub fn run_cli(args: &[String]) -> i32 {
    let [base, ours, theirs, path] = args else {
        eprintln!("usage: jazyk merge-file <base> <ours> <theirs> <path>");
        return 2;
    };
    let read = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    match merge(&read(base), &read(ours), &read(theirs), path) {
        Ok(m) => {
            if let Err(e) = std::fs::write(ours, &m.text) {
                eprintln!("jazyk merge-file: cannot write {}: {}", ours, e);
                return 1;
            }
            if let Some(set) = set_dir_of(Path::new(path)) {
                if let Err(e) = record_dirty_sections(&set, &m.dirty_sections) {
                    eprintln!(
                        "jazyk merge-file: cannot record sections to reconcile: {}",
                        e
                    );
                }
            }
            if !m.conflicts.is_empty() {
                eprintln!(
                    "jazyk merge-file: {}: kept this branch's {}; the next build reconciles {}",
                    path,
                    m.conflicts.join(", "),
                    if m.dirty_sections.is_empty() {
                        "nothing".to_string()
                    } else {
                        m.dirty_sections.join(", ")
                    }
                );
            }
            0
        }
        Err(e) => {
            eprintln!("jazyk merge-file: {}: {}", path, e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "# Jazyk entity: one concept your docs name. Written by jazyk: edit the docs, not this file.\n# yaml-language-server: $schema=https://jazyk.org/schemas/entity.json\n";

    #[test]
    fn fields_changed_on_one_side_each_take_that_side() {
        let base = format!("{HEADER}id: ent:order\nname: Order\ndefinition: An order.\n");
        let ours = format!("{HEADER}id: ent:order\nname: Purchase order\ndefinition: An order.\n");
        let theirs = format!(
            "{HEADER}id: ent:order\nname: Order\ndefinition: A placed order.\nstereotype: record\n"
        );
        let m = merge(
            &base,
            &ours,
            &theirs,
            ".jazyk/s/graph/entities/ent~order.yaml",
        )
        .unwrap();
        assert!(m.conflicts.is_empty(), "{:?}", m.conflicts);
        assert!(m.text.starts_with(HEADER), "{}", m.text);
        let v: Value = serde_norway::from_str(&m.text).unwrap();
        assert_eq!(v["name"].as_str(), Some("Purchase order"));
        assert_eq!(v["definition"].as_str(), Some("A placed order."));
        assert_eq!(v["stereotype"].as_str(), Some("record"));
        assert!(m.dirty_sections.is_empty());
    }

    #[test]
    fn set_lists_union_and_respect_removals() {
        let base = "id: ent:a\naliases: [x, y]\n";
        let ours = "id: ent:a\naliases: [x, z]\n";
        let theirs = "id: ent:a\naliases: [x, y, w]\n";
        let m = merge(base, ours, theirs, ".jazyk/s/graph/entities/ent~a.yaml").unwrap();
        let v: Value = serde_norway::from_str(&m.text).unwrap();
        let got: Vec<&str> = v["aliases"]
            .as_sequence()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(got, vec!["x", "z", "w"], "y was removed on our side");
        assert!(m.conflicts.is_empty());
    }

    #[test]
    fn a_field_both_sides_changed_keeps_ours_and_reopens_its_source() {
        let base = "id: req:a-1\nstatement: The cart holds items.\nsource: {doc: docs/a.md, section: /a, quote: The cart holds items.}\n";
        let ours = base.replace(
            "statement: The cart holds items.",
            "statement: The cart holds goods.",
        );
        let theirs = base.replace(
            "statement: The cart holds items.",
            "statement: The cart keeps items.",
        );
        let m = merge(
            base,
            &ours,
            &theirs,
            ".jazyk/s/graph/requirements/req~a-1.yaml",
        )
        .unwrap();
        assert_eq!(m.conflicts, vec!["statement".to_string()]);
        let v: Value = serde_norway::from_str(&m.text).unwrap();
        assert_eq!(v["statement"].as_str(), Some("The cart holds goods."));
        assert_eq!(m.dirty_sections, vec!["docs/a.md#/a".to_string()]);
    }

    #[test]
    fn a_source_section_both_sides_changed_reconciles_again() {
        let base = "sections:\n  /a:\n    raw: A one.\n    hash: h1\n  /b:\n    raw: B one.\n    hash: h2\n";
        let ours = base.replace("A one.", "A two.").replace("h1", "h3");
        let theirs = base
            .replace("A one.", "A three.")
            .replace("h1", "h4")
            .replace("B one.", "B two.")
            .replace("h2", "h5");
        let m = merge(base, &ours, &theirs, ".jazyk/s/sources/docs/a.md.yaml").unwrap();
        let v: Value = serde_norway::from_str(&m.text).unwrap();
        assert_eq!(v["sections"]["/a"]["raw"].as_str(), Some("A two."));
        assert_eq!(v["sections"]["/b"]["raw"].as_str(), Some("B two."));
        assert_eq!(m.dirty_sections, vec!["docs/a.md#/a".to_string()]);
    }

    #[test]
    fn a_ledger_row_takes_the_newer_verdict() {
        let base = "id: req:a-1\nentity: ent:a\nverdict: none\n";
        let ours = "id: req:a-1\nentity: ent:a\nverdict: fail\nlastRun: '2026-11-01T10:00:00Z'\n";
        let theirs = "id: req:a-1\nentity: ent:a\nverdict: pass\nlastRun: '2026-11-02T10:00:00Z'\n";
        let m = merge(base, ours, theirs, ".jazyk/s/gen/requirements/req~a-1.yaml").unwrap();
        let v: Value = serde_norway::from_str(&m.text).unwrap();
        assert_eq!(v["verdict"].as_str(), Some("pass"));
        assert!(m.dirty_sections.is_empty());
    }

    #[test]
    fn an_unparseable_side_is_an_error() {
        assert!(merge(
            "a: 1\n",
            "a: [2\n",
            "a: 3\n",
            ".jazyk/s/graph/entities/e.yaml"
        )
        .is_err());
    }

    #[test]
    fn set_dir_of_finds_the_set() {
        assert_eq!(
            set_dir_of(Path::new(".jazyk/my-set-ks8mf/graph/requirements/r.yaml")),
            Some(PathBuf::from(".jazyk/my-set-ks8mf"))
        );
        assert_eq!(set_dir_of(Path::new("docs/a.md")), None);
    }

    // A real `git merge` with the driver configured merges a node changed on both
    // branches field by field. Skipped without git or a built binary.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn git_runs_the_driver_on_set_files() {
        // The jazyk binary beside this test binary (target/<profile>/jazyk), when it is
        // built and knows the command.
        let Some(exe) = std::env::current_exe()
            .ok()
            .and_then(|t| t.parent()?.parent().map(|d| d.join("jazyk")))
            .filter(|e| {
                std::process::Command::new(e)
                    .arg("merge-file")
                    .output()
                    .is_ok_and(|o| {
                        o.status.code() == Some(2)
                            && String::from_utf8_lossy(&o.stderr).contains("merge-file")
                    })
            })
        else {
            eprintln!("no current jazyk binary; the driver was not exercised");
            return;
        };
        let exe = exe.display().to_string();
        let dir =
            std::env::temp_dir().join(format!("jazyk-merge-driver-{}", crate::sys::process_id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".jazyk/s/graph/entities")).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(&dir)
                .output()
        };
        if git(&["init", "-q", "-b", "base"])
            .map(|o| !o.status.success())
            .unwrap_or(true)
        {
            eprintln!("no git binary; the driver was not exercised");
            return;
        }
        std::fs::write(
            dir.join(".jazyk/.gitattributes"),
            crate::document_sets::ATTRIBUTES,
        )
        .unwrap();
        let driver = format!("{} merge-file %O %A %B %P", exe);
        assert!(git(&["config", "merge.jazyk.driver", &driver])
            .unwrap()
            .status
            .success());
        let file = dir.join(".jazyk/s/graph/entities/ent~order.yaml");
        std::fs::write(&file, "id: ent:order\nname: Order\ndefinition: An order.\n").unwrap();
        let commit = |m: &str| {
            assert!(git(&["add", "-A"]).unwrap().status.success());
            assert!(git(&["commit", "-q", "-m", m]).unwrap().status.success());
        };
        commit("base");
        assert!(git(&["checkout", "-q", "-b", "a"])
            .unwrap()
            .status
            .success());
        std::fs::write(
            &file,
            "id: ent:order\nname: Purchase order\ndefinition: An order.\n",
        )
        .unwrap();
        commit("a");
        assert!(git(&["checkout", "-q", "base"]).unwrap().status.success());
        std::fs::write(
            &file,
            "id: ent:order\nname: Order\ndefinition: A placed order.\n",
        )
        .unwrap();
        commit("b");
        let merged = git(&["merge", "-q", "--no-edit", "a"]).unwrap();
        assert!(
            merged.status.success(),
            "{}{}",
            String::from_utf8_lossy(&merged.stdout),
            String::from_utf8_lossy(&merged.stderr)
        );
        let v: Value = serde_norway::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["name"].as_str(), Some("Purchase order"));
        assert_eq!(v["definition"].as_str(), Some("A placed order."));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
