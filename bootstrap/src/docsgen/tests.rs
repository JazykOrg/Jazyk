use super::links::{github_anchor, relink_quote};
use super::*;
use crate::model::*;
use crate::vfs::VPathExt;
use std::collections::BTreeMap;

fn gs(out: &std::path::Path) -> GenSettings {
    GenSettings {
        configured: true,
        deliverable: out.join("product"),
        worker: "agentic".into(),
        code: Vec::new(),
    }
}

// Mirrors docs/consumers/docsgen.md#the-requirements-document and #glossary: a
// quote's relative link resolves from the page, and a section link anchors on the
// heading's own slug, never the path.
#[test]
fn quoted_links_relink_and_section_anchors_take_the_heading() {
    let mut s = Store::default();
    let out = std::env::temp_dir()
        .join(format!("jazyk-docsgen-links-{}", crate::sys::process_id()))
        .join(".jazyk");
    crate::vfs::create_dir_all(&out).unwrap();
    s.out = out.clone();
    assert_eq!(
        relink_quote(
            "- [User Management](./subsystems/user.md)",
            "docs/backend.md",
            &s
        ),
        "- [User Management](../../docs/subsystems/user.md)"
    );
    assert_eq!(
        relink_quote(
            "see [the spec](https://x.org/a.md) and [top](#top)",
            "docs/backend.md",
            &s
        ),
        "see [the spec](https://x.org/a.md) and [top](../../docs/backend.md#top)"
    );
    assert_eq!(github_anchor("main.ts"), "maints");
    assert_eq!(github_anchor("API Endpoints"), "api-endpoints");
    let text = "# main.ts\n\nThe entry point.\n";
    s.docs.insert(
        "docs/main.js.md".into(),
        DocRecord {
            content_hash: hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage: BTreeMap::new(),
        },
    );
    s.graph.entities.insert(
        "ent:sort".into(),
        Entity {
            name: "Sort".into(),
            mentions: vec![SourceRef {
                doc: "docs/main.js.md".into(),
                section: "/main-ts".into(),
                quote: "The entry point.".into(),
            }],
            ..Default::default()
        },
    );
    s.graph.entities.insert(
        "ent:backend".into(),
        Entity {
            name: "Backend".into(),
            mentions: vec![SourceRef {
                doc: "docs/main.md".into(),
                section: "/warehouse-erp/backend".into(),
                quote: "the backend".into(),
            }],
            ..Default::default()
        },
    );
    let dir = out.join("docsgen");
    crate::vfs::create_dir_all(&dir).unwrap();
    write_reports(&s, &dir);
    let glossary = crate::vfs::read_to_string(dir.join("glossary.md")).unwrap();
    assert!(
        glossary.contains("[docs/main.md#/warehouse-erp/backend](../../docs/main.md#backend)"),
        "{}",
        glossary
    );
    assert!(
        glossary.contains("[docs/main.js.md#/main-ts](../../docs/main.js.md#maints)"),
        "{}",
        glossary
    );
    crate::vfs::remove_dir_all(out.parent().unwrap()).ok();
}

