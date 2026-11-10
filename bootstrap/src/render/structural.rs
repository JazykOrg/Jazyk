use super::scene::{frame, is_below, structure, title_line, Scene};
use super::text::{
    alias_of, arrow_line, arrow_line_with, arrow_lines, attribute_line, bracketed, declare,
    document, empty_diagram, identifier, link_suffix, notation, oneline, quoted, with_body,
};
use crate::derive::instance_types;
use std::collections::{BTreeMap, BTreeSet};

// ---- structural emitters ----

pub(super) fn emit_class(scene: &Scene) -> String {
    let f = frame(scene);
    let mut out = Vec::new();
    for id in &f.st.shown {
        let line = declare(scene, "class", id, true);
        let body = scene
            .entity(id)
            .attributes
            .iter()
            .map(attribute_line)
            .collect();
        with_body(&mut out, line, body);
    }
    out.extend(arrow_lines(&f.st.arrows));
    document(title_line(scene.view, f.note.as_deref()), out)
}

pub(super) fn emit_object(scene: &Scene) -> String {
    let f = frame(scene);
    let types = instance_types(scene.store);
    let mut out = Vec::new();
    for id in &f.st.shown {
        let e = scene.entity(id);
        let label = match types
            .get(id)
            .and_then(|t| scene.store.graph.entities.get(t))
        {
            Some(t) => format!("{} : {}", oneline(&e.name), oneline(&t.name)),
            None => oneline(&e.name),
        };
        let mut line = format!("object {} as {}", quoted(&label), alias_of(id));
        line.push_str(&link_suffix(id));
        let body = e
            .attributes
            .iter()
            .map(|a| match (a.value.as_deref(), a.r#type.as_deref()) {
                (Some(v), _) => format!("{} = {}", oneline(&a.name), oneline(v)),
                (None, Some(t)) => format!("{} : {}", oneline(&a.name), oneline(t)),
                (None, None) => oneline(&a.name),
            })
            .collect();
        with_body(&mut out, line, body);
    }
    out.extend(arrow_lines(&f.st.arrows));
    document(title_line(scene.view, f.note.as_deref()), out)
}

pub(super) fn emit_package(scene: &Scene) -> String {
    let store = scene.store;
    let f = frame(scene);
    let shown = &f.st.shown;
    let set: BTreeSet<&str> = shown.iter().map(String::as_str).collect();
    let parent_in = |id: &str| -> Option<String> {
        scene
            .entity(id)
            .parent
            .as_deref()
            .map(|p| store.resolve_id(p).to_string())
            .filter(|p| set.contains(p.as_str()))
    };
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for s in shown {
        if let Some(p) = parent_in(s) {
            children.entry(p).or_default().push(s.clone());
        }
    }
    let roots: Vec<String> = shown
        .iter()
        .filter(|s| parent_in(s).is_none())
        .cloned()
        .collect();
    let mut out = Vec::new();
    // Packages go by their quoted name, never an alias: the crate emits invalid DOT for
    // an aliased package with a body. Classes inside keep their aliases.
    // A scope query with no containment among the members: the scope is the package.
    if children.is_empty() && !shown.is_empty() {
        if let Some(scope) = scene.view.query.as_ref().and_then(|q| q.scope.as_deref()) {
            out.push(format!("package {} {{", quoted(scope)));
            for s in shown {
                out.push(format!("  {}", declare(scene, "class", s, true)));
            }
            out.push("}".to_string());
            return document(title_line(scene.view, f.note.as_deref()), out);
        }
    }
    fn emit_node(
        out: &mut Vec<String>,
        indent: usize,
        id: &str,
        children: &BTreeMap<String, Vec<String>>,
        scene: &Scene,
    ) {
        let pad = "  ".repeat(indent);
        let name = quoted(&scene.entity(id).name);
        match children.get(id) {
            Some(kids) if !kids.is_empty() => {
                out.push(format!("{}package {}{} {{", pad, name, link_suffix(id)));
                for k in kids {
                    if children.get(k).is_some_and(|c| !c.is_empty()) {
                        emit_node(out, indent + 1, k, children, scene);
                    } else {
                        out.push(format!("{}  {}", pad, declare(scene, "class", k, true)));
                    }
                }
                out.push(format!("{}}}", pad));
            }
            // Never an empty body: the one-line form.
            _ => out.push(format!("{}package {}{}", pad, name, link_suffix(id))),
        }
    }
    for r in &roots {
        emit_node(&mut out, 0, r, &children, scene);
    }
    // Arrows between top-level packages only: everything lifts to its root.
    let root_set: BTreeSet<String> = roots.iter().cloned().collect();
    let lifted = structure(store, shown, &root_set);
    let by_name = |id: &str| quoted(&scene.name(id));
    for a in &lifted.arrows {
        out.push(arrow_line_with(a, notation(&a.rel_type), None, &by_name));
    }
    document(title_line(scene.view, f.note.as_deref()), out)
}

