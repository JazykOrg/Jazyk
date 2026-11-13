// What generation and verification owe and what they left: the trace matrix with
// its export, pending verification and generation, and the per-entity generation
// package. Derived from the ledger and the files on disk (or in the tab's tree),
// never stored. Mirrors docs/frontends/gui.md#api (generation and verification,
// the trace matrix export) and docs/frontends/wasm.md#the-product.
use super::Refusal;
use crate::gen::GenSettings;
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const COLUMNS: [&str; 8] = [
    "requirement",
    "entity",
    "status",
    "reason",
    "repair",
    "test",
    "lastRun",
    "evidence",
];

fn cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// The matrix as a file: (body, content type, file name), or None for an unknown
// format.
pub fn matrix_export(
    rows: &BTreeMap<String, Value>,
    format: &str,
) -> Option<(String, &'static str, &'static str)> {
    let field = |row: &Value, col: &str| -> String {
        match col {
            "test" => cell(&row["test"]["label"]),
            _ => cell(&row[col]),
        }
    };
    match format {
        "csv" => {
            let mut out = COLUMNS.join(",");
            out.push('\n');
            for (rid, row) in rows {
                let mut cells = vec![csv_escape(rid)];
                cells.extend(COLUMNS[1..].iter().map(|c| csv_escape(&field(row, c))));
                out.push_str(&cells.join(","));
                out.push('\n');
            }
            Some((out, "text/csv; charset=utf-8", "trace-matrix.csv"))
        }
        "md" => {
            let mut out = format!(
                "| {} |\n|{}|\n",
                COLUMNS.join(" | "),
                COLUMNS.iter().map(|_| "---").collect::<Vec<_>>().join("|")
            );
            for (rid, row) in rows {
                let mut cells = vec![rid.clone()];
                cells.extend(
                    COLUMNS[1..]
                        .iter()
                        .map(|c| field(row, c).replace('|', "\\|").replace('\n', " ")),
                );
                out.push_str(&format!("| {} |\n", cells.join(" | ")));
            }
            Some((out, "text/markdown; charset=utf-8", "trace-matrix.md"))
        }
        _ => None,
    }
}

// The matrix rows the JSON endpoint serves, shared with the export: one row per
// graph requirement with its derived status, and the counts by status.
pub fn matrix_rows(
    store: &Store,
    gs: &GenSettings,
) -> (BTreeMap<String, Value>, BTreeMap<String, usize>) {
    let statuses = crate::verify::status_map(store, gs);
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    let mut rows: BTreeMap<String, Value> = BTreeMap::new();
    for (rid, v) in statuses {
        let s = v["status"].as_str().unwrap_or("unknown").to_string();
        *by_status.entry(s).or_default() += 1;
        let entity = store
            .graph
            .requirements
            .get(&rid)
            .and_then(|r| r.entities.first().cloned())
            .unwrap_or_default();
        let mut row = v.clone();
        row["entity"] = json!(entity);
        row["test"] = json!({ "kind": v["kind"], "run": v["run"], "label": v["label"], "path":v["artifactPath"] });
        rows.insert(rid, row);
    }
    (rows, by_status)
}

// `GET /api/verify/matrix`: the rows and the counts.
pub fn matrix_value(store: &Store, gs: &GenSettings) -> Value {
    let (rows, counts) = matrix_rows(store, gs);
    json!({ "rows": rows, "counts": counts })
}

// `GET /api/verify/pending`: the rows needing a verifier, filtered, and the counts
// by reason.
pub fn verify_pending_value(
    store: &Store,
    gs: &GenSettings,
    filter: Option<&str>,
    entity: Option<&str>,
) -> Value {
    let pending = crate::verify::pending(store, gs, filter, entity);
    json!({ "pending": pending, "counts": crate::verify::pending_counts(store, gs) })
}

// `GET /api/gen/pending`: the entities generation owes.
pub fn gen_pending_value(store: &Store, gs: &GenSettings) -> Value {
    json!({ "pending": crate::gen::pending(store, gs) })
}

