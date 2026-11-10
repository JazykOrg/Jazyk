use super::*;

// Mirrors docs/frontends/wasm.md#diagrams: the rendering on disk stands in for a
// render only when the .puml beside it is this emission.
#[test]
fn the_rendering_on_disk_serves_only_when_its_puml_is_the_emission() {
    let dir = std::env::temp_dir().join(format!("jazyk-ondisk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("diagrams/class")).unwrap();
    std::fs::write(
        dir.join("diagrams/class/shop.puml"),
        "@startuml\nA\n@enduml\n",
    )
    .unwrap();
    std::fs::write(dir.join("diagrams/class/shop.svg"), "<svg>seeded</svg>").unwrap();
    assert_eq!(
        rendered_on_disk(&dir, "view:class/shop", "@startuml\nA\n@enduml\n").as_deref(),
        Some("<svg>seeded</svg>")
    );
    assert_eq!(
        rendered_on_disk(&dir, "view:class/shop", "@startuml\nB\n@enduml\n"),
        None
    );
    assert_eq!(
        rendered_on_disk(&dir, "view:class/other", "@startuml\nA\n@enduml\n"),
        None
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_all_writes_paths_and_skips_unchanged() {
    let out = tmp("all");
    let mut s = showcase();
    let report = render_all(&s, &out);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(report.skipped.is_empty());
    // Every target renders; the curated views and the defaults whose rule is fixed
    // by the showcase are named, the level and flow defaults come from derivation.
    let expected: Vec<String> = render_targets(&s)
        .into_iter()
        .map(|(id, _)| id.strip_prefix("view:").unwrap().to_string())
        .collect();
    for rel in [
        "component/shop",
        "state/order",
        "object/customer",
        "object/shopping-cart",
        "usecase/checkout",
        "sequence/checkout",
    ] {
        assert!(expected.contains(&rel.to_string()), "{:?}", expected);
    }
    assert!(expected.contains(&root_view(&s)[5..].to_string()));
    for rel in &expected {
        assert!(
            out.join("diagrams").join(format!("{}.puml", rel)).vexists(),
            "{}",
            rel
        );
        assert!(
            out.join("diagrams").join(format!("{}.svg", rel)).vexists(),
            "{}",
            rel
        );
    }
    assert_eq!(report.rendered.len(), expected.len());
    // A second run renders nothing.
    let again = render_all(&s, &out);
    assert!(again.rendered.is_empty(), "{:?}", again.rendered);
    assert_eq!(again.skipped.len(), expected.len());
    assert!(again.removed.is_empty());
    // A png on request, dropped when the puml changes.
    let png = png_for(&out, "view:state/order").unwrap();
    assert!(png.vexists());
    assert_eq!(png_for(&out, "view:state/order").unwrap(), png);
    assert!(png_for(&out, "view:state/nope").is_err());
    // A deleted curated view loses its files; a machine without a state view still
    // renders as view:state/<slug>; a changed view re-renders and loses its png.
    s.graph.views.remove("view:sequence/checkout");
    s.graph.views.remove("view:state/order");
    s.graph
        .views
        .get_mut("view:usecase/checkout")
        .unwrap()
        .title = "Checkout flow".into();
    let third = render_all(&s, &out);
    assert_eq!(third.removed, vec!["sequence/checkout".to_string()]);
    assert!(!out.join("diagrams/sequence/checkout.svg").vexists());
    assert!(out.join("diagrams/state/order.svg").vexists());
    assert!(third
        .rendered
        .contains(&"view:usecase/checkout".to_string()));
    assert!(third.skipped.contains(&"view:state/order".to_string()));
    assert!(png.vexists());
    s.graph
        .requirements
        .get_mut("req:shop-7")
        .unwrap()
        .transition
        .as_mut()
        .unwrap()
        .trigger = Some("payment cleared".into());
    recompute(&mut s, "g3", &mut RecordBatch::new(3));
    let fourth = render_all(&s, &out);
    assert!(fourth.rendered.contains(&"view:state/order".to_string()));
    assert!(!png.vexists());
    let _ = crate::vfs::remove_dir_all(&out);
}
