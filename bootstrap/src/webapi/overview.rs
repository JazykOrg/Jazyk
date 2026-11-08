// The Product lens's overview: the product itself, as its kind shows it. A written
// product is its prose files in reading order, a built product the artifacts its
// build produces, a program its README and run commands. Read-only and
// target-neutral: every read goes through crate::vfs, so a tab answers from the tree
// what the server answers from disk. Mirrors docs/frontends/gui.md#api
// (`GET /api/product/overview`).
use super::deliverable::walk;
use crate::gen::{GenSettings, Ledger, TestRef};
use crate::project::Project;
use crate::vfs::VPathExt;
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashSet};
use std::path::Path;

// The directories tests and criteria files live under by convention.
const TEST_DIRS: &[&str] = &["test", "tests", "criteria"];
// How many distinct test commands the overview names before counting the rest.
const COMMANDS_SHOWN: usize = 3;

fn ext(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, e)) if !stem.is_empty() => e.to_ascii_lowercase(),
        _ => String::new(),
    }
}

fn is_prose(path: &str) -> bool {
    matches!(ext(path).as_str(), "md" | "markdown" | "txt")
}

fn tests_of(ledger: &Ledger) -> impl Iterator<Item = &TestRef> {
    ledger
        .requirements
        .values()
        .flat_map(|row| std::iter::once(&row.test).chain(row.tests.values().map(|c| &c.test)))
}

// Tests aside: a test artifact the ledger records, or any file under a test or
// criteria directory.
fn is_test(path: &str, artifacts: &HashSet<&str>) -> bool {
    if artifacts.contains(path) {
        return true;
    }
    let mut dirs: Vec<&str> = path.split('/').collect();
    dirs.pop();
    dirs.iter()
        .any(|d| TEST_DIRS.contains(&d.to_ascii_lowercase().as_str()))
}

// File names compared the way a reader counts: a run of digits as a number, so
// `2-x` sorts before `10-x`; the rest case-insensitively.
pub(crate) fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn runs(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, run)) if *d == digit => run.push(c),
                _ => out.push((digit, c.to_string())),
            }
        }
        out
    }
    let (ra, rb) = (runs(a), runs(b));
    for (x, y) in ra.iter().zip(rb.iter()) {
        let o = match (x.0, y.0) {
            (true, true) => {
                let (tx, ty) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
                tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty))
            }
            _ => x.1.to_lowercase().cmp(&y.1.to_lowercase()),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    ra.len().cmp(&rb.len()).then_with(|| a.cmp(b))
}

// The relative links of an index, in the order it gives them, resolved against the
// deliverable's root: no scheme, no fragment, no query, `./` and `..` folded.
pub(crate) fn index_links(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\]\(\s*<?([^)\s>]+)>?").expect("link pattern");
    let mut out: Vec<String> = Vec::new();
    for cap in re.captures_iter(text) {
        let raw = &cap[1];
        if raw.starts_with('#') || raw.contains(':') {
            continue;
        }
        let path = raw
            .split(['#', '?'])
            .next()
            .unwrap_or("")
            .replace("%20", " ");
        let mut parts: Vec<&str> = Vec::new();
        for seg in path.split('/') {
            match seg {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                s => parts.push(s),
            }
        }
        if parts.is_empty() {
            continue;
        }
        let joined = parts.join("/");
        if !out.contains(&joined) {
            out.push(joined);
        }
    }
    out
}

fn read_text(abs: &Path) -> Option<String> {
    String::from_utf8(crate::vfs::read(abs).ok()?).ok()
}

// A written product's parts in reading order: an index first, then what it links to
// in its order, then the rest by name; without an index, the names alone.
fn reading_order(prose: &[String]) -> (Vec<String>, Option<String>) {
    let index = prose
        .iter()
        .find(|p| {
            matches!(
                p.rsplit('/')
                    .next()
                    .unwrap_or(p)
                    .to_ascii_lowercase()
                    .as_str(),
                "readme.md" | "index.md"
            )
        })
        .cloned();
    let mut rest: Vec<String> = prose
        .iter()
        .filter(|p| Some(*p) != index.as_ref())
        .cloned()
        .collect();
    rest.sort_by(|a, b| natural_cmp(a, b));
    (rest, index)
}

