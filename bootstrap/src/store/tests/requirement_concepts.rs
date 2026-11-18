use super::*;

fn concept_store() -> Store {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(
        &mut s,
        "a.md",
        "# A\nThe background is blue. Use #1E40AF for it.\n",
    );
    seed_doc(&mut s, "b.md", "# B\nThe background uses #1E40AF.\n");
    s.graph
        .entities
        .insert("ent:cart".into(), Entity::default());
    s
}
fn concept() -> Requirement {
    let mut r = seeded_req("a.md", "The background uses #1E40AF.");
    r.sources = vec![mention(
        "a.md",
        "/a",
        "The background is blue. Use #1E40AF for it.",
    )];
    r
}

#[test]
fn requirement_sources_union_on_retry_and_preserve_statement_hash() {
    let mut s = concept_store();
    let mut r = concept();
    s.graph
        .requirements
        .insert("req:background".into(), r.clone());
    let hash = v5::statement_hash(&r.statement);
    let before = v5::entity_fact_hash(&s, "ent:cart");
    r.sources = vec![mention("b.md", "/b", "The background uses #1E40AF.")];
    let report = s.apply(
        vec![Op::CreateRequirement {
            id: "req:retry".into(),
            requirement: r.clone(),
        }],
        &session(),
    );
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(s.graph.requirements.len(), 1);
    assert_eq!(s.graph.requirements["req:background"].sources.len(), 2);
    assert_eq!(
        hash,
        v5::statement_hash(&s.graph.requirements["req:background"].statement)
    );
    assert_ne!(before, v5::entity_fact_hash(&s, "ent:cart"));
    s.apply(
        vec![Op::CreateRequirement {
            id: "req:retry".into(),
            requirement: r,
        }],
        &session(),
    );
    assert_eq!(s.graph.requirements["req:background"].sources.len(), 2);
}

#[test]
fn requirement_partial_source_removal_preserves_concept_until_last_location() {
    let mut s = concept_store();
    let mut r = concept();
    let a = r.sources[0].clone();
    let b = mention("b.md", "/b", "The background uses #1E40AF.");
    r.sources.push(b.clone());
    s.graph.requirements.insert("req:background".into(), r);
    s.apply(
        vec![Op::RemoveRequirementSource {
            id: "req:background".into(),
            source: a,
            reason: "sentence removed".into(),
        }],
        &session(),
    );
    assert_eq!(
        s.graph.requirements["req:background"].sources,
        vec![b.clone()]
    );
    s.apply(
        vec![Op::RemoveRequirementSource {
            id: "req:background".into(),
            source: b,
            reason: "last sentence removed".into(),
        }],
        &session(),
    );
    assert!(!s.graph.requirements.contains_key("req:background"));
}

#[test]
fn requirement_each_location_participates_in_staleness_and_sweep() {
    let mut s = concept_store();
    let mut r = concept();
    r.sources
        .push(mention("b.md", "/b", "The background uses #1E40AF."));
    s.graph.requirements.insert("req:background".into(), r);
    s.docs
        .get_mut("b.md")
        .unwrap()
        .sections
        .get_mut("/b")
        .unwrap()
        .raw = "The background is red.".into();
    assert_eq!(s.stale_extras("b.md").0, vec!["req:background"]);
    s.docs.remove("b.md");
    s.gc();
    assert_eq!(s.graph.requirements["req:background"].sources.len(), 1);
    let back = Store::load(&s.out);
    assert_eq!(back.graph.requirements["req:background"].sources.len(), 1);
}

#[test]
fn requirement_duplicate_merge_redirects_and_reparents_criteria() {
    let mut s = concept_store();
    s.graph.requirements.insert("req:a".into(), concept());
    let mut b = concept();
    b.sources = vec![mention("b.md", "/b", "The background uses #1E40AF.")];
    s.graph.requirements.insert("req:b".into(), b);
    s.graph.criteria.insert(
        "crit:b".into(),
        Criterion {
            requirement: "req:b".into(),
            statement: "The rendered pixel is #1E40AF.".into(),
            provenance: Some(Provenance::Derived {
                from: vec!["req:b".into()],
                reasoning: "observable color".into(),
            }),
            ..Default::default()
        },
    );
    let report = s.apply(
        vec![Op::MergeRequirements {
            keep: "req:a".into(),
            absorb: "req:b".into(),
            reason: "same color obligation".into(),
        }],
        &session(),
    );
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(s.resolve_id("req:b"), "req:a");
    assert_eq!(s.graph.requirements["req:a"].sources.len(), 2);
    assert_eq!(s.graph.criteria["crit:b"].requirement, "req:a");
}

#[test]
fn requirement_v5_single_source_loads_and_serializes_only_sources() {
    let r: Requirement = serde_norway::from_str("statement: Color is blue\nentities: []\nsource: {doc: a.md, section: /a, quote: Color is blue}\n").unwrap();
    assert_eq!(r.sources.len(), 1);
    let yaml = serde_norway::to_string(&r).unwrap();
    assert!(yaml.contains("sources:"));
    assert!(!yaml.contains("\nsource:"));
    assert_eq!(STORE_VERSION, 7);
    assert!(readable_version(6));
    assert!(!readable_version(5));
}

#[test]
fn requirement_consolidation_gc_derives_a_cluster() {
    let mut s = concept_store();
    s.graph.requirements.insert("req:a".into(), concept());
    s.graph.requirements.insert("req:b".into(), concept());
    let goals = crate::goals::kind("consolidate-requirements")
        .unwrap()
        .derive_goals(&s, &crate::gen::GenSettings::from_out(&s.out));
    assert_eq!(goals.len(), 1);
    assert!(!goals[0].mandatory);
    assert_eq!(goals[0].class, "gc");
    assert_eq!(goals[0].change["requirements"].as_array().unwrap().len(), 2);
}

#[test]
fn requirement_consolidation_hard_limit_and_dirty_second_source_block_readiness() {
    let mut s = concept_store();
    let mut r = concept();
    r.sources
        .push(mention("b.md", "/b", "The background uses #1E40AF."));
    for i in 0..crate::limits::REQUIREMENT_CLUSTER_HARD {
        s.graph
            .requirements
            .insert(format!("req:color-{}", i), r.clone());
    }
    let goals = crate::goals::kind("consolidate-requirements")
        .unwrap()
        .derive_goals(&s, &crate::gen::GenSettings::from_out(&s.out));
    assert!(goals[0].mandatory);
    let mut batch = RecordBatch::new(1);
    batch.push(
        0,
        CHANGE_SECTION_DIRTY,
        "b.md#/b",
        "section",
        serde_json::json!({"doc":"b.md", "section":"/b"}),
    );
    for record in batch.records() {
        s.status.record_change(record.clone());
    }
    let project = crate::project::Project {
        out: s.out.clone(),
        ..Default::default()
    };
    let board = crate::board::Board::derive(&s, &project, &crate::control::Control::default());
    assert!(!crate::goals::kind("consolidate-requirements")
        .unwrap()
        .ready(&goals[0], &board)
        .is_ready());
}

#[test]
fn requirement_obligation_edit_invalidates_review_hash_without_source_edit() {
    let mut s = concept_store();
    s.graph.requirements.insert("req:color".into(), concept());
    let hash = v5::entity_fact_hash(&s, "ent:cart");
    s.graph.requirements.get_mut("req:color").unwrap().statement =
        "The background uses red.".into();
    assert_ne!(hash, v5::entity_fact_hash(&s, "ent:cart"));
}
