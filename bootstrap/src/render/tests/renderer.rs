use super::*;
use crate::render::renderer::{
    adapt_component_for_crate, anchor_entities, attr_value, group_end, native_svg,
    native_svg_within, strip_element_links, xml_attr,
};
use crate::render::text::alias_of;

// The crate draws no actor and no undirected link in a component diagram without an
// interface; the in-process conversion adapts the standard source.
// Mirrors docs/compiler/diagrams.md#the-renderer.
#[test]
fn a_component_view_without_interfaces_draws_its_links_and_actor() {
    let puml = "@startuml\ncomponent \"Frontend\" as frontend [[../../docsgen/entities/frontend.md]]\ncomponent \"Backend\" as backend [[../../docsgen/entities/backend.md]]\ncomponent \"Product\" as product <<entity>> [[../../docsgen/entities/product.md]]\nactor \"Warehouse Staff\" as warehouse_staff [[../../docsgen/entities/warehouse-staff.md]]\nbackend -- frontend\nbackend -- product\nbackend -- warehouse_staff\n@enduml\n";
    let adapted = adapt_component_for_crate(&strip_element_links(puml).0);
    assert!(
        adapted.contains("component \"Warehouse Staff\" as warehouse_staff <<actor>>"),
        "{}",
        adapted
    );
    assert!(adapted.contains("backend --> frontend\n"), "{}", adapted);
    let svg = render_svg(puml).unwrap();
    assert!(
        svg.contains(">Warehouse Staff<"),
        "the actor is drawn with its name"
    );
    assert!(
        svg.matches("<path").count() >= 3,
        "three links drawn: {}",
        svg.matches("<path").count()
    );
    assert!(
        svg.contains("warehouse-staff.md"),
        "the actor keeps its link"
    );
    // A class or use case diagram passes through untouched.
    let uc = "@startuml\nactor \"A\" as a\nusecase \"U\" as u\na -- u\n@enduml\n";
    assert_eq!(adapt_component_for_crate(uc), uc);
}

#[test]
fn in_process_renderings_carry_anchors_for_links() {
    let s = level_tree();
    let href = "../../docsgen/entities/a.md";
    for kind in ["class", "component"] {
        let v = view(kind, "System", &["ent:a", "ent:b"]);
        let puml = emit(&s, &format!("view:{}/system", kind), &v);
        assert!(puml.contains("[["), "{}", puml);
        let svg = render_svg(&puml).unwrap();
        let anchor = format!(
            "<a href=\"{0}\" xlink:href=\"{0}\"><g class=\"entity\" data-qualified-name=\"a\"",
            href
        );
        assert_eq!(svg.matches(&anchor).count(), 1, "{}", svg);
        assert_eq!(svg.matches("<a ").count(), 2, "{}", svg);
        assert_eq!(svg.matches("</a>").count(), 2, "{}", svg);
        // The label survives the link: the crate alone would draw the alias.
        assert!(svg.contains(">A</text>"), "{}", svg);
        assert!(!svg.contains(">a</text>"), "{}", svg);
        assert!(!svg.contains("[["), "{}", svg);
        // The anchored svg still rasterizes.
        render_png(&svg).unwrap();
    }
}

#[test]
fn element_links_strip_by_alias_or_name_and_anchor_by_key() {
    let puml = "@startuml\nclass \"A b\" as a <<svc>> [[../class/a.svg]] {\n  x : int\n}\npackage \"P q\" [[../class/p.svg]]\nA -- B : see [[not a link]]\n@enduml\n";
    let (plain, links) = strip_element_links(puml);
    assert_eq!(
        plain,
        "@startuml\nclass \"A b\" as a <<svc>> {\n  x : int\n}\npackage \"P q\"\nA -- B : see [[not a link]]\n@enduml\n"
    );
    assert_eq!(
        links,
        vec![
            ("a".to_string(), "../class/a.svg".to_string()),
            ("P q".to_string(), "../class/p.svg".to_string())
        ]
    );
    // A key the crate drew nothing for keeps no anchor; nested groups close where
    // the entity's own group does.
    let svg = "<svg><g><g class=\"entity\" data-qualified-name=\"a\" id=\"e1\"><g><text>A b</text></g></g><g class=\"entity\" data-qualified-name=\"b\"><text>B</text></g></g></svg>";
    assert_eq!(
        anchor_entities(svg, &links),
        "<svg><g><a href=\"../class/a.svg\" xlink:href=\"../class/a.svg\"><g class=\"entity\" data-qualified-name=\"a\" id=\"e1\"><g><text>A b</text></g></g></a><g class=\"entity\" data-qualified-name=\"b\"><text>B</text></g></g></svg>"
    );
    assert_eq!(anchor_entities(svg, &[]), svg);
}

