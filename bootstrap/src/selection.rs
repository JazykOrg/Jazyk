//! Selected builds: the part of one document set a build runs.
//! Mirrors docs/compiler/compilation.md#selected-builds.
use crate::model::{Goal, Provenance};
use crate::project::Project;
use crate::store::Store;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// One selected path: a document, a folder, or lines of a document (one-based,
// inclusive).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<[usize; 2]>,
}

impl Item {
    pub fn label(&self) -> String {
        match self.lines {
            Some([a, b]) => format!("{}:{}-{}", self.path, a, b),
            None => self.path.clone(),
        }
    }
}

// The field of a job request: absent or null is no selection. Shape only; the paths
// are checked against a set by `validate`.
pub fn parse(value: &Value) -> Result<Option<Vec<Item>>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let items: Vec<Item> =
        serde_json::from_value(value.clone()).map_err(|e| format!("Invalid selection: {e}"))?;
    if items.is_empty() {
        return Err("Select at least one document, folder, or range.".into());
    }
    if items.len() > 1000 {
        return Err("Select at most 1000 items.".into());
    }
    let mut out: Vec<Item> = Vec::new();
    for mut item in items {
        let path = item.path.trim().trim_end_matches('/').to_string();
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path
                .split('/')
                .any(|s| s.is_empty() || s == "." || s == "..")
        {
            return Err(format!(
                "`{}` is not a project-relative path.",
                item.path.trim()
            ));
        }
        if let Some([a, b]) = item.lines {
            if a == 0 || b < a {
                return Err(format!(
                    "`{}` has an invalid line range {}-{}: lines are one-based and first <= last.",
                    path, a, b
                ));
            }
        }
        item.path = path;
        out.push(item);
    }
    Ok(Some(normalize(out)))
}

// `docs/a.md:12-40` or `docs/a.md`: the CLI spelling.
pub fn parse_arg(arg: &str) -> Result<Item, String> {
    let (path, lines) = match arg.rsplit_once(':') {
        Some((p, r)) if r.contains('-') && r.chars().all(|c| c.is_ascii_digit() || c == '-') => {
            let (a, b) = r.split_once('-').unwrap();
            let a: usize = a
                .parse()
                .map_err(|_| format!("`{arg}`: the range is <first>-<last>"))?;
            let b: usize = b
                .parse()
                .map_err(|_| format!("`{arg}`: the range is <first>-<last>"))?;
            (p, Some([a, b]))
        }
        _ => (arg, None),
    };
    let items = parse(&serde_json::json!([{"path": path, "lines": lines}]))?.unwrap();
    Ok(items.into_iter().next().unwrap())
}

// Sorted, deduplicated; a whole document or folder absorbs the ranges under it.
pub fn normalize(mut items: Vec<Item>) -> Vec<Item> {
    items.sort();
    items.dedup();
    let wholes: Vec<String> = items
        .iter()
        .filter(|i| i.lines.is_none())
        .map(|i| i.path.clone())
        .collect();
    items.retain(|i| {
        !wholes
            .iter()
            .any(|w| (w != &i.path || i.lines.is_some()) && under(&i.path, w))
    });
    items
}