pub(super) fn emit_component(scene: &Scene) -> String {
    let f = frame(scene);
    let mut out = Vec::new();
    for id in &f.st.shown {
        let keyword = if scene.labeled(id, "actor") {
            "actor"
        } else if scene.labeled(id, "interface") {
            "interface"
        } else {
            "component"
        };
        out.push(declare(scene, keyword, id, true));
    }
    for a in &f.st.arrows {
        let to_interface = scene.labeled(&a.b, "interface");
        let line = if a.rel_type == "realization" && to_interface {
            arrow_line(a, "--", None)
        } else if a.rel_type == "dependency" && to_interface && !scene.labeled(&a.a, "actor") {
            arrow_line(a, "--(", Some("use"))
        } else {
            arrow_line(a, notation(&a.rel_type), None)
        };
        out.push(line);
    }
    document(title_line(scene.view, f.note.as_deref()), out)
}

pub(super) fn emit_composite(scene: &Scene) -> String {
    let store = scene.store;
    let f = frame(scene);
    let shown = &f.st.shown;
    let boundary = shown
        .iter()
        .find(|b| shown.iter().any(|o| o != *b && is_below(store, b, o)))
        .or(shown.first())
        .cloned();
    let Some(boundary) = boundary else {
        return empty_diagram(scene.view, "no boundary entity");
    };
    let (parts, outside): (Vec<&String>, Vec<&String>) = shown
        .iter()
        .filter(|o| **o != boundary)
        .partition(|o| is_below(store, &boundary, o));
    let mut out = Vec::new();
    out.push(format!(
        "component {} as {}{} {{",
        quoted(&scene.name(&boundary)),
        alias_of(&boundary),
        link_suffix(&boundary)
    ));
    for p in &parts {
        out.push(format!(
            "  {} as {}{}",
            bracketed(&scene.name(p)),
            alias_of(p),
            link_suffix(p)
        ));
    }
    out.push("}".to_string());
    for o in &outside {
        out.push(format!(
            "{} as {}{}",
            bracketed(&scene.name(o)),
            alias_of(o),
            link_suffix(o)
        ));
    }
    for a in &f.st.arrows {
        if a.a == boundary || a.b == boundary {
            continue;
        }
        out.push(arrow_line(a, notation(&a.rel_type), None));
    }
    document(title_line(scene.view, f.note.as_deref()), out)
}

pub(super) fn emit_deployment(scene: &Scene) -> String {
    let f = frame(scene);
    let mut placed: Vec<(String, Vec<String>)> = Vec::new();
    let mut bare: Vec<String> = Vec::new();
    for id in &f.st.shown {
        match scene
            .entity(id)
            .attributes
            .iter()
            .find(|a| a.value.is_some())
        {
            Some(a) => {
                let label = format!(
                    "{} {}",
                    oneline(a.value.as_deref().unwrap_or_default()),
                    oneline(&a.name)
                );
                match placed.iter_mut().find(|(l, _)| *l == label) {
                    Some((_, arts)) => arts.push(id.clone()),
                    None => placed.push((label, vec![id.clone()])),
                }
            }
            None => bare.push(id.clone()),
        }
    }
    let mut out = Vec::new();
    for (label, arts) in &placed {
        out.push(format!(
            "node {} as {} {{",
            quoted(label),
            identifier(&label.to_lowercase(), "p_")
        ));
        for id in arts {
            out.push(format!("  {}", declare(scene, "artifact", id, false)));
        }
        out.push("}".to_string());
    }
    for id in &bare {
        out.push(declare(scene, "artifact", id, false));
    }
    out.extend(arrow_lines(&f.st.arrows));
    document(title_line(scene.view, f.note.as_deref()), out)
}