// The anchoring as it once was (a linear link search per group, two `insert_str`
// on the whole document per anchor), kept as the reference the single pass must
// match byte for byte.
fn anchor_entities_reference(svg: &str, links: &[(String, String)]) -> String {
    if links.is_empty() {
        return svg.to_string();
    }
    let mut out = svg.to_string();
    let mut from = 0;
    while let Some(rel) = out[from..].find("<g ") {
        let start = from + rel;
        from = start + "<g ".len();
        let Some(tag_len) = out[start..].find('>') else {
            break;
        };
        let tag = out[start..=start + tag_len].to_string();
        let linked = match attr_value(&tag, "class").as_deref() {
            Some("entity") => true,
            Some(c) => c.starts_with("participant participant-"),
            None => false,
        };
        if !linked {
            continue;
        }
        let Some(key) = attr_value(&tag, "data-qualified-name") else {
            continue;
        };
        let Some(href) = links
            .iter()
            .find(|(k, _)| xml_attr(k) == key)
            .map(|(_, h)| h)
        else {
            continue;
        };
        let Some(end) = group_end(&out, start) else {
            break;
        };
        let anchor = format!("<a href=\"{0}\" xlink:href=\"{0}\">", xml_attr(href));
        out.insert_str(end, "</a>");
        out.insert_str(start, &anchor);
        from = start + anchor.len() + "<g ".len();
    }
    out
}

fn link(key: &str, href: &str) -> (String, String) {
    (key.to_string(), href.to_string())
}

#[test]
fn anchoring_matches_the_reference_byte_for_byte() {
    let links = vec![
        link("a", "../class/a.svg"),
        link("a", "../class/shadowed.svg"),
        link("P q", "../class/p.svg"),
        link("R & <D>", "../class/r&d.svg"),
        link("say \"hi\"", "../class/hi.svg"),
        link("Kunde", "../class/kunde.svg"),
        link("Bob", "../sequence/bob.svg"),
        link("nowhere", "../class/nowhere.svg"),
    ];
    let svg = concat!(
        "<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\">",
        "<defs><g id=\"d\"></g></defs><g>",
        // A package holding a linked class: nested anchors.
        "<g class=\"entity\" data-qualified-name=\"P q\"><rect/>",
        "<g class=\"entity\" data-qualified-name=\"a\" id=\"e1\"><g><text>A b</text></g></g>",
        "</g>",
        // Adjacent siblings: one closes where the next opens.
        "<g class=\"entity\" data-qualified-name=\"R &amp; &lt;D&gt;\"><text>R</text></g>",
        "<g class=\"entity\" data-qualified-name=\"say &quot;hi&quot;\"><text>hi</text></g>",
        "<g class=\"entity\" data-qualified-name=\"Kunde\"><text>K\u{fc}nde \u{2192} \u{1f600}</text></g>",
        // Unlinked, unnamed, unclassed, and other classes keep no anchor.
        "<g class=\"entity\" data-qualified-name=\"b\"><text>B</text></g>",
        "<g class=\"entity\"><text>anon</text></g>",
        "<g data-qualified-name=\"a\"><text>plain</text></g>",
        "<g class=\"link\" data-qualified-name=\"a\"><path/></g>",
        "<g class=\"cluster\" data-qualified-name=\"P q\"><rect/></g>",
        // A sequence participant's head and tail.
        "<g class=\"participant participant-head\" data-qualified-name=\"Bob\"><rect/></g>",
        "<g class=\"participant participant-tail\" data-qualified-name=\"Bob\"><rect/></g>",
        "<g class=\"participant-lifeline\" data-qualified-name=\"Bob\"><line/></g>",
        "</g></svg>"
    );
    let expected = anchor_entities_reference(svg, &links);
    assert_eq!(anchor_entities(svg, &links), expected);
    assert_eq!(expected.matches("<a ").count(), 7, "{}", expected);
    assert!(
        expected.contains("href=\"../class/r&amp;d.svg\""),
        "{}",
        expected
    );
    assert!(!expected.contains("shadowed"), "{}", expected);
    // A truncated document: anchoring stops at the group that never closes.
    for cut in [
        svg.len() - 12,
        svg.find("<g class=\"entity\" data-qualified-name=\"b\"")
            .unwrap()
            + 20,
    ] {
        let truncated = &svg[..cut];
        assert_eq!(
            anchor_entities(truncated, &links),
            anchor_entities_reference(truncated, &links),
            "cut at {}",
            cut
        );
    }
    let unclosed = "<svg><g class=\"entity\" data-qualified-name=\"a\"><g class=\"entity\" data-qualified-name=\"Kunde\"></g>";
    assert_eq!(
        anchor_entities(unclosed, &links),
        anchor_entities_reference(unclosed, &links)
    );
    assert_eq!(anchor_entities("<svg><g ", &links), "<svg><g ");

    // Generated nestings and sibling runs agree too.
    let mut seed = 0x2545_f491_u64;
    let mut next = |n: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    for _ in 0..200 {
        let mut doc = String::from("<svg>");
        let mut depth = 0;
        for _ in 0..40 {
            match next(5) {
                0 | 1 => {
                    let class = [
                        "entity",
                        "participant participant-head",
                        "cluster",
                        "entity",
                    ][next(4) as usize];
                    let key = ["a", "P q", "Kunde", "b", "Bob"][next(5) as usize];
                    doc.push_str(&format!(
                        "<g class=\"{}\" data-qualified-name=\"{}\">",
                        class, key
                    ));
                    depth += 1;
                }
                2 if depth > 0 => {
                    doc.push_str("</g>");
                    depth -= 1;
                }
                3 => doc.push_str("<g>"),
                _ => doc.push_str("<text>t</text>"),
            }
        }
        for _ in 0..next(depth + 1) {
            doc.push_str("</g>");
        }
        assert_eq!(
            anchor_entities(&doc, &links),
            anchor_entities_reference(&doc, &links),
            "{}",
            doc
        );
    }
}