fn under(path: &str, prefix: &str) -> bool {
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

// Every path names a document of the set or a folder holding one.
pub fn validate(proj: &Project, items: &[Item]) -> Result<(), String> {
    let docs: Vec<String> = proj
        .doc_files()
        .into_iter()
        .map(|f| {
            f.strip_prefix(&proj.root)
                .unwrap_or(&f)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    for item in items {
        let is_doc = docs.iter().any(|d| d == &item.path);
        if item.lines.is_some() && !is_doc {
            return Err(format!(
                "`{}` is not a document of set `{}`; a line range needs a document.",
                item.path,
                proj.set_id.as_deref().unwrap_or("main")
            ));
        }
        if !is_doc && !docs.iter().any(|d| under(d, &item.path)) {
            return Err(format!(
                "`{}` is not a document of set `{}` or a folder holding one.",
                item.path,
                proj.set_id.as_deref().unwrap_or("main")
            ));
        }
    }
    Ok(())
}

// The items with what they resolved to in a store: the documents and the number of
// sections each selected.
pub fn describe(store: &Store, items: &[Item]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|item| {
                let one = Resolved::new(store, std::slice::from_ref(item));
                let mut v = serde_json::to_value(item).unwrap();
                v["documents"] = serde_json::json!(one.docs);
                v["sections"] = serde_json::json!(one.sections.len());
                v
            })
            .collect(),
    )
}

// A selection resolved against the current store: the selected sections and the
// nodes anchored in them. Recomputed at every derivation, so nodes a session creates
// in a selected section join it.
pub struct Resolved {
    pub docs: BTreeSet<String>,
    // `<doc>#<ref>`
    pub sections: BTreeSet<String>,
    pub nodes: BTreeSet<String>,
}

impl Resolved {
    pub fn new(store: &Store, items: &[Item]) -> Resolved {
        let mut sections = BTreeSet::new();
        for (doc, rec) in &store.docs {
            for item in items {
                if !under(doc, &item.path) {
                    continue;
                }
                match item.lines {
                    None => {
                        sections.extend(rec.sections.keys().map(|r| format!("{doc}#{r}")));
                    }
                    Some([a, b]) => {
                        // Section lines are zero-based and half-open.
                        let (from, to) = (a - 1, b);
                        for (r, s) in &rec.sections {
                            let [l0, l1] = s.lines;
                            let hit = if l1 > l0 {
                                l0 < to && from < l1
                            } else {
                                l0 >= from && l0 < to
                            };
                            if hit {
                                let owner = crate::goals::owning_section(store, doc, r);
                                sections.insert(format!("{doc}#{owner}"));
                            }
                        }
                    }
                }
            }
        }
        let docs: BTreeSet<String> = sections
            .iter()
            .filter_map(|s| s.split_once('#').map(|(d, _)| d.to_string()))
            .collect();
        let in_sel = |doc: &str, sec: &str| sections.contains(&format!("{doc}#{sec}"));
        let g = &store.graph;
        let mut nodes: BTreeSet<String> = BTreeSet::new();
        for (id, r) in &g.requirements {
            if r.source
                .as_ref()
                .is_some_and(|s| in_sel(&s.doc, &s.section))
            {
                nodes.insert(id.clone());
            }
        }
        for (id, e) in &g.entities {
            if e.mentions.iter().any(|m| in_sel(&m.doc, &m.section)) {
                nodes.insert(id.clone());
            }
        }
        for (id, c) in &g.criteria {
            if nodes.contains(&c.requirement) {
                nodes.insert(id.clone());
            }
        }
        // One step of derived provenance: a node built from a selected node.
        let from_selected = |p: &Option<Provenance>| match p {
            Some(Provenance::Derived { from, .. }) => from.iter().any(|f| nodes.contains(f)),
            _ => false,
        };
        let mut derived: Vec<String> = Vec::new();
        for (id, r) in &g.requirements {
            if from_selected(&r.provenance) {
                derived.push(id.clone());
            }
        }
        for (id, e) in &g.entities {
            if from_selected(&e.provenance) {
                derived.push(id.clone());
            }
        }
        for (id, c) in &g.criteria {
            if from_selected(&c.provenance) {
                derived.push(id.clone());
            }
        }
        nodes.extend(derived);
        Resolved {
            docs,
            sections,
            nodes,
        }
    }

    pub fn holds_target(&self, target: &str) -> bool {
        if target.starts_with("scope:") {
            return false;
        }
        if let Some((a, b)) = crate::goals::pair_members(target) {
            return self.nodes.contains(a) || self.nodes.contains(b);
        }
        if self.sections.contains(target) || self.docs.contains(target) {
            return true;
        }
        self.nodes.contains(target)
    }

    pub fn holds(&self, goal: &Goal) -> bool {
        self.holds_target(&goal.target)
    }

    // The entities and requirements a ledger action targets.
    pub fn entities(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|n| n.starts_with("ent:"))
            .cloned()
            .collect()
    }
    pub fn requirements(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|n| n.starts_with("req:"))
            .cloned()
            .collect()
    }
}

