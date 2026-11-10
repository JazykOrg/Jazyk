use super::*;
use crate::store::Store;

mod graph;
mod views;

fn src(doc: &str, sec: &str, q: &str) -> SourceRef {
    SourceRef {
        doc: doc.into(),
        section: sec.into(),
        quote: q.into(),
    }
}

fn edge(a: &str, b: &str, t: &str, card: Option<&str>) -> ReqEdge {
    ReqEdge {
        a: a.into(),
        b: b.into(),
        rel_type: Some(t.into()),
        cardinality: card.map(String::from),
    }
}

fn attr(name: &str, ty: Option<&str>, value: Option<&str>) -> Attribute {
    Attribute {
        name: name.into(),
        r#type: ty.map(String::from),
        value: value.map(String::from),
        provenance: Provenance::Quote(src("shop.md", "/shop", "attributes")),
    }
}

fn behavior() -> Facet {
    Facet {
        facet: "behavior".into(),
        reasoning: "a step".into(),
        measure: None,
    }
}

// The showcase graph: one small shop that exercises every emitter
// (docs/compiler/diagrams.md#the-emitters), in an in-memory store with the one
// document its quotes cite.
pub(crate) fn showcase_store() -> Store {
    let mut s = Store::default();
    let text = "# Shop\n\nThe shop.\n\n## Checkout\n\ncheckout\n\n## Orders\n\norders\n\n## Examples\n\nexamples\n";
    s.docs.insert(
        "shop.md".into(),
        DocRecord {
            content_hash: hash_hex(text),
            sections: md::parse_sections(text),
            coverage: BTreeMap::new(),
        },
    );
    let ent = |name: &str,
               stereotype: Option<&str>,
               parent: Option<&str>,
               attributes: Vec<Attribute>| Entity {
        name: name.into(),
        stereotype: stereotype.map(String::from),
        parent: parent.map(String::from),
        attributes,
        ..Default::default()
    };
    let entities = [
        (
            "ent:shop",
            ent(
                "Shop",
                Some("system"),
                None,
                vec![attr("region", None, Some("EU"))],
            ),
        ),
        (
            "ent:order-service",
            ent("Order Service", Some("service"), Some("ent:shop"), vec![]),
        ),
        (
            "ent:inventory-service",
            ent(
                "Inventory Service",
                Some("service"),
                Some("ent:shop"),
                vec![],
            ),
        ),
        (
            "ent:checkout-api",
            ent(
                "checkout API",
                Some("interface"),
                Some("ent:order-service"),
                vec![],
            ),
        ),
        (
            "ent:stock-api",
            ent(
                "stock API",
                Some("interface"),
                Some("ent:inventory-service"),
                vec![],
            ),
        ),
        (
            "ent:customer",
            ent(
                "Customer",
                Some("actor"),
                None,
                vec![attr("tier", Some("string"), None)],
            ),
        ),
        (
            "ent:shopping-cart",
            ent(
                "Shopping Cart",
                None,
                Some("ent:order-service"),
                vec![attr("items", None, None), attr("currency", None, None)],
            ),
        ),
        (
            "ent:order",
            ent(
                "Order",
                None,
                Some("ent:order-service"),
                vec![attr("total", None, None), attr("currency", None, None)],
            ),
        ),
        (
            "ent:order-item",
            ent("Order Item", None, Some("ent:order-service"), vec![]),
        ),
        (
            "ent:ana",
            ent("Ana", None, None, vec![attr("tier", None, Some("gold"))]),
        ),
        (
            "ent:anas-cart",
            ent(
                "Ana's cart",
                None,
                None,
                vec![
                    attr("items", None, Some("3")),
                    attr("currency", None, Some("EUR")),
                ],
            ),
        ),
    ];
    for (id, e) in entities {
        s.graph.entities.insert(id.into(), e);
    }
    let req = |statement: &str,
               section: &str,
               entities: &[&str],
               edges: Vec<ReqEdge>,
               transition: Option<Transition>,
               facets: Vec<Facet>| Requirement {
        statement: statement.into(),
        entities: entities.iter().map(|e| e.to_string()).collect(),
        edges,
        transition,
        facets,
        source: Some(src("shop.md", section, statement)),
        ..Default::default()
    };
    let reqs = [
        (
            "req:shop-1",
            req(
                "The customer submits the shopping cart through the checkout API.",
                "/shop/checkout",
                &["ent:customer", "ent:shopping-cart", "ent:checkout-api"],
                vec![
                    edge("ent:customer", "ent:checkout-api", "dependency", None),
                    edge("ent:customer", "ent:shopping-cart", "association", None),
                ],
                None,
                vec![behavior()],
            ),
        ),
        (
            "req:shop-2",
            req(
                "The order service provides the checkout API.",
                "/shop",
                &["ent:order-service", "ent:checkout-api"],
                vec![edge(
                    "ent:order-service",
                    "ent:checkout-api",
                    "realization",
                    None,
                )],
                None,
                vec![],
            ),
        ),
        (
            "req:shop-3",
            req(
                "When checkout succeeds, the order service reserves stock through the stock API.",
                "/shop/checkout",
                &["ent:customer", "ent:order-service", "ent:stock-api"],
                vec![edge(
                    "ent:order-service",
                    "ent:stock-api",
                    "dependency",
                    None,
                )],
                None,
                vec![behavior()],
            ),
        ),
        (
            "req:shop-4",
            req(
                "The inventory service provides the stock API.",
                "/shop",
                &["ent:inventory-service", "ent:stock-api"],
                vec![edge(
                    "ent:inventory-service",
                    "ent:stock-api",
                    "realization",
                    None,
                )],
                None,
                vec![],
            ),
        ),
        (
            "req:shop-5",
            req(
                "An order carries a total and a currency.",
                "/shop/orders",
                &["ent:order"],
                vec![],
                None,
                vec![],
            ),
        ),
        (
            "req:shop-6",
            req(
                "A shopping cart holds one or more order items.",
                "/shop",
                &["ent:shopping-cart", "ent:order-item"],
                vec![edge(
                    "ent:shopping-cart",
                    "ent:order-item",
                    "composition",
                    Some("1..*"),
                )],
                None,
                vec![],
            ),
        ),
        (
            "req:shop-7",
            req(
                "When payment succeeds, the order becomes paid.",
                "/shop/orders",
                &["ent:customer", "ent:order"],
                vec![],
                Some(Transition {
                    subject: "ent:order".into(),
                    from: "placed".into(),
                    to: "paid".into(),
                    trigger: Some("payment succeeds".into()),
                    guard: None,
                }),
                vec![behavior()],
            ),
        ),
        (
            "req:shop-8",
            req(
                "If payment is declined, then the order is held for review.",
                "/shop/orders",
                &["ent:customer", "ent:order"],
                vec![],
                Some(Transition {
                    subject: "ent:order".into(),
                    from: "placed".into(),
                    to: "held".into(),
                    trigger: Some("payment declined".into()),
                    guard: None,
                }),
                vec![Facet {
                    facet: "failure-mode".into(),
                    reasoning: "the declined branch".into(),
                    measure: None,
                }],
            ),
        ),
        (
            "req:shop-9",
            req(
                "Ana, a gold-tier customer, keeps 3 items in her cart, priced in EUR.",
                "/shop/examples",
                &[
                    "ent:ana",
                    "ent:customer",
                    "ent:anas-cart",
                    "ent:shopping-cart",
                ],
                vec![
                    edge("ent:ana", "ent:customer", "instantiation", None),
                    edge("ent:anas-cart", "ent:shopping-cart", "instantiation", None),
                    edge("ent:ana", "ent:anas-cart", "association", None),
                ],
                None,
                vec![],
            ),
        ),
        (
            "req:shop-10",
            req(
                "The shop shall confirm checkout within 2 seconds.",
                "/shop",
                &["ent:shop"],
                vec![],
                None,
                vec![Facet {
                    facet: "quality".into(),
                    reasoning: "bounded".into(),
                    measure: Some("2 seconds".into()),
                }],
            ),
        ),
        (
            "req:shop-11",
            req(
                "The shop is deployed in the EU region.",
                "/shop",
                &["ent:shop"],
                vec![],
                None,
                vec![],
            ),
        ),
    ];
    for (id, r) in reqs {
        s.graph.requirements.insert(id.into(), r);
    }
    s
}