#[test]
fn renders_and_prunes() {
    let out = std::env::temp_dir().join(format!("jazyk-docsgen-test-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    s.graph.entities.insert(
        "ent:cart".into(),
        Entity {
            name: "Cart".into(),
            definition: Some("holds items".into()),
            mentions: vec![SourceRef {
                doc: "shop.md".into(),
                section: "/shop".into(),
                quote: "the Cart".into(),
            }],
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
                quote: "holds\nitems".into(),
            }),
            ..Default::default()
        },
    );
    // A quote that says what its statement says, once "shall" and the verb's
    // ending are set aside, is omitted (docs/consumers/docsgen.md#the-requirements-document).
    s.graph.requirements.insert(
        "req:shop-2".into(),
        Requirement {
            statement: "The Cart notifies the Customer when it empties.".into(),
            entities: vec!["ent:cart".into()],
            edges: vec![],
            source: Some(SourceRef {
                doc: "shop.md".into(),
                section: "/shop".into(),
                quote: "The Cart shall notify the Customer when it empties.".into(),
            }),
            ..Default::default()
        },
    );
    // A stale file for an entity that no longer exists must be pruned.
    crate::vfs::create_dir_all(out.join("docsgen")).ok();
    crate::vfs::write(out.join("docsgen/ghost.md"), "old").ok();
    // A stale card and a stale diagram page are pruned with it.
    crate::vfs::create_dir_all(out.join("docsgen/entities")).ok();
    crate::vfs::write(out.join("docsgen/entities/ghost.md"), "old").ok();
    crate::vfs::create_dir_all(out.join("docsgen/diagrams/class")).ok();
    crate::vfs::write(out.join("docsgen/diagrams/class/ghost.md"), "old").ok();
    // The document, the card, and the three reports: five pages.
    let n = write_all(&s, &gs(&out));
    assert_eq!(n, 5);
    let doc = crate::vfs::read_to_string(out.join("docsgen/cart.md")).unwrap();
    assert!(doc.contains("# Cart"));
    // A one-member scope has no level page: the card's breadcrumb names it plain.
    let card = crate::vfs::read_to_string(out.join("docsgen/entities/cart.md")).unwrap();
    assert!(card.contains("## Sits in\n\nPublic › Cart\n"), "{}", card);
    assert!(doc.contains("req:shop-1"));
    assert!(
        doc.contains("> holds items"),
        "quote is whitespace-normalized: {}",
        doc
    );
    assert!(doc.contains("[card](./entities/cart.md)"), "{}", doc);
    assert!(
        !doc.contains("> The Cart shall notify the Customer"),
        "a quote equal to its statement but for the modal is omitted: {}",
        doc
    );
    assert!(!out.join("docsgen/ghost.md").vexists());
    assert!(!out.join("docsgen/entities/ghost.md").vexists());
    assert!(!out.join("docsgen/diagrams/class/ghost.md").vexists());
    let card = crate::vfs::read_to_string(out.join("docsgen/entities/cart.md")).unwrap();
    assert!(
        card.contains("# Cart\n\n`ent:cart` · holds items\n"),
        "{}",
        card
    );
    assert!(card.contains("## Inside\n\na leaf\n"), "{}", card);
    assert!(
        card.contains("- [Requirements document](../cart.md) (2 requirements)"),
        "{}",
        card
    );
}

