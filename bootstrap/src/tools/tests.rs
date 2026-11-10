use super::*;
use std::collections::BTreeMap;

mod anchors_coverage;
mod args;
mod catalog;
mod claims;
mod diagnostics;
mod entities;
mod files;
mod guard;
mod ledger;
mod pairs;
mod read;
mod requirements;
mod views;

fn session() -> ToolSession {
    let mut s = Store::default();
    let text = "# Shop\nintro text\n\n## Cart\nThe Shopping Cart holds items a Customer intends to buy.\nA Gadget and a Line sit beside a Sprocket, the Departments, their Employee Handbook, and their Employee Badge.\n";
    s.docs.insert(
        "shop.md".into(),
        DocRecord {
            content_hash: hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage: BTreeMap::new(),
        },
    );
    s.graph.entities.insert(
        "ent:customer".into(),
        Entity {
            name: "Customer".into(),
            definition: Some("a person who buys".into()),
            ..Default::default()
        },
    );
    ToolSession::new(
        s,
        reconcile_scope("shop.md", &["/shop", "/shop/cart"], &[]),
        64,
        24_000,
    )
}

// A batch of reconcile-section goals, one per section, with the stale anchors on
// each section's goal.
fn reconcile_scope(doc: &str, sections: &[&str], stale: &[&str]) -> WorkScope {
    let goals: Vec<Goal> = sections
        .iter()
        .map(|sec| Goal {
            id: format!("g:reconcile-section:{}#{}", doc, sec),
            kind: "reconcile-section".into(),
            mandatory: true,
            target: format!("{}#{}", doc, sec),
            change: json!({"staleAnchors": stale}),
            ..Default::default()
        })
        .collect();
    WorkScope::for_batch("b0-1", &goals)
}

fn plain(name: &str) -> Entity {
    Entity {
        name: name.into(),
        ..Default::default()
    }
}

fn under(name: &str, parent: &str) -> Entity {
    Entity {
        name: name.into(),
        parent: Some(parent.into()),
        ..Default::default()
    }
}

fn quoted_req(statement: &str, entities: &[&str], quote: &str) -> Requirement {
    Requirement {
        statement: statement.into(),
        entities: entities.iter().map(|e| e.to_string()).collect(),
        source: Some(SourceRef {
            doc: "shop.md".into(),
            section: "/shop/cart".into(),
            quote: quote.into(),
        }),
        ..Default::default()
    }
}

// A session whose snapshot holds a requirement quoting text the document no longer
// contains, listed as a stale anchor in the work scope.
fn session_with_stale_anchor() -> ToolSession {
    let mut s = Store::default();
    let text =
        "# Shop\nintro text\n\n## Cart\nThe Shopping Cart keeps items a Customer intends to buy.\n";
    s.docs.insert(
        "shop.md".into(),
        DocRecord {
            content_hash: hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage: BTreeMap::new(),
        },
    );
    s.graph.entities.insert(
        "ent:shopping-cart".into(),
        Entity {
            name: "Shopping Cart".into(),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:shop-1".into(),
        Requirement {
            statement: "The Shopping Cart shall hold items a Customer intends to buy.".into(),
            entities: vec!["ent:shopping-cart".into()],
            edges: Vec::new(),
            source: Some(SourceRef {
                doc: "shop.md".into(),
                section: "/shop/cart".into(),
                quote: "The Shopping Cart holds items a Customer intends to buy.".into(),
            }),
            ..Default::default()
        },
    );
    ToolSession::new(
        s,
        reconcile_scope("shop.md", &["/shop/cart"], &["req:shop-1"]),
        64,
        24_000,
    )
}

fn open_diag(rule: &str, subjects: &[&str]) -> Diagnostic {
    Diagnostic {
        rule: rule.into(),
        severity: "warning".into(),
        subjects: subjects.iter().map(|s| s.to_string()).collect(),
        message: "x".into(),
        reasoning: None,
        lifecycle: "open".into(),
        triage: None,
        prompt: None,
        answer: None,
        created: None,
        updated: None,
    }
}

fn one_goal(kind: &str, target: &str, change: Value) -> Vec<Goal> {
    vec![Goal {
        id: format!("g:{}:{}", kind, target),
        kind: kind.into(),
        mandatory: true,
        target: target.into(),
        change,
        ..Default::default()
    }]
}

// A pair batch over three requirements: a~b, a~c, and b~c, with a reconcile
// goal beside them for the wrong-kind case.
fn pair_session() -> ToolSession {
    let mut s = Store::default();
    for (id, statement) in [
        ("req:a", "The Cart shall hold items until checkout."),
        ("req:b", "The Cart shall show its item count."),
        ("req:c", "The Cart shall drop its items after one hour."),
    ] {
        s.graph
            .requirements
            .insert(id.into(), quoted_req(statement, &["ent:cart"], statement));
    }
    s.graph.entities.insert("ent:cart".into(), plain("Cart"));
    let pair = |a: &str, b: &str| Goal {
        id: format!("g:rejudge-pair:{}~{}", a, b),
        kind: "rejudge-pair".into(),
        mandatory: true,
        target: format!("{}~{}", a, b),
        ..Default::default()
    };
    let goals = vec![
        pair("req:a", "req:b"),
        pair("req:a", "req:c"),
        pair("req:b", "req:c"),
        Goal {
            id: "g:reconcile-section:shop.md#/shop".into(),
            kind: "reconcile-section".into(),
            mandatory: true,
            target: "shop.md#/shop".into(),
            ..Default::default()
        },
    ];
    ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000)
}