// A synthetic store of a given size: twenty documents linked in a chain from the
// root, each with ten sections; a containment tree of fan-out eight over a wide
// top level; names from a small vocabulary, every fiftieth entity a lookalike of
// its neighbor across documents; requirements naming two entities each with an
// edge, a behavior facet on every other, a transition on every tenth.
pub(crate) fn synthetic_store(entities: usize, requirements: usize) -> Store {
    const WORDS: [&str; 24] = [
        "order", "service", "cart", "customer", "stock", "api", "gateway", "ledger", "invoice",
        "payment", "shipment", "account", "catalog", "price", "report", "audit", "session",
        "token", "queue", "worker", "cache", "index", "policy", "region",
    ];
    let docs = 20;
    let statement = |j: usize| {
        format!(
            "Statement {} of ent:e{} toward ent:e{}.",
            j,
            j % entities,
            (j * 7 + 3) % entities
        )
    };
    let mut s = Store::default();
    for k in 0..docs {
        let mut text = format!(
            "# D{}\n\nThe document, see [next](d{}.md).\n",
            k,
            (k + 1) % docs
        );
        for j in 0..10 {
            text.push_str(&format!(
                "\n## S{}\n\nThe body of section {} in {}.\n",
                j, j, k
            ));
            // The statements quoted from this section, so every anchor locates.
            for r in (0..requirements).filter(|r| r % docs == k && (r / docs) % 10 == j) {
                text.push_str(&statement(r));
                text.push('\n');
            }
        }
        let sections = md::parse_sections(&text);
        let coverage = sections
            .keys()
            .map(|r| {
                (
                    r.clone(),
                    Coverage {
                        state: "covered".into(),
                        note: None,
                        claimed_by: Some("g1".into()),
                    },
                )
            })
            .collect();
        s.docs.insert(
            format!("d{}.md", k),
            DocRecord {
                content_hash: hash_hex(&text),
                sections,
                coverage,
            },
        );
    }
    s.status.roots = vec!["d0.md".into()];
    s.status.generation = 1;
    for i in 0..entities {
        let name = if i % 50 < 2 {
            format!("{} concept", WORDS[(i / 50) % WORDS.len()])
        } else {
            format!(
                "{} {}{}",
                WORDS[i % WORDS.len()],
                WORDS[(i / WORDS.len()) % WORDS.len()],
                i
            )
        };
        let parent = if i < 64 {
            None
        } else {
            Some(format!("ent:e{}", (i - 64) / 8))
        };
        let doc = format!("d{}.md", i % docs);
        let section = format!("/d{}/s{}", i % docs, (i / docs) % 10);
        s.graph.entities.insert(
            format!("ent:e{}", i),
            Entity {
                name: name.clone(),
                stereotype: if i % 7 == 0 {
                    Some("service".into())
                } else {
                    None
                },
                parent,
                mentions: vec![src(&doc, &section, &name)],
                ..Default::default()
            },
        );
    }
    let types = ["dependency", "association", "composition", "realization"];
    for j in 0..requirements {
        let a = format!("ent:e{}", j % entities);
        let b = format!("ent:e{}", (j * 7 + 3) % entities);
        let doc = format!("d{}.md", j % docs);
        let section = format!("/d{}/s{}", j % docs, (j / docs) % 10);
        let statement = statement(j);
        let transition = (j % 10 == 0).then(|| Transition {
            subject: a.clone(),
            from: "idle".into(),
            to: format!("state{}", j % 3),
            trigger: Some("event".into()),
            guard: None,
        });
        s.graph.requirements.insert(
            format!("req:r{:05}", j),
            Requirement {
                statement: statement.clone(),
                entities: vec![a.clone(), b.clone()],
                edges: if a == b {
                    vec![]
                } else {
                    vec![edge(&a, &b, types[j % 4], None)]
                },
                transition,
                facets: if j % 2 == 0 { vec![behavior()] } else { vec![] },
                source: Some(src(&doc, &section, &statement)),
                ..Default::default()
            },
        );
    }
    s
}
