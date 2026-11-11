use super::text::oneline;
use super::*;
use crate::derive::recompute;
use crate::derive::tests::showcase_store;
use crate::store::RecordBatch;
use regex::Regex;

mod emitters;
mod output;
mod renderer;
mod scene;

fn showcase() -> Store {
    let mut s = showcase_store();
    let mut batch = RecordBatch::new(1);
    recompute(&mut s, "g1", &mut batch);
    for (id, kind, members) in [
        (
            "view:usecase/checkout",
            "use-case",
            vec!["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"],
        ),
        (
            "view:sequence/checkout",
            "sequence",
            vec!["req:shop-1", "req:shop-3"],
        ),
    ] {
        s.graph.views.insert(
            id.to_string(),
            View {
                kind: kind.to_string(),
                title: "Checkout".to_string(),
                members: members.iter().map(|m| m.to_string()).collect(),
                provenance: Some(Provenance::Decree {
                    author: "owner".into(),
                    at: "g1".into(),
                    note: None,
                }),
                ..Default::default()
            },
        );
    }
    s
}

fn view(kind: &str, title: &str, members: &[&str]) -> View {
    View {
        kind: kind.to_string(),
        title: title.to_string(),
        members: members.iter().map(|m| m.to_string()).collect(),
        ..Default::default()
    }
}

fn emit_kind(s: &Store, kind: &str, members: &[&str]) -> String {
    let v = view(kind, "Checkout", members);
    emit(s, &format!("view:{}/checkout", view_kind_slug(kind)), &v)
}

// Structural lines: aliases replaced by display names, quotes dropped, whitespace
// collapsed, free-text labels reduced to their requirement id or step number, the
// drill-down links dropped (their own tests read the raw text).
fn canon(puml: &str) -> Vec<String> {
    let decl = Regex::new(r#"^(\w+) (?:"([^"]*)"|([^\s"]+)) as ([^\s{\[]+)(.*)$"#).unwrap();
    let part = Regex::new(r"^\[([^\]]+)\] as (\S+)(.*)$").unwrap();
    let req = Regex::new(r"\((req:[^)]+)\)").unwrap();
    let num = Regex::new(r"^(\d+\.)").unwrap();
    let link = Regex::new(r" \[\[[^\]]*\]\]").unwrap();
    let mut map: Vec<(String, String)> = Vec::new();
    let mut out = Vec::new();
    for raw in puml.lines() {
        let line = link.replace_all(&oneline(raw), "").to_string();
        if line.is_empty() {
            continue;
        }
        // A declaration names its element directly; only other lines carry aliases.
        if let Some(c) = decl.captures(&line) {
            let name = c.get(2).or(c.get(3)).map(|m| m.as_str()).unwrap();
            map.push((c[4].to_string(), name.to_string()));
            out.push(format!("{} {}{}", &c[1], name, &c[5]).replace('"', ""));
            continue;
        }
        if let Some(c) = part.captures(&line) {
            map.push((c[2].to_string(), c[1].to_string()));
            out.push(format!("[{}]{}", &c[1], &c[3]));
            continue;
        }
        let (head, label) = match line.split_once(" : ") {
            Some((h, l)) => (h.to_string(), Some(l.to_string())),
            None => (line.clone(), None),
        };
        let head = head
            .split(' ')
            .map(|tok| {
                let t = tok.trim_matches('"');
                map.iter()
                    .find(|(a, _)| a == t)
                    .map(|(_, n)| n.as_str())
                    .unwrap_or(t)
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(" ");
        let label = label.map(|l| {
            if let Some(c) = num.captures(&l) {
                c[1].to_string()
            } else if let Some(c) = req.captures(&l) {
                format!("({})", &c[1])
            } else {
                l
            }
        });
        let line = match label {
            Some(l) => format!("{} : {}", head, l),
            None => head,
        };
        let line = if line.starts_with(':') && line.ends_with(';') {
            match req.captures(&line) {
                Some(c) => format!(":({});", &c[1]),
                None => line,
            }
        } else {
            line
        };
        out.push(line.replace('"', ""));
    }
    out
}

fn lines_with(puml: &str, needle: &str) -> Vec<String> {
    canon(puml)
        .into_iter()
        .filter(|l| l.contains(needle))
        .collect()
}

fn tree_store() -> Store {
    let mut s = Store::default();
    for (id, parent) in [
        ("ent:a", None),
        ("ent:a1", Some("ent:a")),
        ("ent:a2", Some("ent:a")),
        ("ent:b", None),
        ("ent:b1", Some("ent:b")),
    ] {
        s.graph.entities.insert(
            id.into(),
            Entity {
                name: id.trim_start_matches("ent:").to_uppercase(),
                parent: parent.map(String::from),
                ..Default::default()
            },
        );
    }
    s
}

fn add_req(s: &mut Store, id: &str, a: &str, b: &str, t: &str) {
    s.graph.requirements.insert(
        id.into(),
        Requirement {
            statement: format!("{} uses {}", a, b),
            entities: vec![a.into(), b.into()],
            edges: vec![ReqEdge {
                a: a.into(),
                b: b.into(),
                rel_type: Some(t.into()),
                cardinality: None,
            }],
            source: Some(SourceRef {
                doc: "t.md".into(),
                section: "/t".into(),
                quote: "q".into(),
            }),
            ..Default::default()
        },
    );
}

// ent:a holds two children and ent:b one: a has a level view, b is a leaf for
// drill-down.
fn level_tree() -> Store {
    let mut s = tree_store();
    add_req(&mut s, "req:t-1", "ent:a1", "ent:b1", "dependency");
    recompute(&mut s, "g1", &mut RecordBatch::new(1));
    s
}

thread_local! {
    pub(super) static XML_ATTR_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// The scope root's level view: view:class/public or view:component/public by the
// kind rule, whichever the derivation minted.
fn root_view(s: &Store) -> String {
    ["view:component/public", "view:class/public"]
        .into_iter()
        .find(|id| s.graph.views.contains_key(*id))
        .map(String::from)
        .expect("the showcase derives a root level view")
}

fn tmp(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jazyk-render-{}-{}",
        label,
        crate::sys::process_id()
    ));
    let _ = crate::vfs::remove_dir_all(&dir);
    crate::vfs::create_dir_all(&dir).unwrap();
    dir
}