// `GET /api/product/overview`: what the overview shows. Mirrors
// docs/frontends/gui.md#api.
pub fn overview_value(p: &Project, gs: &GenSettings) -> Value {
    let ledger = Ledger::load(&p.out);
    let mut files: Vec<(String, u64)> = Vec::new();
    walk(&gs.deliverable, &gs.deliverable, &mut files);
    files.retain(|(f, _)| gs.allows_source(f));
    let sizes: std::collections::HashMap<String, u64> = files.iter().cloned().collect();
    let artifacts: HashSet<&str> = tests_of(&ledger)
        .map(|t| t.artifact.as_str())
        .filter(|a| !a.is_empty())
        .collect();
    // A deliverable at the project root holds the project too: its documents and
    // its settings are the source, never the product.
    let docs: HashSet<String> = p
        .doc_files()
        .iter()
        .filter_map(|f| f.strip_prefix(&gs.deliverable).ok())
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .collect();
    let settings = p.root.join(crate::document_sets::CONFIG);
    let mut product: Vec<String> = files
        .iter()
        .map(|(f, _)| f.clone())
        .filter(|f| !is_test(f, &artifacts) && !docs.contains(f))
        .filter(|f| gs.deliverable.join(f) != settings)
        .collect();
    product.sort_by(|a, b| natural_cmp(a, b));
    let prose: Vec<String> = product.iter().filter(|f| is_prose(f)).cloned().collect();

    // The distinct test commands, in ledger order.
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut commands: Vec<Value> = Vec::new();
    for t in tests_of(&ledger) {
        if t.kind != "programmatic" || t.run.trim().is_empty() {
            continue;
        }
        if seen.insert((t.run.clone(), t.cwd.clone())) {
            commands.push(json!({ "run": t.run, "cwd": t.cwd }));
        }
    }
    let more = commands.len().saturating_sub(COMMANDS_SHOWN);
    commands.truncate(COMMANDS_SHOWN);

    let built = ledger.medium.as_ref().is_some_and(|m| m.is_built());
    let kind = if built {
        "built"
    } else if product.is_empty() {
        "none"
    } else if prose.len() * 2 > product.len() {
        "written"
    } else {
        "program"
    };

    let mut v = json!({
        "kind": kind,
        "medium": ledger.medium,
        "build": ledger.build,
        "commands": commands,
        "moreCommands": more,
    });
    match kind {
        "written" => {
            let (rest, index) = reading_order(&prose);
            let mut order: Vec<String> = Vec::new();
            if let Some(ix) = &index {
                order.push(ix.clone());
                let text = read_text(&gs.deliverable.join(ix)).unwrap_or_default();
                for l in index_links(&text) {
                    let l = Path::new(ix)
                        .parent()
                        .unwrap_or_else(|| Path::new(""))
                        .join(l)
                        .to_string_lossy()
                        .replace('\\', "/");
                    if rest.contains(&l) && !order.contains(&l) {
                        order.push(l);
                    }
                }
            }
            for f in rest {
                if !order.contains(&f) {
                    order.push(f);
                }
            }
            let parts: Vec<Value> = order
                .iter()
                .filter_map(|f| {
                    let text = read_text(&gs.deliverable.join(f))?;
                    Some(json!({ "path": f, "text": text }))
                })
                .collect();
            v["order"] = json!(if index.is_some() { "index" } else { "names" });
            v["index"] = json!(index);
            v["parts"] = json!(parts);
        }
        "built" => {
            let mut paths: Vec<String> = ledger
                .build
                .as_ref()
                .map(|b| b.produces.clone())
                .unwrap_or_default();
            if paths.is_empty() {
                if let Some(a) = ledger.medium.as_ref().map(|m| m.artifact.clone()) {
                    if !a.is_empty() {
                        paths.push(a);
                    }
                }
            }
            let list: Vec<Value> = paths
                .iter()
                .map(|a| {
                    let a = a.trim_start_matches("./").to_string();
                    let abs = gs.deliverable.join(&a);
                    let exists = abs.vis_file();
                    // The listing skips `target` and hidden entries, where a build
                    // may still write its artifact.
                    let size = sizes.get(&a).copied().or_else(|| {
                        exists
                            .then(|| crate::vfs::read(&abs).ok().map(|b| b.len() as u64))
                            .flatten()
                    });
                    json!({
                        "path": a,
                        "exists": exists,
                        "size": size,
                        "contentType": super::outfiles::content_type(&a),
                    })
                })
                .collect();
            v["artifacts"] = json!(list);
        }
        "program" => {
            v["readme"] = readme(p, gs, &product);
        }
        _ => {}
    }
    v
}