#[test]
fn anchoring_stays_linear_in_elements_and_links() {
    let n = 20_000;
    let links: Vec<(String, String)> = (0..n)
        .map(|i| link(&format!("e{}", i), &format!("../class/e{}.svg", i)))
        .collect();
    let doc = |n: usize| {
        let mut svg = String::from("<svg><g>");
        for i in 0..n {
            svg.push_str(&format!(
                "<g class=\"entity\" data-qualified-name=\"e{}\"><rect/><text>E{}</text></g>",
                i, i
            ));
        }
        svg.push_str("</g></svg>");
        svg
    };
    // The old shape escaped every link key once per group: n * n / 2 calls.
    let calls = |f: fn(&str, &[(String, String)]) -> String, svg: &str, links| {
        XML_ATTR_CALLS.with(|c| c.set(0));
        let out = f(svg, links);
        (out, XML_ATTR_CALLS.with(|c| c.get()))
    };
    let small = 200;
    let small_svg = doc(small);
    let svg = doc(n);
    let (old, old_calls) = calls(anchor_entities_reference, &small_svg, &links[..small]);
    let (new, new_calls) = calls(anchor_entities, &small_svg, &links[..small]);
    assert_eq!(new, old);
    assert!(old_calls > small * small / 2, "{}", old_calls);
    assert_eq!(new_calls, 2 * small);
    let (out, big_calls) = calls(anchor_entities, &svg, &links);
    assert_eq!(big_calls, 2 * n);
    assert_eq!(out.matches("<a href=").count(), n);
    assert_eq!(out.matches("</a>").count(), n);
    assert_eq!(
        out.len(),
        svg.len()
            + (0..n)
                .map(|i| format!(
                    "<a href=\"../class/e{0}.svg\" xlink:href=\"../class/e{0}.svg\"></a>",
                    i
                )
                .len())
                .sum::<usize>()
    );
}

#[test]
fn every_emitter_renders_through_the_crate() {
    let s = showcase();
    let cases: Vec<(&str, Vec<&str>)> = vec![
        (
            "class",
            vec![
                "ent:customer",
                "ent:shopping-cart",
                "ent:order",
                "ent:order-item",
            ],
        ),
        ("object", vec!["ent:ana", "ent:anas-cart"]),
        (
            "package",
            vec![
                "ent:order-service",
                "ent:shopping-cart",
                "ent:order",
                "ent:order-item",
                "ent:inventory-service",
            ],
        ),
        (
            "component",
            vec![
                "ent:customer",
                "ent:order-service",
                "ent:inventory-service",
                "ent:checkout-api",
                "ent:stock-api",
            ],
        ),
        (
            "composite",
            vec![
                "ent:order-service",
                "ent:shopping-cart",
                "ent:order",
                "ent:order-item",
            ],
        ),
        ("deployment", vec!["ent:shop", "ent:customer"]),
        (
            "use-case",
            vec!["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"],
        ),
        (
            "activity",
            vec!["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"],
        ),
        ("state", vec!["ent:order"]),
        ("sequence", vec!["req:shop-1", "req:shop-3"]),
        ("communication", vec!["req:shop-1", "req:shop-3"]),
        ("timing", vec!["ent:order", "req:shop-10"]),
        (
            "overview",
            vec!["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"],
        ),
    ];
    assert_eq!(cases.len(), VIEW_KINDS.len());
    for (kind, members) in &cases {
        let puml = emit_kind(&s, kind, members);
        let svg = render_svg(&puml).unwrap_or_else(|e| panic!("{}: {}\n{}", kind, e, puml));
        assert!(svg.contains("<svg"), "{}", kind);
        // An empty view of the kind renders too.
        let puml = emit_kind(&s, kind, &[]);
        render_svg(&puml).unwrap_or_else(|e| panic!("empty {}: {}\n{}", kind, e, puml));
    }
    // The collapsed node's link and the over-limit title render.
    let mut v = view(
        "class",
        "Public",
        &["ent:shop", "ent:order-service", "ent:order"],
    );
    v.collapse.push("ent:shop".into());
    let puml = emit(&s, "view:class/public", &v);
    render_svg(&puml).unwrap_or_else(|e| panic!("{}\n{}", e, puml));
}