// The showcase graph: an entity page embeds the scope class view, the containing
// system's component view, its own state view, and the flow view naming it, each
// as a relative image link into ../diagrams/, with captions cross-linking the
// member pages. Mirrors docs/consumers/docsgen.md#diagrams-on-entity-pages.
#[test]
fn showcase_page_embeds_images_and_cross_links() {
    let out = std::env::temp_dir().join(format!(
        "jazyk-docsgen-showcase-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    crate::vfs::create_dir_all(&out).unwrap();
    let mut s = crate::derive::tests::showcase_store();
    s.out = out.clone();
    let mut batch = crate::store::RecordBatch::new(1);
    crate::derive::recompute(&mut s, "g1", &mut batch);
    write_all(&s, &gs(&out));

    let page = crate::vfs::read_to_string(out.join("docsgen/order.md")).unwrap();
    // The images, in the stated order: the level neighborhood (the parent's level
    // view, order-service holding four children; the order itself holds none), own
    // state view, the flow view naming the order. The scope root's level view and
    // the shop's belong to the pages of their members, not to the order's.
    let level = crate::derive::level_view_id(&s, "ent:order-service").unwrap();
    let level_svg = format!("](../diagrams/{}.svg)", &level["view:".len()..]);
    assert!(page.contains(&level_svg), "{}", page);
    assert!(
        !page.contains("](../diagrams/component/public.svg)"),
        "{}",
        page
    );
    assert!(
        !page.contains("](../diagrams/component/shop.svg)"),
        "{}",
        page
    );
    assert!(page.contains("](../diagrams/state/order.svg)"), "{}", page);
    // Flows are listed under their own heading, never embedded.
    assert!(
        !page.contains("](../diagrams/usecase/shop.svg)"),
        "{}",
        page
    );
    assert!(
        page.contains("## Flows\n")
            && page.contains("[page](./diagrams/usecase/shop.md) `view:usecase/shop`"),
        "{}",
        page
    );
    let level_pos = page.find(&level_svg).unwrap();
    let state_pos = page.find("../diagrams/state/order.svg").unwrap();
    let flow_pos = page.find("## Flows").unwrap();
    assert!(level_pos < state_pos && state_pos < flow_pos);
    // The state caption lists the states; the level caption links the drawn
    // members to their cards and the view to its diagram page.
    assert!(page.contains("3 states: placed, paid, held"), "{}", page);
    assert!(page.contains("(./entities/checkout-api.md)"), "{}", page);
    assert!(!page.contains("](./checkout-api.md)"), "{}", page);
    assert!(
        page.contains(&format!(
            "[page](./diagrams/{}.md)",
            &level["view:".len()..]
        )),
        "{}",
        page
    );
    // The header links the card; the containment line keeps linking documents.
    assert!(page.contains("[card](./entities/order.md)"), "{}", page);
    assert!(
        page.contains("Parent: [Order Service](./order-service.md)"),
        "{}",
        page
    );
    assert!(
        page.contains(&format!(
            "[source](../diagrams/{}.puml)",
            &level["view:".len()..]
        )),
        "{}",
        page
    );
    // The rendered files exist, so the links resolve.
    assert!(out.join("diagrams/state/order.svg").vexists());

    // The index embeds the structural level views as images (no mermaid), the
    // scope root's (view:component/public: shop is a system, customer an actor)
    // and the shop's, lists every view, and cross-links the pages.
    let idx = crate::vfs::read_to_string(out.join("docsgen/index.md")).unwrap();
    assert!(
        idx.contains("](../diagrams/component/public.svg)"),
        "{}",
        idx
    );
    assert!(!idx.contains("](../diagrams/class/public.svg)"), "{}", idx);
    assert!(idx.contains("](../diagrams/component/shop.svg)"), "{}", idx);
    assert!(!idx.contains("mermaid"), "{}", idx);
    assert!(idx.contains("- [Order](./order.md) `ent:order`"), "{}", idx);
    assert!(idx.contains("`view:usecase/shop`"), "{}", idx);
    crate::vfs::remove_dir_all(&out).ok();
}

// The showcase graph's level pages: the scope root and every node with a level
// view get a page under levels/, a leaf and a one-child node get none, the
// breadcrumb walks the containment chain up to the root page, the members link
// down, and the index links the root level page.
// Mirrors docs/consumers/docsgen.md#level-pages.
#[test]
fn level_pages_nest_as_the_containment_tree() {
    let out =
        std::env::temp_dir().join(format!("jazyk-docsgen-levels-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    crate::vfs::create_dir_all(&out).unwrap();
    let mut s = crate::derive::tests::showcase_store();
    s.out = out.clone();
    let mut batch = crate::store::RecordBatch::new(1);
    crate::derive::recompute(&mut s, "g1", &mut batch);
    // A stale page for a node that lost its level is pruned.
    crate::vfs::create_dir_all(out.join("docsgen/levels")).unwrap();
    crate::vfs::write(out.join("docsgen/levels/ghost.md"), "old").unwrap();
    write_all(&s, &gs(&out));
    let levels = out.join("docsgen/levels");

    // A page per level: the scope root, the shop (two services), the order
    // service (four children). No page for the inventory service (one child)
    // nor for a leaf.
    assert!(levels.join("scope-public.md").vexists());
    assert!(levels.join("shop.md").vexists());
    assert!(levels.join("order-service.md").vexists());
    assert!(!levels.join("inventory-service.md").vexists());
    assert!(!levels.join("checkout-api.md").vexists());
    assert!(!levels.join("ghost.md").vexists());

    // The order service's page: the breadcrumb chain root → shop → node (the node
    // unlinked), the header with the entity page link, the level view embedded
    // with paths one directory up, the members linked to their entity pages.
    let page = crate::vfs::read_to_string(levels.join("order-service.md")).unwrap();
    assert!(
        page.starts_with("[Public](./scope-public.md) › [Shop](./shop.md) › Order Service\n"),
        "{}",
        page
    );
    assert!(page.contains("# Order Service\n"), "{}", page);
    assert!(
        page.contains("[card](../entities/order-service.md) · [entity page](../order-service.md)"),
        "{}",
        page
    );
    let level = crate::derive::level_view_id(&s, "ent:order-service").unwrap();
    let rel = &level["view:".len()..];
    assert!(
        page.contains(&format!("](../../diagrams/{}.svg)", rel)),
        "{}",
        page
    );
    assert!(
        page.contains(&format!("[source](../../diagrams/{}.puml)", rel)),
        "{}",
        page
    );
    assert!(
        page.contains("## Members\n\n- [checkout API](../entities/checkout-api.md) · «interface»"),
        "{}",
        page
    );
    assert!(
        !page.contains("[level](./checkout-api.md)"),
        "a leaf has no link down: {}",
        page
    );

    // The shop's page links down to the order service's level page with its child
    // count and not to the inventory service's; the customer, an outside entity
    // the lifted edge brings into the view, is not a member.
    let shop = crate::vfs::read_to_string(levels.join("shop.md")).unwrap();
    assert!(
        shop.starts_with("[Public](./scope-public.md) › Shop\n"),
        "{}",
        shop
    );
    assert!(
        shop.contains("- [Order Service](../entities/order-service.md) · «service» · [level](./order-service.md) (4 children)"),
        "{}",
        shop
    );
    assert!(
        shop.contains("- [Inventory Service](../entities/inventory-service.md) · «service»\n"),
        "{}",
        shop
    );
    assert!(
        !shop.contains("- [Customer](../entities/customer.md)"),
        "{}",
        shop
    );
    assert!(
        shop.contains("](../../diagrams/component/shop.svg)"),
        "{}",
        shop
    );

    // The root page: unlinked breadcrumb, the scope's level view, the shop as a
    // member with its link down, the flow view lifted into the root level.
    let root = crate::vfs::read_to_string(levels.join("scope-public.md")).unwrap();
    assert!(
        root.starts_with("Public\n\n# Public\n\n`scope:public` · scope `public`"),
        "{}",
        root
    );
    assert!(
        root.contains("](../../diagrams/component/public.svg)"),
        "{}",
        root
    );
    assert!(
        root.contains("- [Shop](../entities/shop.md) · «system» · [level](./shop.md) (2 children)"),
        "{}",
        root
    );
    assert!(root.contains("`view:usecase/shop`"), "{}", root);
    let structural = root.find("component/public.svg").unwrap();
    let flow = root.find("usecase/shop").unwrap();
    assert!(
        structural < flow,
        "the structural view embeds first: {}",
        root
    );

    // The index links the root level page from the scope's caption.
    let idx = crate::vfs::read_to_string(out.join("docsgen/index.md")).unwrap();
    assert!(
        idx.contains("level page: [Public](./levels/scope-public.md)"),
        "{}",
        idx
    );
    crate::vfs::remove_dir_all(&out).ok();
}

// The showcase graph's walk: a card per entity under entities/ listing one level
// in every direction with every entity link pointing at a card, and a page per
// view under diagrams/<kind>/ with the level, the legend, the steps, and the
// views around. Mirrors docs/consumers/docsgen.md#entity-cards and
// docs/consumers/docsgen.md#diagram-pages.
#[test]
fn cards_and_diagram_pages_walk_the_graph() {
    let out = std::env::temp_dir().join(format!("jazyk-docsgen-walk-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    crate::vfs::create_dir_all(&out).unwrap();
    let mut s = crate::derive::tests::showcase_store();
    s.out = out.clone();
    let mut batch = crate::store::RecordBatch::new(1);
    crate::derive::recompute(&mut s, "g1", &mut batch);
    write_all(&s, &gs(&out));
    let cards = out.join("docsgen/entities");
    let pages = out.join("docsgen/diagrams");

    // The order service's card: the header, the breadcrumb up (the scope root to
    // its level page, the shop to its card), the context and inside views embedded
    // two directories up with card links in the captions, the relationships,
    // the flow at the shop's level, the sibling, the children, the long reads.
    let card = crate::vfs::read_to_string(cards.join("order-service.md")).unwrap();
    assert!(
        card.starts_with("# Order Service\n\n`ent:order-service` · «service»"),
        "{}",
        card
    );
    assert!(
        card.contains("## Sits in\n\n[Public](../levels/scope-public.md) › [Shop](./shop.md) › Order Service\n"),
        "{}",
        card
    );
    let context = card.find("## In context").unwrap();
    let inside = card.find("## Inside").unwrap();
    assert!(context < inside);
    assert!(
        card[context..inside].contains("](../../diagrams/component/shop.svg)"),
        "{}",
        card
    );
    assert!(
        card[context..inside].contains("[Inventory Service](./inventory-service.md)"),
        "{}",
        card
    );
    assert!(
        card[context..inside].contains("· [page](../diagrams/component/shop.md)"),
        "{}",
        card
    );
    assert!(
        card[inside..].contains("](../../diagrams/component/order-service.svg)"),
        "{}",
        card
    );
    assert!(
        card.contains("- dependency → [Inventory Service](./inventory-service.md) · 1 requirement"),
        "{}",
        card
    );
    // The three reports the index links.
    let out_dir = out.join("docsgen");
    let glossary = crate::vfs::read_to_string(out_dir.join("glossary.md")).unwrap();
    assert!(glossary.contains("- **Customer**"), "{}", glossary);
    assert!(
        glossary.contains("[card](./entities/customer.md)"),
        "{}",
        glossary
    );
    assert!(out_dir.join("fragmentation.md").vexists());
    let staleness = crate::vfs::read_to_string(out_dir.join("staleness.md")).unwrap();
    assert!(staleness.starts_with("# Staleness"), "{}", staleness);
    let index = crate::vfs::read_to_string(out_dir.join("index.md")).unwrap();
    assert!(index.contains("[glossary](./glossary.md)"), "{}", index);
    assert!(
        card.contains("[use case](../diagrams/usecase/shop-shop.md) · [sequence](../diagrams/sequence/shop-shop.md)"),
        "{}",
        card
    );
    assert!(
        card.contains("## Siblings\n\n- [Inventory Service](./inventory-service.md)\n"),
        "{}",
        card
    );
    assert!(card.contains("## Children\n\n"), "{}", card);
    assert!(card.contains("- [Order](./order.md)\n"), "{}", card);
    assert!(
        card.contains("- [Requirements document](../order-service.md) ("),
        "{}",
        card
    );
    assert!(
        card.contains("- Level page of [Shop](../levels/shop.md)\n"),
        "{}",
        card
    );
    assert!(
        card.contains("- Own level page: [Order Service](../levels/order-service.md)\n"),
        "{}",
        card
    );
    // No link on a card points at a requirements document except the long reads.
    assert!(!card.contains("](../order-service.md#"), "{}", card);
    assert!(!card.contains(".svg]]"), "{}", card);

    // A leaf's card: a leaf inside, no children, its context the parent's level.
    let leaf = crate::vfs::read_to_string(cards.join("order-item.md")).unwrap();
    assert!(leaf.contains("## Inside\n\na leaf\n"), "{}", leaf);
    assert!(!leaf.contains("## Children"), "{}", leaf);
    assert!(
        leaf.contains("](../../diagrams/component/order-service.svg)"),
        "{}",
        leaf
    );
    assert!(
        leaf.contains("[Public](../levels/scope-public.md) › [Shop](./shop.md) › [Order Service](./order-service.md) › Order Item\n"),
        "{}",
        leaf
    );

    // The shop's level view page: the level line ends in the level page, the image
    // sits three directories up, the legend links every drawn entity to its card
    // and the ones with a level to their level view's page, and the views around
    // are the shop's flow views, the root's view above, the order service's below.
    let page = crate::vfs::read_to_string(pages.join("component/shop.md")).unwrap();
    assert!(
        page.starts_with("# Shop\n\n`view:component/shop` · component · "),
        "{}",
        page
    );
    assert!(
        page.contains("## Level\n\n[Public](../../levels/scope-public.md) › [Shop](../../entities/shop.md) · level page: [Shop](../../levels/shop.md)\n"),
        "{}",
        page
    );
    assert!(
        page.contains("](../../../diagrams/component/shop.svg)"),
        "{}",
        page
    );
    assert!(
        page.contains("[source](../../../diagrams/component/shop.puml)"),
        "{}",
        page
    );
    assert!(
        page.contains("- [Order Service](../../entities/order-service.md) · «service» · [level below](./order-service.md)\n"),
        "{}",
        page
    );
    assert!(
        page.contains("- [Inventory Service](../../entities/inventory-service.md) · «service»\n"),
        "{}",
        page
    );
    assert!(!page.contains("## Steps"), "{}", page);
    assert!(
        page.contains("(../usecase/shop-shop.md) `view:usecase/shop-shop`"),
        "{}",
        page
    );
    assert!(
        page.contains("- Above: [Public](./public.md) `view:component/public`"),
        "{}",
        page
    );
    assert!(
        page.contains(
            "- Below: [Order Service](./order-service.md) `view:component/order-service`"
        ),
        "{}",
        page
    );

    // The shop level's sequence page: the steps as drawn, each id linked to its
    // block in a requirements document, the ends linked to their cards.
    let seq = crate::vfs::read_to_string(pages.join("sequence/shop-shop.md")).unwrap();
    assert!(
        seq.contains("## Level\n\n[Public](../../levels/scope-public.md) › [Shop](../../entities/shop.md) · level page: [Shop](../../levels/shop.md)\n"),
        "{}",
        seq
    );
    assert!(
        seq.contains("## Steps\n\n- [`req:shop-1`](../../customer.md#reqshop-1) The customer submits the shopping cart through the checkout API. · [Customer](../../entities/customer.md) → [Order Service](../../entities/order-service.md)\n"),
        "{}",
        seq
    );
    assert!(
        seq.contains("- Same level: ") && seq.contains("`view:component/shop`"),
        "{}",
        seq
    );

    // The state view's page belongs to its machine, not to a level.
    let state = crate::vfs::read_to_string(pages.join("state/order.md")).unwrap();
    assert!(
        state.contains("## Level\n\nthe state machine of [Order](../../entities/order.md)\n"),
        "{}",
        state
    );
    crate::vfs::remove_dir_all(&out).ok();
}

// A pending ratification proposal renders as its prompt on the entity page, and
// the index groups proposals by target document.
// Mirrors docs/consumers/docsgen.md#ratification-proposals.
#[test]
fn ratification_proposal_renders_as_a_prompt() {
    let out =
        std::env::temp_dir().join(format!("jazyk-docsgen-ratify-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    s.graph.entities.insert(
        "ent:pricing".into(),
        Entity {
            name: "Pricing".into(),
            provenance: Some(Provenance::Derived {
                from: vec!["req:orders-3".into()],
                reasoning: "a separable concern".into(),
            }),
            ..Default::default()
        },
    );
    s.graph.diagnostics.insert(
        "diag:ratification-pending-1".into(),
        Diagnostic {
            rule: "ratification-pending".into(),
            severity: "warning".into(),
            subjects: vec!["ent:pricing".into()],
            message: "ent:pricing is derived and no document states it.".into(),
            reasoning: Some("req:orders-3 separates pricing".into()),
            lifecycle: "open".into(),
            triage: None,
            prompt: Some(DiagnosticPrompt {
                question: "Should docs/orders.md state the pricing module?".into(),
                options: vec![
                    PromptOption {
                        label: "Insert into docs/orders.md /orders/service".into(),
                        edit: Some(SuggestedEdit {
                            doc: "docs/orders.md".into(),
                            section: "/orders/service".into(),
                            old_text: String::new(),
                            new_text: "The order service contains a pricing module.".into(),
                            facts: Vec::new(),
                        }),
                        answer: None,
                    },
                    PromptOption {
                        label: "Retract".into(),
                        edit: None,
                        answer: Some("retract".into()),
                    },
                ],
                freeform: true,
            }),
            answer: None,
            created: None,
            updated: None,
        },
    );
    write_all(&s, &gs(&out));
    let page = crate::vfs::read_to_string(out.join("docsgen/pricing.md")).unwrap();
    assert!(page.contains("## Proposals"), "{}", page);
    assert!(
        page.contains("> The order service contains a pricing module."),
        "{}",
        page
    );
    assert!(
        page.contains("Target: `docs/orders.md#/orders/service`"),
        "{}",
        page
    );
    assert!(page.contains("- Answer: Retract"), "{}", page);
    assert!(
        page.contains("This entity is derived"),
        "the header names the provenance: {}",
        page
    );
    let idx = crate::vfs::read_to_string(out.join("docsgen/index.md")).unwrap();
    assert!(idx.contains("## Ratification"), "{}", idx);
    assert!(idx.contains("### `docs/orders.md`"), "{}", idx);
    crate::vfs::remove_dir_all(&out).ok();
}