// A program's README: the deliverable's own, else the project's root document, else
// its first document.
fn readme(p: &Project, gs: &GenSettings, product: &[String]) -> Value {
    let own = product.iter().find(|f| {
        let lower = f.rsplit('/').next().unwrap_or(f).to_ascii_lowercase();
        matches!(
            lower.as_str(),
            "readme" | "readme.md" | "readme.markdown" | "readme.txt"
        )
    });
    if let Some(f) = own {
        if let Some(text) = read_text(&gs.deliverable.join(f)) {
            return json!({ "path": f, "tree": "deliverable", "text": text });
        }
    }
    let mut docs: Vec<String> = p
        .doc_files()
        .iter()
        .map(|f| super::status::rel_doc(&p.root, f))
        .collect();
    docs.sort_by(|a, b| natural_cmp(a, b));
    let pick = docs
        .iter()
        .find(|d| p.is_root_file(d))
        .or_else(|| docs.first());
    match pick.and_then(|d| read_text(&p.root.join(d)).map(|t| (d, t))) {
        Some((d, text)) => json!({ "path": d, "tree": "docs", "text": text }),
        None => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_sort_the_way_a_reader_counts() {
        let mut v = vec![
            "10-the-dawn.md",
            "03-the-survey.md",
            "2-the-lease.md",
            "b.md",
            "A.md",
        ];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            v,
            vec![
                "2-the-lease.md",
                "03-the-survey.md",
                "10-the-dawn.md",
                "A.md",
                "b.md"
            ]
        );
    }

    #[test]
    fn an_index_gives_its_links_in_order_resolved_against_the_root() {
        let text = "# Handbook\n\n- [Hiring](./hiring.md)\n- [Web](https://x.org/a.md)\n- [Top](#top)\n- [Pay](policies/pay.md#rates)\n- [Again](hiring.md)\n- [Up](sub/../expenses.md)\n";
        assert_eq!(
            index_links(text),
            vec!["hiring.md", "policies/pay.md", "expenses.md"]
        );
    }

    fn project_at(name: &str, toml: &str, files: &[(&str, &[u8])]) -> Project {
        let dir = std::env::temp_dir().join(format!(
            "jazyk-overview-{}-{}",
            name,
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(&dir).unwrap();
        crate::vfs::write(
            {
                let path = dir.join(".jazyk/jazyk.toml");
                crate::vfs::create_dir_all(path.parent().unwrap()).unwrap();
                path
            },
            toml,
        )
        .unwrap();
        for (rel, bytes) in files {
            let p = dir.join(rel);
            crate::vfs::create_dir_all(p.parent().unwrap()).unwrap();
            crate::vfs::write(&p, bytes).unwrap();
        }
        Project::load(&dir)
    }

    const TOML: &str = "version = 1\ngen = {}\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = [\"product/**\"]\n";

    #[test]
    fn a_written_product_reads_in_index_order_then_by_name() {
        let p = project_at(
            "written",
            TOML,
            &[
                ("docs/main.md", b"# Book\n"),
                (
                    "product/README.md",
                    b"# Contents\n\n[Two](b-two.md)\n[One](a-one.md)\n",
                ),
                ("product/a-one.md", b"# One\n"),
                ("product/b-two.md", b"# Two\n"),
                ("product/c-three.md", b"# Three\n"),
                ("product/tests/check.md", b"# a criteria file\n"),
            ],
        );
        let v = overview_value(&p, &GenSettings::resolve(&p));
        assert_eq!(v["kind"], "written");
        assert_eq!(v["order"], "index");
        let paths: Vec<&str> = v["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            vec![
                "product/README.md",
                "product/b-two.md",
                "product/a-one.md",
                "product/c-three.md"
            ]
        );
        crate::vfs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn a_program_shows_the_root_document_when_it_has_no_readme() {
        let p = project_at(
            "program",
            TOML,
            &[
                ("docs/main.md", b"# The sort\n"),
                ("product/main.js", b"console.log(1)\n"),
                ("product/lib.js", b"module.exports = 1\n"),
                ("product/notes.md", b"# notes\n"),
            ],
        );
        let v = overview_value(&p, &GenSettings::resolve(&p));
        assert_eq!(v["kind"], "program");
        assert_eq!(v["readme"]["tree"], "docs");
        assert_eq!(v["readme"]["path"], "docs/main.md");
        crate::vfs::write(p.root.join("product/Readme.md"), "# Sort\n").unwrap();
        let v = overview_value(&p, &GenSettings::resolve(&p));
        assert_eq!(v["readme"]["tree"], "deliverable");
        assert_eq!(v["readme"]["path"], "product/Readme.md");
        crate::vfs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn a_built_product_lists_its_artifact_and_an_empty_one_has_none() {
        let p = project_at(
            "built",
            TOML,
            &[
                ("docs/main.md", b"# Deck\n"),
                ("product/build.py", b"print(1)\n"),
                ("product/dist/deck.pptx", &[0x50, 0x4b, 0x03, 0x04, 0xff]),
            ],
        );
        crate::vfs::create_dir_all(p.out.join("gen")).unwrap();
        crate::vfs::write(
            p.out.join("gen/ledger.yaml"),
            "medium:\n  form: deck\n  produced: built\n  toolchain: python\n  artifact: product/dist/deck.pptx\n",
        )
        .unwrap();
        let v = overview_value(&p, &GenSettings::resolve(&p));
        assert_eq!(v["kind"], "built");
        assert_eq!(v["artifacts"][0]["path"], "product/dist/deck.pptx");
        assert_eq!(v["artifacts"][0]["exists"], true);
        assert_eq!(v["artifacts"][0]["size"], 5);
        assert_eq!(
            v["artifacts"][0]["contentType"],
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        );
        crate::vfs::remove_dir_all(&p.root).ok();

        let p = project_at("none", TOML, &[("docs/main.md", b"# Shop\n")]);
        assert_eq!(
            overview_value(&p, &GenSettings::resolve(&p))["kind"],
            "none"
        );
        crate::vfs::remove_dir_all(&p.root).ok();

        // A deliverable at the root holds the documents and the settings: not a product.
        let p = project_at(
            "root",
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
            &[("docs/main.md", b"# Shop\n"), ("docs/cart.md", b"# Cart\n")],
        );
        assert_eq!(
            overview_value(&p, &GenSettings::resolve(&p))["kind"],
            "none"
        );
        crate::vfs::remove_dir_all(&p.root).ok();
    }
}