#[test]
fn render_svg_draws_the_showcase_root_view() {
    let s = showcase();
    let root = root_view(&s);
    let (puml, svg) = render_view(&s, &root).unwrap();
    assert!(puml.starts_with("@startuml\n"));
    // Every top-level member is drawn by its element group, the actor included: the
    // emitter declares it as an actor, and the in-process conversion draws it as a
    // component with the actor stereotype (docs/compiler/diagrams.md#the-renderer).
    // Every element links to its card, four anchors.
    let members = &s.graph.views[&root].members;
    assert!(members.contains(&"ent:shop".to_string()), "{:?}", members);
    assert!(
        members.contains(&"ent:customer".to_string()),
        "{:?}",
        members
    );
    assert!(
        puml.contains("actor \"Customer\" as customer [[../../docsgen/entities/customer.md]]\n"),
        "{}",
        puml
    );
    for m in members.iter() {
        let group = format!("data-qualified-name=\"{}\"", alias_of(m));
        assert!(svg.contains(&group), "{} missing from\n{}", m, svg);
    }
    assert!(svg.contains(">Shop</text>"), "{}", svg);
    assert!(
        svg.contains(
            "<a href=\"../../docsgen/entities/shop.md\" xlink:href=\"../../docsgen/entities/shop.md\"><g class=\"entity\" data-qualified-name=\"shop\""
        ),
        "{}",
        svg
    );
    assert_eq!(svg.matches("<a ").count(), 4, "{}", svg);
    assert!(svg.contains("entities/customer.md"), "{}", svg);
    assert!(svg.contains(">Customer</text>"), "{}", svg);
    assert!(render_view(&s, "view:class/nope").is_err());
    assert!(render_svg("@startuml\nthis is not plantuml at all ((\n@enduml\n").is_err());
    // A defect inside the crate (it measures edge labels byte by byte, so a
    // multi-byte character in one panics) surfaces as an error, never an abort.
    if let Err(e) = render_svg("@startuml\nclass A\nclass B\nA -- B : ×2\n@enduml\n") {
        assert!(e.0.contains("renderer panicked"), "{}", e);
    }
}

#[cfg(unix)]
#[test]
fn native_binary_swap_pipes_stdin_and_reports_a_failed_exit() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp("native");
    let stub = dir.join("plantuml");
    crate::vfs::write(
        &stub,
        "#!/bin/sh\ntest \"$1\" = -tsvg && test \"$2\" = -pipe && cat\n",
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let puml = "@startuml\nclass A\n@enduml\n";
    assert_eq!(native_svg(stub.to_str().unwrap(), puml).unwrap(), puml);
    let err = native_svg("/usr/bin/false", puml).unwrap_err();
    assert!(err.0.contains("exited with"), "{}", err);
    assert!(native_svg(dir.join("missing").to_str().unwrap(), puml).is_err());
    let _ = crate::vfs::remove_dir_all(&dir);
}

// A native binary that hangs is killed at the deadline and reads as a renderer
// error (docs/compiler/diagrams.md#the-renderer).
#[cfg(unix)]
#[test]
fn native_binary_past_the_deadline_is_killed() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp("native-hang");
    let stub = dir.join("plantuml");
    crate::vfs::write(&stub, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Larger than a pipe buffer: a child that never reads stdin cannot stall the write.
    let puml = format!("@startuml\n{}@enduml\n", "class A\n".repeat(40_000));
    let started = std::time::Instant::now();
    let err = native_svg_within(
        stub.to_str().unwrap(),
        &puml,
        std::time::Duration::from_secs(1),
    )
    .unwrap_err();
    assert!(err.0.contains("killed after"), "{}", err);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    let _ = crate::vfs::remove_dir_all(&dir);
}

#[test]
fn render_png_yields_a_png_header() {
    let s = showcase();
    let (_, svg) = render_view(&s, "view:state/order").unwrap();
    let png = render_png(&svg).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}
