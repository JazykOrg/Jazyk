use crate::model::*;
use crate::store::Store;
use std::collections::BTreeMap;

use super::*;

// A sandbox store holding the fixture's documents and coverage, no graph nodes yet.
pub fn sandbox_docs(case: &Case, tmp: &std::path::Path) -> Store {
    let mut s = Store {
        out: tmp.join(".jazyk/ds_main"),
        ..Default::default()
    };
    for (doc, text) in &case.docs {
        s.docs.insert(
            doc.clone(),
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
    }
    for (full, state) in &case.coverage {
        if let Some((doc, section)) = split_section_ref(full) {
            if let Some(rec) = s.docs.get_mut(&doc) {
                rec.coverage.insert(
                    section,
                    Coverage {
                        state: state.clone(),
                        note: None,
                        claimed_by: None,
                    },
                );
            }
        }
    }
    s
}

// Seed a sandbox store from a case fixture, the nodes written straight into the
// graph. The sandbox writes to a throwaway out dir.
pub fn sandbox(case: &Case, tmp: &std::path::Path) -> Store {
    let mut s = sandbox_docs(case, tmp);
    for (id, e) in &case.entities {
        s.graph.entities.insert(id.clone(), entity_of(e));
    }
    for (id, r) in &case.requirements {
        if let Some(req) = requirement_of(r) {
            s.graph.requirements.insert(id.clone(), req);
        }
    }
    for (id, v) in &case.views {
        if let Ok(view) = view_of(v) {
            s.graph.views.insert(id.clone(), view);
        }
    }
    for (id, c) in &case.criteria {
        if let Ok(crit) = criterion_of(c) {
            s.graph.criteria.insert(id.clone(), crit);
        }
    }
    crate::derive::recompute_relationships(&mut s);
    s
}

// The fixture as one changeset: entities parents first, then requirements, then
// views. Mirrors docs/benchmark/cases.md#derived-goals.
fn fixture_ops(case: &Case) -> Result<Vec<crate::store::Op>, String> {
    use crate::store::Op;
    let mut ops = Vec::new();
    let mut placed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut pending: Vec<(&String, Entity)> = case
        .entities
        .iter()
        .map(|(id, e)| (id, entity_of(e)))
        .collect();
    while !pending.is_empty() {
        let before = pending.len();
        let (ready, rest): (Vec<_>, Vec<_>) = pending.into_iter().partition(|(_, e)| {
            e.parent
                .as_ref()
                .map(|p| placed.contains(p) || !case.entities.contains_key(p))
                .unwrap_or(true)
        });
        for (id, entity) in ready {
            placed.insert(id.clone());
            ops.push(Op::CreateEntity {
                id: id.clone(),
                entity,
            });
        }
        pending = rest;
        if pending.len() == before {
            let ids: Vec<&str> = pending.iter().map(|(id, _)| id.as_str()).collect();
            return Err(format!("entities with a parent cycle: {}", ids.join(", ")));
        }
    }
    for (id, r) in &case.requirements {
        let requirement =
            requirement_of(r).ok_or_else(|| format!("requirement {} has no source", id))?;
        ops.push(Op::CreateRequirement {
            id: id.clone(),
            requirement,
        });
    }
    for (id, c) in &case.criteria {
        let criterion = criterion_of(c).map_err(|e| format!("criterion {}: {}", id, e))?;
        ops.push(Op::CreateCriterion {
            id: id.clone(),
            criterion,
        });
    }
    for (id, v) in &case.views {
        let view = view_of(v).map_err(|e| format!("view {}: {}", id, e))?;
        ops.push(Op::CreateView {
            id: id.clone(),
            view,
        });
    }
    Ok(ops)
}

// Seed a sandbox through one real commit and derive the case's goal on the board:
// the commit writes the change records the fixture implies, and the goal carries the
// change and hints a build would give it. A fixture the commit refuses, or one that
// derives no such goal, is a fixture error. Mirrors docs/benchmark/cases.md#derived-goals.
pub fn prepare_runtime_fixture(case: &Case, tmp: &std::path::Path) -> Result<(), String> {
    crate::vfs::create_dir_all(tmp.join(".jazyk")).map_err(|e| e.to_string())?;
    let docs: Vec<_> = case.docs.keys().collect();
    let config = format!(
        "version=1\n[[sets]]\nid=\"ds_main\"\nname=\"Benchmark\"\ndocs={}\nsource=[\"**\"]\n",
        serde_json::to_string(&docs).map_err(|e| e.to_string())?
    );
    crate::vfs::write(tmp.join(".jazyk/jazyk.toml"), config).map_err(|e| e.to_string())?;
    for (path, text) in &case.docs {
        if !crate::document_sets::safe_relative(tmp, path) || crate::task_runtime::protected(path) {
            return Err(format!("Unsafe fixture document path: {path}"));
        }
        let path = tmp.join(path);
        if let Some(parent) = path.parent() {
            crate::vfs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        crate::vfs::write(path, text).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn fixture_gen_settings(tmp: &std::path::Path) -> Result<crate::gen::GenSettings, String> {
    let workspace = crate::document_sets::Workspace::load(tmp).map_err(|e| e.1)?;
    let project = workspace.resolve(Some("ds_main")).map_err(|e| e.1)?;
    Ok(crate::gen::GenSettings::resolve(&project))
}

pub fn seed_derived(case: &Case, tmp: &std::path::Path) -> Result<(Store, Goal), String> {
    prepare_runtime_fixture(case, tmp)?;
    let mut store = sandbox_docs(case, tmp);
    let ops = fixture_ops(case)?;
    let report = store.apply(ops, &crate::store::Commit::store("edit"));
    if !report.skipped.is_empty() {
        return Err(format!(
            "the seeding commit refused: {}",
            report.skipped.join("; ")
        ));
    }
    // A deliverable is configured so the ledger-tier kinds derive, holding the
    // fixture's files (docs/benchmark/cases.md#case-format).
    write_deliverable(case, tmp)?;
    let proj = crate::project::Project {
        root: tmp.to_path_buf(),
        out: store.out.clone(),
        gen_deliverable: Some(".".into()),
        gen_code: vec!["**".into()],
        ..Default::default()
    };
    let control = crate::control::Control::load(&proj, &proj.out);
    let board = crate::board::Board::derive(&store, &proj, &control);
    let id = crate::goals::goal_id(&case.kind, &case.target);
    match board.goals.iter().find(|g| g.id == id) {
        Some(g) => Ok((store, g.clone())),
        None => {
            let same_kind: Vec<&str> = board
                .goals
                .iter()
                .filter(|g| g.kind == case.kind)
                .map(|g| g.id.as_str())
                .collect();
            Err(format!(
                "the fixture derives no {}; {} goals derived: {}",
                id,
                case.kind,
                if same_kind.is_empty() {
                    "(none)".to_string()
                } else {
                    same_kind.join(", ")
                }
            ))
        }
    }
}

// The fixture's deliverable files, written under the sandbox's deliverable directory.
fn write_deliverable(case: &Case, dir: &std::path::Path) -> Result<(), String> {
    for (f, text) in &case.deliverable {
        let p = dir.join(f);
        if let Some(parent) = p.parent() {
            crate::vfs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        crate::vfs::write(&p, text).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// Seed the llm-judge fixture for a verify-requirement case: the implementing files
// under the temp deliverable, the criteria file, and the ledger row the judge reads.
pub fn seed_verification(
    case: &Case,
    store: &Store,
    gs: &crate::gen::GenSettings,
) -> Result<(), String> {
    let rid = &case.target;
    let r = store
        .graph
        .requirements
        .get(rid)
        .ok_or_else(|| format!("fixture has no requirement {}", rid))?;
    write_deliverable(case, &gs.deliverable)?;
    let files: Vec<String> = case.deliverable.keys().cloned().collect();
    let artifact = format!("criteria/req-{}.md", crate::gen::req_slug(rid));
    let criteria = format!(
        "---\nrequirement: {}\nhash: {}\n---\n\n# Verify {}\n\nStatement: {}\n\n> {}\n\nImplementing files (under the deliverable):\n{}\n\nConfirm the statement holds in the implementation. Verdict contract: reply PASS or FAIL with reasoning.\n",
        rid,
        hash_hex(&r.statement),
        rid,
        r.statement,
        r.source.as_ref().map(|s| s.quote.as_str()).unwrap_or_default(),
        files.iter().map(|f| format!("- {}", f)).collect::<Vec<_>>().join("\n")
    );
    let crit_path = store.out.join("gen").join(&artifact);
    if let Some(parent) = crit_path.parent() {
        crate::vfs::create_dir_all(parent).ok();
    }
    crate::vfs::write(&crit_path, &criteria).map_err(|e| e.to_string())?;
    let test = crate::gen::TestRef {
        kind: "llm".into(),
        label: "judge".into(),
        artifact,
        name: crate::gen::test_name(rid, &r.statement),
        run: format!("jazyk test {}", rid),
        cwd: ".".into(),
    };
    let _lock = crate::gen::Ledger::lock(&store.out);
    let mut ledger = crate::gen::Ledger::load(&store.out);
    ledger.requirements.insert(
        rid.clone(),
        crate::gen::ReqRow {
            tests: std::collections::BTreeMap::new(),
            entity: r.entities.first().cloned().unwrap_or_default(),
            files: files.clone(),
            sites: Vec::new(),
            hashes: crate::gen::RowHashes {
                requirement: hash_hex(&r.statement),
                test: crate::gen::hash_file(&crate::gen::artifact_path(&store.out, gs, &test)),
                files: crate::gen::hash_files(gs, &files),
            },
            test,
            verdict: "none".into(),
            last_run: None,
            exit_code: None,
            evidence: None,
        },
    );
    ledger.save(&store.out)
}