// The selection a job holds: shared between the build that reads it at every
// derivation and the server that widens it. `None` items is the whole set.
#[derive(Default)]
pub struct Handle {
    items: Mutex<Option<Vec<Item>>>,
    // Set when the job leaves its first action: later requests queue instead.
    // Changed only under the `items` lock, so a widening never lands after it.
    sealed: AtomicBool,
    // Widenings not yet reported on the trace.
    widened: Mutex<Vec<Vec<Item>>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Widen {
    // The job took the items.
    Widened,
    // The job already covers them.
    Covered,
    // The job left the action a selection steers: queue a new job.
    Sealed,
}

impl Handle {
    pub fn new(items: Option<Vec<Item>>) -> Arc<Handle> {
        Arc::new(Handle {
            items: Mutex::new(items),
            ..Default::default()
        })
    }
    pub fn items(&self) -> Option<Vec<Item>> {
        self.items.lock().unwrap().clone()
    }
    pub fn seal(&self) {
        let _items = self.items.lock().unwrap();
        self.sealed.store(true, Ordering::SeqCst);
    }
    // Seal unless a widening waits to be applied: the build loop's last check
    // before it ends its sessions.
    pub fn seal_unless_widened(&self) -> bool {
        let _items = self.items.lock().unwrap();
        if !self.widened.lock().unwrap().is_empty() {
            return false;
        }
        self.sealed.store(true, Ordering::SeqCst);
        true
    }
    pub fn sealed(&self) -> bool {
        self.sealed.load(Ordering::SeqCst)
    }
    // Take more items. `None` widens to the whole set.
    pub fn widen(&self, add: Option<Vec<Item>>) -> Widen {
        let mut items = self.items.lock().unwrap();
        if self.sealed.load(Ordering::SeqCst) {
            return Widen::Sealed;
        }
        let Some(current) = items.as_mut() else {
            return Widen::Covered;
        };
        match add {
            None => {
                *items = None;
                self.widened.lock().unwrap().push(Vec::new());
                Widen::Widened
            }
            Some(add) => {
                let before = current.clone();
                let mut next = before.clone();
                next.extend(add.iter().cloned());
                let next = normalize(next);
                if next == before {
                    return Widen::Covered;
                }
                let added: Vec<Item> = add.into_iter().filter(|i| !before.contains(i)).collect();
                *current = next;
                self.widened.lock().unwrap().push(added);
                Widen::Widened
            }
        }
    }
    pub fn take_widened(&self) -> Vec<Vec<Item>> {
        std::mem::take(&mut *self.widened.lock().unwrap())
    }
}

thread_local! { static CURRENT: RefCell<Option<Arc<Handle>>> = const { RefCell::new(None) }; }

// The selection in force on this thread while the guard lives.
pub struct Guard(Option<Arc<Handle>>);
pub fn enter(handle: Arc<Handle>) -> Guard {
    Guard(CURRENT.with(|c| c.replace(Some(handle))))
}
impl Drop for Guard {
    fn drop(&mut self) {
        let prev = self.0.take();
        CURRENT.with(|c| *c.borrow_mut() = prev);
    }
}
pub fn handle() -> Option<Arc<Handle>> {
    CURRENT.with(|c| c.borrow().clone())
}
// The items in force, or `None` for the whole set.
pub fn current() -> Option<Vec<Item>> {
    handle().and_then(|h| h.items())
}
// Run `f` with `items` in force (a read such as the selected board).
pub fn with<T>(items: Vec<Item>, f: impl FnOnce() -> T) -> T {
    let _g = enter(Handle::new(Some(items)));
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes() {
        let v = serde_json::json!([
            {"path": "docs/a.md", "lines": [3, 9]},
            {"path": "docs/"},
            {"path": "other/b.md", "lines": [1, 1]},
            {"path": "other/b.md", "lines": [1, 1]}
        ]);
        let items = parse(&v).unwrap().unwrap();
        assert_eq!(
            items,
            vec![
                Item {
                    path: "docs".into(),
                    lines: None
                },
                Item {
                    path: "other/b.md".into(),
                    lines: Some([1, 1])
                },
            ]
        );
        assert!(parse(&Value::Null).unwrap().is_none());
        assert!(parse(&serde_json::json!([])).is_err());
        assert!(parse(&serde_json::json!([{"path": "../x.md"}])).is_err());
        assert!(parse(&serde_json::json!([{"path": "a.md", "lines": [0, 2]}])).is_err());
        assert!(parse(&serde_json::json!([{"path": "a.md", "lines": [5, 2]}])).is_err());
        assert!(parse(&serde_json::json!([{"path": "a.md", "extra": 1}])).is_err());
    }

    #[test]
    fn parses_cli_arguments() {
        assert_eq!(
            parse_arg("docs/a.md:12-40").unwrap(),
            Item {
                path: "docs/a.md".into(),
                lines: Some([12, 40])
            }
        );
        assert_eq!(parse_arg("docs").unwrap().lines, None);
        assert!(parse_arg("docs/a.md:9-2").is_err());
    }

    #[test]
    fn widening_merges_and_reports() {
        let doc = |p: &str| Item {
            path: p.into(),
            lines: None,
        };
        let h = Handle::new(Some(vec![Item {
            path: "docs/a.md".into(),
            lines: Some([1, 4]),
        }]));
        assert_eq!(h.widen(Some(vec![doc("docs/b.md")])), Widen::Widened);
        assert_eq!(h.widen(Some(vec![doc("docs/b.md")])), Widen::Covered);
        assert_eq!(h.items().unwrap().len(), 2);
        assert!(!h.seal_unless_widened());
        assert_eq!(h.take_widened().len(), 1);
        assert_eq!(h.widen(None), Widen::Widened);
        assert!(h.items().is_none());
        assert_eq!(h.widen(Some(vec![doc("docs/c.md")])), Widen::Covered);
        h.take_widened();
        assert!(h.seal_unless_widened());
        assert_eq!(h.widen(Some(vec![doc("docs/c.md")])), Widen::Sealed);
    }
}