// `GET /api/gen/package/{entity}`: the per-entity generation package a session
// receives; 404 for an unknown entity.
pub fn gen_package_value(store: &Store, id: &str, gs: &GenSettings) -> Result<Value, Refusal> {
    let id = store.resolve_id(id).to_string();
    if !store.graph.entities.contains_key(&id) {
        return Err((404, format!("no entity {}", id)));
    }
    crate::gen::task_package(store, &id, gs).map_err(|e| (400, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::{mark as gen_mark, test_name};
    use crate::model::*;
    use crate::webapi::deliverable::{deliverable_file, deliverable_value};

    // A small project written through crate::vfs: one entity, one requirement, a
    // product file with a marker, a test naming the requirement, and the ledger row
    // generation records for them.
    fn fixture(out: &std::path::Path) -> (Store, GenSettings) {
        let mut s = Store {
            out: out.to_path_buf(),
            ..Default::default()
        };
        s.graph.entities.insert(
            "ent:cart".into(),
            Entity {
                name: "Cart".into(),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                statement: "The Cart shall hold items.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "holds".into(),
                }),
                ..Default::default()
            },
        );
        s.save_for_test();
        let gs = GenSettings {
            configured: true,
            deliverable: out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        for d in ["src", "tests", "notes"] {
            crate::vfs::create_dir_all(gs.deliverable.join(d)).unwrap();
        }
        crate::vfs::write(
            gs.deliverable.join("src/cart.rs"),
            "// product\n// req:shop-1 hash:abcd\npub fn hold() {}\n",
        )
        .unwrap();
        crate::vfs::write(
            gs.deliverable.join("tests/cart.rs"),
            format!("fn {}() {{}}\n", name),
        )
        .unwrap();
        crate::vfs::write(gs.deliverable.join("notes/loose.txt"), "no one owns this\n").unwrap();
        let manifest = json!({
            "files": ["src/cart.rs", "tests/cart.rs"],
            "tests": [{
                "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
                "artifact": "tests/cart.rs", "name": name, "run": "true",
                "files": ["src/cart.rs"],
            }],
        });
        gen_mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
        (s, gs)
    }

    #[test]
    fn deliverable_lists_all_criterion_tests_without_duplicate_headlines() {
        let out = std::env::temp_dir().join(format!(
            "jazyk-webapi-criterion-sites-{}",
            crate::sys::process_id()
        ));
        let (_, gs) = fixture(&out);
        let mut ledger = crate::gen::Ledger::load(&out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        for (id, name) in [
            ("crit:first", row.test.name.clone()),
            ("crit:second", "second_case".into()),
        ] {
            row.tests.insert(
                id.into(),
                crate::gen::CritTest {
                    test: crate::gen::TestRef {
                        name,
                        ..row.test.clone()
                    },
                    criterion_hash: String::new(),
                    test_hash: String::new(),
                    verdict: "none".into(),
                    last_run: None,
                    exit_code: None,
                    evidence: None,
                },
            );
        }
        let text = format!("fn {}() {{}}\nfn second_case() {{}}\n", row.test.name);
        crate::vfs::write(gs.deliverable.join("tests/cart.rs"), text).unwrap();
        ledger.save(&out).unwrap();
        let file = deliverable_file(&out, &gs, "tests/cart.rs").unwrap();
        let sites = file["sites"].as_array().unwrap();
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0]["line"], 1);
        assert_eq!(sites[1]["line"], 2);
        crate::vfs::remove_dir_all(out).unwrap();
    }

    // The product reads the tab serves are the ones the server serves: the listing
    // with each file's owners, a file with its resolved site and test, and the
    // matrix derived from the ledger and the files, all through crate::vfs. An edit
    // to a product file moves its row to stale without touching the ledger.
    // Mirrors docs/frontends/wasm.md#the-product.
    #[test]
    fn deliverable_and_matrix_read_the_ledger_and_the_files() {
        let out =
            std::env::temp_dir().join(format!("jazyk-webapi-product-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);

        let listing = deliverable_value(&out, &gs);
        let files = listing["files"].as_array().unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
        assert_eq!(
            paths,
            vec!["notes/loose.txt", "src/cart.rs", "tests/cart.rs"]
        );
        let cart = &files[1];
        assert_eq!(cart["owners"]["entities"], json!(["cart"]));
        assert_eq!(cart["owners"]["requirements"], json!(["req:shop-1"]));
        assert_eq!(files[2]["owners"]["tests"], json!(["req:shop-1"]));
        assert_eq!(files[0]["owners"]["requirements"], json!([]));

        // The marker was stripped at mark time and its site recorded; the file read
        // locates it against the current text.
        let f = deliverable_file(&out, &gs, "src/cart.rs").unwrap();
        assert!(!f["text"].as_str().unwrap().contains("hash:abcd"));
        let site = f["sites"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["kind"] == "site")
            .unwrap();
        assert_eq!(site["requirement"], "req:shop-1");
        assert_eq!(site["located"], "exact");
        assert_eq!(site["line"], 2);
        assert_eq!(site["exists"], true);
        let t = deliverable_file(&out, &gs, "tests/cart.rs").unwrap();
        assert_eq!(t["sites"][0]["kind"], "test");
        assert_eq!(t["sites"][0]["located"], "exact");
        assert_eq!(
            deliverable_file(&out, &gs, "src/missing.rs").unwrap_err().0,
            404
        );
        assert_eq!(
            deliverable_file(&out, &gs, "../outside").unwrap_err().0,
            400
        );

        let m = matrix_value(&s, &gs);
        let row = &m["rows"]["req:shop-1"];
        assert_eq!(row["entity"], "ent:cart");
        assert_eq!(row["test"]["kind"], "programmatic");
        assert_eq!(row["test"]["label"], "unit");
        let status = row["status"].as_str().unwrap().to_string();
        assert!(
            !status.starts_with("stale") && status != "missing",
            "{}",
            status
        );
        assert_eq!(m["counts"][status.as_str()], 1);

        let (rows, _) = matrix_rows(&s, &gs);
        let (csv, ctype, name) = matrix_export(&rows, "csv").unwrap();
        assert!(csv.starts_with("requirement,entity,status"));
        assert!(csv.contains("req:shop-1,ent:cart,"));
        assert_eq!(
            (ctype, name),
            ("text/csv; charset=utf-8", "trace-matrix.csv")
        );
        assert!(matrix_export(&rows, "md")
            .unwrap()
            .0
            .contains("| req:shop-1 | ent:cart |"));
        assert!(matrix_export(&rows, "xls").is_none());

        // Status is derived: the product file changes, the row goes stale.
        crate::vfs::write(
            gs.deliverable.join("src/cart.rs"),
            "// product\npub fn hold() { todo!() }\n",
        )
        .unwrap();
        let m = matrix_value(&s, &gs);
        assert_eq!(m["rows"]["req:shop-1"]["status"], "stale-code");
        let f = deliverable_file(&out, &gs, "src/cart.rs").unwrap();
        assert_eq!(f["sites"][0]["located"], "lost");
        let pending = verify_pending_value(&s, &gs, Some("all"), None);
        assert_eq!(pending["pending"][0]["status"], "stale-code");

        assert_eq!(gen_package_value(&s, "ent:nope", &gs).unwrap_err().0, 404);
        crate::vfs::remove_dir_all(&out).ok();
    }

    // A cell is quoted when it carries a separator, a quote, or a line break of
    // either kind, and an inner quote doubles; a plain cell passes through.
    #[test]
    fn csv_escape_quotes_commas_quotes_newlines_and_cr() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape(""), "");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape("line\nbreak"), "\"line\nbreak\"");
        assert_eq!(csv_escape("carriage\rreturn"), "\"carriage\rreturn\"");
        assert_eq!(csv_escape("both\r\nends"), "\"both\r\nends\"");
    }
}
