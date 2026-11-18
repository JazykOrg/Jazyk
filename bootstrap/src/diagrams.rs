//! Authored diagram analysis and context associations. Never creates graph nodes.
use crate::model::*;
use crate::store::{scope_root_target, Store};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const FIELDS: &[&str] = &[
    "purpose",
    "description",
    "area",
    "perspective",
    "relationships",
    "labels",
    "notes",
    "groups",
    "layout",
    "fragments",
    "links",
];

pub fn area(view: &View) -> Option<&str> {
    view.authored.get("area")?.get("target")?.as_str()
}

pub fn descendants(store: &Store, target: &str) -> BTreeSet<String> {
    store
        .graph
        .entities
        .keys()
        .filter(|id| {
            let mut cur = id.as_str();
            for _ in 0..64 {
                if cur == target {
                    return true;
                }
                let Some(e) = store.graph.entities.get(cur) else {
                    break;
                };
                if target == scope_root_target(&e.scope) {
                    return true;
                }
                let Some(p) = &e.parent else { break };
                cur = p;
            }
            false
        })
        .cloned()
        .collect()
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub boundary: Value,
    pub measure: Option<usize>,
    pub id: String,
    pub target: String,
    pub family: String,
    pub candidates: BTreeMap<String, String>,
    pub kinds: Vec<String>,
    pub soft: usize,
    pub hard: usize,
}
impl Finding {
    pub fn count(&self) -> usize {
        self.measure.unwrap_or(self.candidates.len())
    }
    pub fn value(&self) -> Value {
        json!({"id":self.id,"boundary":self.boundary,"target":self.target,"family":self.family,"candidates":self.candidates,"kinds":self.kinds,"count":self.count(),"soft":self.soft,"hard":if self.hard==usize::MAX {None}else{Some(self.hard)}})
    }
    pub fn hash(&self) -> String {
        hash_hex(&self.value().to_string())
    }
    pub fn suppressed(&self, store: &Store) -> bool {
        if self.family == "source" {
            return false;
        }
        let Some(d) = store.decisions.diagram_coverage.get(&self.id) else {
            return false;
        };
        let Some(base) = d["candidates"].as_object() else {
            return false;
        };
        if d["target"] != self.target || d["ruleVersion"] != 1 || d["boundary"] != self.boundary {
            return false;
        }
        let delta = base.len().div_ceil(5).max(1);
        let removed = base
            .iter()
            .filter(|(id, h)| {
                self.candidates
                    .get(*id)
                    .is_none_or(|s| h.as_str() != Some(s))
            })
            .count();
        let added = self
            .candidates
            .keys()
            .filter(|id| !base.contains_key(*id))
            .count();
        removed + added < delta && self.candidates.len() < base.len() + delta
    }
    pub fn covered(&self, store: &Store) -> bool {
        if self.family == "source" {
            return store.graph.views.values().any(|v| !effective_members(store,v).is_empty() && matches!(&v.provenance,Some(Provenance::Quote(src)) if self.candidates.contains_key(&format!("{}#{}",src.doc,src.section)) && store.docs.get(&src.doc).and_then(|doc|doc.sections.get(&src.section)).is_some_and(|block|crate::md::locate(&block.raw,&src.quote).is_some())));
        }
        if self.family == "drift" {
            return !findings(store).iter().any(|f| f.id == self.id);
        }
        if self.candidates.is_empty() {
            return false;
        }
        let mut represented = BTreeSet::new();
        for v in store
            .graph
            .views
            .values()
            .filter(|v| area(v) == Some(self.target.as_str()) && self.kinds.contains(&v.kind))
        {
            let effective = effective_members(store, v);
            if effective.is_empty() {
                continue;
            }
            if self.family == "system" {
                let inside = descendants(store, &self.target);
                let has_outside = self
                    .candidates
                    .keys()
                    .filter(|id| !inside.contains(*id))
                    .all(|id| effective.contains(id));
                if !has_outside {
                    continue;
                }
                if effective.contains(&self.target) {
                    return true;
                }
            }
            if self.family == "structure"
                && v.kind == "composite"
                && effective.contains(&self.target)
            {
                return true;
            }
            if self.family == "state" && effective.contains(&self.target) {
                return true;
            }
            if self.family == "timing" && effective.contains(&self.target) {
                return true;
            }
            if self.family == "overview" {
                for link in v
                    .authored
                    .get("links")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(id) = link["view"].as_str() {
                        represented.insert(id.to_string());
                    }
                }
            }
            for id in self.candidates.keys() {
                if effective.contains(id)
                    || (self.family == "deployment"
                        && id
                            .split_once('#')
                            .is_some_and(|(e, _)| effective.iter().any(|m| m == e)))
                    || v.collapse
                        .iter()
                        .any(|c| effective.contains(c) && descendants(store, c).contains(id))
                {
                    represented.insert(id.clone());
                }
            }
        }
        5 * represented.len() >= 4 * self.candidates.len()
    }
}

pub fn findings(store: &Store) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut push = |family: &str,
                    target: String,
                    ids: Vec<String>,
                    kinds: &[&str],
                    soft: usize,
                    hard: usize| {
        if ids.len() <= soft {
            return;
        }
        let candidates = ids
            .into_iter()
            .map(|id| {
                let value = if let Some(r) = store.graph.requirements.get(&id) {
                    serde_json::to_value(r).unwrap_or_default()
                } else if let Some(e) = store.graph.entities.get(&id) {
                    serde_json::to_value(e).unwrap_or_default()
                } else {
                    json!(id)
                };
                (id, semantic_hash(value))
            })
            .collect();
        out.push(Finding {
            boundary: Value::Null,
            measure: None,
            id: format!("{}:{}", family, target),
            target,
            family: family.into(),
            candidates,
            kinds: kinds.iter().map(|s| s.to_string()).collect(),
            soft,
            hard,
        });
    };
    let instances = crate::derive::instance_types(store);
    let mut levels: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, e) in &store.graph.entities {
        if !instances.contains_key(id) {
            levels
                .entry(
                    e.parent
                        .clone()
                        .unwrap_or_else(|| scope_root_target(&e.scope)),
                )
                .or_default()
                .push(id.clone());
        }
    }
    for (target, children) in &levels {
        push(
            "structure",
            target.clone(),
            children.clone(),
            &["class", "component", "package", "composite"],
            crate::limits::DIAGRAM_STRUCTURE_SOFT,
            crate::limits::DIAGRAM_STRUCTURE_HARD,
        );
    }
    for (id, e) in &store.graph.entities {
        if e.stereotype.as_deref() == Some("system") {
            let mut ids = levels.get(id).cloned().unwrap_or_default();
            let inside = descendants(store, id);
            for c in store
                .graph
                .relationships
                .values()
                .flat_map(|r| r.contributions.iter())
            {
                if inside.contains(&c.a) && !inside.contains(&c.b) && !ids.contains(&c.b) {
                    ids.push(c.b.clone());
                }
                if inside.contains(&c.b) && !inside.contains(&c.a) && !ids.contains(&c.a) {
                    ids.push(c.a.clone());
                }
            }
            if ids.is_empty() {
                ids.push(id.clone());
            }
            push(
                "system",
                id.clone(),
                ids,
                &["component"],
                0,
                crate::limits::DIAGRAM_SYSTEM_HARD,
            );
        }
    }
    for (target, document, ids) in crate::derive::diagram_flow_areas(store) {
        let candidates = ids
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    semantic_hash(
                        serde_json::to_value(&store.graph.requirements[id]).unwrap_or_default(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let requirements = ids
            .iter()
            .filter_map(|id| store.graph.requirements.get(id))
            .collect::<Vec<_>>();
        let participants = requirements
            .iter()
            .flat_map(|r| r.entities.iter())
            .collect::<BTreeSet<_>>();
        let interaction = requirements.iter().any(|r| !r.edges.is_empty());
        let paths = requirements.iter().any(|r| r.transition.is_some());
        let actors = participants.iter().any(|id| {
            store
                .graph
                .entities
                .get(*id)
                .is_some_and(|e| e.stereotype.as_deref() == Some("actor"))
        });
        let opportunity =
            interaction && participants.len() >= crate::limits::DIAGRAM_COMMUNICATION_PARTICIPANTS;
        if candidates.len() > crate::limits::DIAGRAM_FLOW_SOFT || opportunity {
            let mut kinds = vec!["activity".into(), "sequence".into(), "communication".into()];
            if actors && !paths && !interaction {
                kinds.push("use-case".into());
            }
            out.push(Finding {
                boundary: Value::Null,
                measure: None,
                id: format!("flow:{}:{}", target, document),
                target,
                family: "flow".into(),
                candidates,
                kinds,
                soft: if opportunity {
                    ids.len()
                        .saturating_sub(1)
                        .min(crate::limits::DIAGRAM_FLOW_SOFT)
                } else {
                    crate::limits::DIAGRAM_FLOW_SOFT
                },
                hard: crate::limits::DIAGRAM_FLOW_HARD,
            });
        }
    }

    for machine in store.graph.state_machines.values() {
        let ids: Vec<String> = machine
            .states
            .iter()
            .map(|s| format!("{}#{}", machine.subject, s))
            .collect();
        // Machine-state identity is enough for count changes; transitions join the fingerprint below.
        let mut f = Finding {
            boundary: Value::Null,
            measure: None,
            id: format!("state:{}", machine.subject),
            target: machine.subject.clone(),
            family: "state".into(),
            candidates: BTreeMap::new(),
            kinds: vec!["state".into()],
            soft: crate::limits::DIAGRAM_STATE_SOFT,
            hard: crate::limits::DIAGRAM_STATE_HARD,
        };
        for id in ids {
            f.candidates.insert(
                id.clone(),
                hash_hex(&json!({"state":id.split_once('#').map(|(_,state)|state),"initial":machine.initial.as_ref().is_some_and(|state|id==format!("{}#{}",machine.subject,state)),"transitions":machine.transitions.iter().filter(|t|id==format!("{}#{}",machine.subject,t.from) || id==format!("{}#{}",machine.subject,t.to)).collect::<Vec<_>>()}).to_string()),
            );
        }
        if f.candidates.len() > f.soft {
            out.push(f);
        }
    }
    let mut objects: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, ty) in instances {
        objects.entry(ty).or_default().push(id);
    }
    for (target, ids) in objects {
        let candidates = ids
            .into_iter()
            .map(|id| {
                let h = semantic_hash(
                    serde_json::to_value(&store.graph.entities[&id]).unwrap_or_default(),
                );
                (id, h)
            })
            .collect::<BTreeMap<_, _>>();
        if candidates.len() > crate::limits::DIAGRAM_OBJECT_SOFT {
            out.push(Finding {
                boundary: Value::Null,
                measure: None,
                id: format!("object:{}", target),
                target,
                family: "object".into(),
                candidates,
                kinds: vec!["object".into()],
                soft: crate::limits::DIAGRAM_OBJECT_SOFT,
                hard: crate::limits::DIAGRAM_OBJECT_HARD,
            });
        }
    }
    let mut placements: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (id, e) in &store.graph.entities {
        let mut target = scope_root_target(&e.scope);
        let mut cur = Some(id.as_str());
        for _ in 0..64 {
            let Some(at) = cur else { break };
            let Some(n) = store.graph.entities.get(at) else {
                break;
            };
            if n.stereotype.as_deref() == Some("system") {
                target = at.into();
                break;
            }
            cur = n.parent.as_deref();
        }
        for attr in &e.attributes {
            if let Some(value) = &attr.value {
                placements
                    .entry(target.clone())
                    .or_default()
                    .insert(format!("{}#{}", id, attr.name), hash_hex(value));
            }
        }
    }
    for (target, candidates) in placements {
        out.push(Finding {
            boundary: Value::Null,
            measure: None,
            id: format!("deployment:{}", target),
            target,
            family: "deployment".into(),
            candidates,
            kinds: vec!["deployment".into()],
            soft: 0,
            hard: crate::limits::DIAGRAM_DEPLOYMENT_HARD,
        });
    }
    for (target, children) in &levels {
        let groups = children
            .iter()
            .filter(|id| levels.contains_key(*id))
            .count();
        let parts = children.len();
        let candidates = children
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    semantic_hash(
                        serde_json::to_value(&store.graph.entities[id]).unwrap_or_default(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let connected = store
            .graph
            .relationships
            .values()
            .flat_map(|r| r.contributions.iter())
            .any(|c| children.contains(&c.a) && children.contains(&c.b));
        if parts >= 2
            && (groups >= 2 || connected)
            && parts <= crate::limits::DIAGRAM_STRUCTURE_SOFT
        {
            out.push(Finding {
                boundary: Value::Null,
                measure: None,
                id: format!("structure:{}", target),
                target: target.clone(),
                family: "structure".into(),
                candidates: candidates.clone(),
                kinds: vec![
                    "package".into(),
                    "component".into(),
                    "class".into(),
                    "composite".into(),
                ],
                soft: 0,
                hard: usize::MAX,
            });
        }
    }
    for machine in store.graph.state_machines.values() {
        let candidates = store
            .graph
            .criteria
            .iter()
            .filter(|(_, c)| {
                c.measure.is_some()
                    && (c.entities.contains(&machine.subject)
                        || store
                            .graph
                            .requirements
                            .get(&c.requirement)
                            .is_some_and(|r| r.entities.contains(&machine.subject)))
            })
            .map(|(id, c)| {
                (
                    id.clone(),
                    semantic_hash(serde_json::to_value(c).unwrap_or_default()),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if !candidates.is_empty() {
            out.push(Finding {
                boundary: Value::Null,
                measure: None,
                id: format!("timing:{}", machine.subject),
                target: machine.subject.clone(),
                family: "timing".into(),
                candidates,
                kinds: vec!["timing".into()],
                soft: 0,
                hard: usize::MAX,
            });
        }
    }
    let mut sequences: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (id, v) in &store.graph.views {
        if v.kind == "sequence" {
            if let Some(target) = area(v) {
                sequences.entry(target.into()).or_default().insert(
                    id.clone(),
                    semantic_hash(serde_json::to_value(v).unwrap_or_default()),
                );
            }
        }
    }
    for (target, candidates) in sequences {
        if candidates.len() >= crate::limits::DIAGRAM_OVERVIEW_MIN {
            out.push(Finding {
                boundary: Value::Null,
                measure: None,
                id: format!("overview:{}", target),
                target,
                family: "overview".into(),
                candidates,
                kinds: vec!["overview".into()],
                soft: crate::limits::DIAGRAM_OVERVIEW_MIN - 1,
                hard: usize::MAX,
            });
        }
    }
    let mut drift = Vec::new();
    for (id, v) in &store.graph.views {
        if v.authored
            .get("area")
            .and_then(|a| a.get("extent"))
            .and_then(Value::as_str)
            != Some("area")
        {
            continue;
        }
        if let Some(base) = out.iter().find(|f| {
            area(v) == Some(f.target.as_str())
                && f.kinds.contains(&v.kind)
                && !["state", "timing", "system"].contains(&f.family.as_str())
        }) {
            let missing = base
                .candidates
                .iter()
                .filter(|(id, _)| !effective_members(store, v).contains(id))
                .map(|(id, h)| (id.clone(), h.clone()))
                .collect::<BTreeMap<_, _>>();
            if 5 * missing.len() > base.candidates.len() {
                drift.push(Finding {
                    boundary: Value::Null,
                    id: format!("drift:{}", id),
                    target: base.target.clone(),
                    family: "drift".into(),
                    measure: Some(missing.len()),
                    candidates: base.candidates.clone(),
                    kinds: vec![v.kind.clone()],
                    soft: base.candidates.len() / crate::limits::DIAGRAM_DRIFT_SOFT_DENOMINATOR,
                    hard: 2 * base.candidates.len() / crate::limits::DIAGRAM_DRIFT_SOFT_DENOMINATOR,
                });
            }
        }
    }
    out.extend(drift);
    for (doc, record) in &store.docs {
        for (section, block) in &record.sections {
            if block.kind != "diagram" {
                continue;
            }
            let key = format!("{}#{}", doc, section);
            let target = store
                .graph
                .requirements
                .values()
                .find(|r| {
                    r.sources
                        .iter()
                        .any(|src| src.doc == *doc && src.section == *section)
                })
                .and_then(|r| r.entities.first())
                .cloned()
                .unwrap_or_else(|| "scope:public".into());
            out.push(Finding {
                boundary: Value::Null,
                measure: None,
                id: format!("source:{}", key),
                target,
                family: "source".into(),
                candidates: BTreeMap::from([(key, hash_hex(&block.raw))]),
                kinds: VIEW_KINDS.iter().map(|k| k.to_string()).collect(),
                soft: 0,
                hard: 0,
            });
        }
    }
    for f in &mut out {
        let entity = store.graph.entities.get(&f.target);
        f.boundary = json!({"target":f.target,"parent":entity.and_then(|e|e.parent.as_ref()),"scope":entity.map(|e|&e.scope)});
        f.boundary["rule"] = json!({"version":1,"family":f.family,"soft":f.soft,"hard":if f.hard==usize::MAX {None} else {Some(f.hard)}});
        if f.family == "drift" {
            f.boundary["definition"] = store
                .graph
                .views
                .get(f.id.trim_start_matches("drift:"))
                .map(|v| json!(semantic_hash(serde_json::to_value(v).unwrap_or_default())))
                .unwrap_or_default();
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn references(store: &Store, entity: &str) -> Vec<Value> {
    let parent = store.graph.entities.get(entity).map(|e| {
        e.parent
            .clone()
            .unwrap_or_else(|| scope_root_target(&e.scope))
    });
    store.graph.views.iter().filter_map(|(id,v)| {
        let mut reasons=Vec::new();
        let members = effective_members(store, v);
        if members.iter().any(|m|m==entity || store.graph.requirements.get(m).is_some_and(|r|r.entities.iter().any(|e|e==entity))) { reasons.push("member-or-participant"); }
        if v.kind == "object" && members.iter().any(|m|crate::derive::instance_types(store).get(m).is_some_and(|ty|ty==entity)) { reasons.push("type-subject"); }
        if v.collapse.iter().any(|c|descendants(store,c).contains(entity)) { reasons.push("collapsed-under"); }
        if area(v)==Some(entity) {reasons.push("area");}
        if area(v).is_some() && area(v)==parent.as_deref() {reasons.push("parent-context");}
        (!reasons.is_empty()).then(||json!({"id":id,"kind":v.kind,"title":v.title,"purpose":v.authored.get("purpose"),"area":v.authored.get("area"),"reasons":reasons,"members":v.members.len(),"factHash":hash_hex(&serde_json::to_string(v).unwrap_or_default())}))
    }).collect()
}

pub fn validate_fields(
    store: &Store,
    kind: &str,
    fields: &BTreeMap<String, Value>,
) -> Result<(), String> {
    for (key, value) in fields {
        if !FIELDS.contains(&key.as_str()) {
            return Err(format!("Unknown diagram field {key}"));
        }
        if value.is_null() {
            continue;
        }
        let valid = match key.as_str() {
            "purpose" | "description" | "perspective" => {
                value.as_str().is_some_and(|s| !s.trim().is_empty())
            }
            "area" | "relationships" | "layout" => value.is_object(),
            _ => value.is_array(),
        };
        if !valid {
            return Err(format!("Invalid shape for {key}"));
        }
    }
    if let Some(a) = fields.get("area") {
        let target = a["target"].as_str().ok_or("Area needs a target")?;
        if !target.starts_with("scope:") && !store.graph.entities.contains_key(target) {
            return Err(format!("Unknown area {target}"));
        }
        if !matches!(a["extent"].as_str(), Some("area" | "selection")) {
            return Err("Invalid area extent".into());
        }
    }
    if fields
        .get("perspective")
        .is_some_and(|p| !matches!(p.as_str(), Some("context" | "container" | "component")))
    {
        return Err("Unknown diagram perspective".into());
    }
    for key in ["labels", "notes"] {
        for item in fields
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if item["text"].as_str().is_none_or(|s| s.trim().is_empty()) {
                return Err(format!("{key} needs text"));
            }
            if key == "labels"
                && item["target"]
                    .as_str()
                    .is_none_or(|id| !reference_exists(store, id))
            {
                return Err("A display label needs a live entity or requirement target".into());
            }
            if key == "notes" {
                for id in item["targets"].as_array().into_iter().flatten() {
                    if id.as_str().is_none_or(|id| !reference_exists(store, id)) {
                        return Err("Unknown note target".into());
                    }
                }
            }
        }
    }
    for link in fields
        .get("links")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if link["view"]
            .as_str()
            .is_none_or(|id| !store.graph.views.contains_key(id))
        {
            return Err("Unknown linked diagram".into());
        }
        if !matches!(link["relation"].as_str(), Some("detail" | "phase")) {
            return Err("Unknown diagram link relation".into());
        }
    }
    if let Some(layout) = fields.get("layout") {
        if layout.as_object().is_some_and(|o| {
            o.keys()
                .any(|k| !["direction", "together"].contains(&k.as_str()))
        }) {
            return Err("Unknown layout hint".into());
        }
        if let Some(d) = layout.get("direction") {
            if !matches!(d.as_str(), Some("left-to-right" | "top-to-bottom")) {
                return Err("Unknown diagram direction".into());
            }
        }
    }
    if let Some(together) = fields.get("layout").and_then(|l| l.get("together")) {
        if together.as_array().is_none_or(|a| !a.is_empty()) {
            return Err("Together hints are unsupported; use presentation groups".into());
        }
    }
    if fields
        .get("groups")
        .and_then(Value::as_array)
        .is_some_and(|g| !g.is_empty())
        && !["class", "component", "object"].contains(&kind)
    {
        return Err(
            "Presentation groups are supported for class, component and object diagrams".into(),
        );
    }
    if let Some(groups) = fields.get("groups").and_then(Value::as_array) {
        let mut ids = BTreeSet::new();
        let mut parents = BTreeMap::new();
        let mut members = BTreeSet::new();
        for g in groups {
            let id = g["id"].as_str().ok_or("Group needs an ID")?;
            if !ids.insert(id) {
                return Err("Duplicate group ID".into());
            }
            if let Some(p) = g["parent"].as_str() {
                parents.insert(id, p);
            }
            for m in g["members"].as_array().ok_or("Group needs members")? {
                let m = m.as_str().ok_or("Group member must be an ID")?;
                if !members.insert(m) {
                    return Err("Element belongs to several groups".into());
                }
            }
        }
        for id in &ids {
            let mut cur = *id;
            let mut seen = BTreeSet::new();
            while let Some(p) = parents.get(cur) {
                if !ids.contains(p) {
                    return Err("Unknown parent group".into());
                }
                if !seen.insert(cur) {
                    return Err("Cyclic groups".into());
                }
                cur = p;
            }
        }
    }
    if let Some(rel) = fields.get("relationships") {
        if !matches!(rel["mode"].as_str(), Some("all" | "selected")) {
            return Err("Unknown relationship mode".into());
        }
        for s in rel["include"].as_array().into_iter().flatten().chain(
            rel["exclude"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|x| &x["selector"]),
        ) {
            if ["a", "b", "type"].iter().any(|k| s[*k].as_str().is_none()) {
                return Err("Invalid relationship selector".into());
            }
            if !store
                .graph
                .relationships
                .values()
                .flat_map(|r| &r.contributions)
                .any(|c| s["a"] == c.a && s["b"] == c.b && s["type"] == c.r#type)
            {
                return Err("Unknown relationship selector".into());
            }
        }
        for x in rel["exclude"].as_array().into_iter().flatten() {
            if x["reason"].as_str().is_none_or(|s| s.trim().is_empty()) {
                return Err("Relationship exclusion needs a reason".into());
            }
        }
    }
    if fields
        .get("fragments")
        .is_some_and(|x| x.as_array().is_some_and(|a| !a.is_empty()))
        && kind != "sequence"
    {
        return Err("Fragments require sequence kind".into());
    }
    fn fragments(
        store: &Store,
        items: &[Value],
        seen: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        for f in items {
            let id = f["id"].as_str().ok_or("Fragment needs ID")?;
            if !seen.insert(id.to_string()) {
                return Err("Duplicate fragment ID".into());
            }
            if !matches!(
                f["operator"].as_str(),
                Some("alt" | "opt" | "loop" | "par" | "break" | "critical" | "ref")
            ) {
                return Err("Unknown fragment operator".into());
            }
            if f["operator"] == "ref"
                && f["view"]
                    .as_str()
                    .and_then(|id| store.graph.views.get(id))
                    .is_none_or(|v| v.kind != "sequence")
            {
                return Err("Reference fragment needs a sequence diagram".into());
            }
            let operands = f["operands"].as_array().ok_or("Fragment needs operands")?;
            if operands.is_empty() {
                return Err("Fragment needs an operand".into());
            }
            for operand in operands {
                if let Some(guard) = operand.get("guard") {
                    if guard["text"].as_str().is_none_or(|s| s.trim().is_empty())
                        || guard["sources"].as_array().is_none_or(|s| s.is_empty())
                    {
                        return Err("Fragment guard needs sources".into());
                    }
                    if guard["sources"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|id| id.as_str().is_none_or(|id| !reference_exists(store, id)))
                    {
                        return Err("Fragment guard source is missing".into());
                    }
                }
                if let Some(nested) = operand["fragments"].as_array() {
                    fragments(store, nested, seen)?;
                }
            }
        }
        Ok(())
    }
    if let Some(fs) = fields.get("fragments").and_then(Value::as_array) {
        fragments(store, fs, &mut BTreeSet::new())?;
    }
    Ok(())
}

pub fn shows_relationship(view: &View, c: &Contribution) -> bool {
    let Some(rel) = view.authored.get("relationships") else {
        return true;
    };
    let matches = |s: &Value| {
        s["a"].as_str() == Some(&c.a)
            && s["b"].as_str() == Some(&c.b)
            && s["type"].as_str() == Some(&c.r#type)
    };
    (rel["mode"] != "selected" || rel["include"].as_array().into_iter().flatten().any(matches))
        && !rel["exclude"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|x| matches(&x["selector"]))
}

/// A rendering copy, with authored selection and display labels applied before lifting.
pub fn projection(store: &Store, view: &View) -> Store {
    let mut out = store.clone();
    for r in out.graph.relationships.values_mut() {
        r.contributions.retain(|c| shows_relationship(view, c));
    }
    for label in view
        .authored
        .get("labels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(id), Some(text)) = (label["target"].as_str(), label["text"].as_str()) {
            if let Some(e) = out.graph.entities.get_mut(id) {
                e.name = text.to_string();
            }
            if let Some(r) = out.graph.requirements.get_mut(id) {
                r.statement = text.to_string();
            }
        }
    }
    out
}

fn semantic_hash(mut value: Value) -> String {
    if let Some(o) = value.as_object_mut() {
        o.remove("created");
        o.remove("updated");
    }
    hash_hex(&value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn flat(n: usize) -> Store {
        let mut s = Store::default();
        for i in 0..n {
            s.graph.entities.insert(
                format!("ent:item-{i}"),
                Entity {
                    name: format!("Item {i}"),
                    ..Default::default()
                },
            );
        }
        s
    }
    #[test]
    fn structural_thresholds_and_suppression_use_cumulative_semantic_growth() {
        assert!(findings(&flat(3)).is_empty());
        let mut s = flat(5);
        let f = findings(&s).remove(0);
        assert_eq!((f.soft, f.hard), (3, 9));
        assert!(!f.covered(&s));
        let mut d = f.value();
        d["ruleVersion"] = json!(1);
        d["reason"] = json!("A flat list is clearer");
        s.decisions.diagram_coverage.insert(f.id.clone(), d);
        assert!(f.suppressed(&s));
        s.graph.entities.get_mut("ent:item-0").unwrap().updated = Some("g9".into());
        assert!(findings(&s)[0].suppressed(&s));
        s.graph.entities.insert(
            "ent:item-5".into(),
            Entity {
                name: "New item".into(),
                ..Default::default()
            },
        );
        assert!(!findings(&s)[0].suppressed(&s));
    }
    #[test]
    fn diagram_coverage_requires_compatible_kind_and_area() {
        let mut s = flat(4);
        let f = findings(&s).remove(0);
        let mut view = View {
            kind: "class".into(),
            members: s.graph.entities.keys().cloned().collect(),
            ..Default::default()
        };
        s.graph
            .views
            .insert("view:class/items".into(), view.clone());
        assert!(!f.covered(&s));
        view.authored.insert(
            "area".into(),
            json!({"target":"scope:public","extent":"area"}),
        );
        s.graph.views.insert("view:class/items".into(), view);
        assert!(f.covered(&s));
        assert_eq!(references(&s, "ent:item-0").len(), 1);
    }
    #[test]
    fn invalid_presentation_is_rejected() {
        let s = flat(1);
        let f = BTreeMap::from([(
            "groups".into(),
            json!([{"id":"a","parent":"b","members":[]},{"id":"b","parent":"a","members":[]}]),
        )]);
        assert!(validate_fields(&s, "class", &f).is_err());
        let f = BTreeMap::from([(
            "fragments".into(),
            json!([{"id":"x","operator":"alt","operands":[{"guard":{"text":"invented"}}]}]),
        )]);
        assert!(validate_fields(&s, "sequence", &f).is_err());
    }
}

pub fn list(store: &Store, args: &Value) -> Value {
    let ids: Option<BTreeSet<String>> = args["entity"].as_str().map(|e| {
        references(store, e)
            .into_iter()
            .filter_map(|v| v["id"].as_str().map(String::from))
            .collect()
    });
    let entries:Vec<Value>=store.graph.views.iter().filter(|(id,v)| ids.as_ref().is_none_or(|ids|ids.contains(*id)) && args["kind"].as_str().is_none_or(|k|v.kind==k) && args["area"].as_str().is_none_or(|a|area(v)==Some(a))).map(|(id,v)|json!({"id":id,"definition":v,"kind":v.kind,"title":v.title,"purpose":v.authored.get("purpose"),"area":v.authored.get("area"),"members":v.members.len()})).collect();
    let offset = args["offset"]
        .as_u64()
        .or_else(|| args["offset"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0) as usize;
    let limit = (args["limit"]
        .as_u64()
        .or_else(|| args["limit"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(16) as usize)
        .clamp(1, 64);
    let total = entries.len();
    let end = offset.saturating_add(limit).min(total);
    json!({"views":entries.into_iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":total,"complete":end>=total,"next":if end<total {Some(end)} else {None}})
}

pub fn suppressions(store: &Store) -> Value {
    let fs = findings(store);
    json!({"suppressions":store.decisions.diagram_coverage.iter().map(|(id,d)|json!({"id":id,"decision":d,"active":fs.iter().find(|f|&f.id==id).is_some_and(|f|f.suppressed(store))})).collect::<Vec<_>>()})
}

pub fn authored_references(view: &View) -> Vec<String> {
    fn walk(value: &Value, out: &mut BTreeSet<String>) {
        match value {
            Value::String(s)
                if s.starts_with("ent:")
                    || s.starts_with("req:")
                    || s.starts_with("crit:")
                    || s.starts_with("view:")
                    || s.starts_with("scope:") =>
            {
                out.insert(s.clone());
            }
            Value::Array(a) => {
                for v in a {
                    walk(v, out)
                }
            }
            Value::Object(o) => {
                for (k, v) in o {
                    if ![
                        "text",
                        "purpose",
                        "description",
                        "title",
                        "reason",
                        "reasoning",
                    ]
                    .contains(&k.as_str())
                    {
                        walk(v, out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut refs = BTreeSet::new();
    for (key, value) in &view.authored {
        if !["purpose", "description"].contains(&key.as_str()) {
            walk(value, &mut refs);
        }
    }
    refs.into_iter().collect()
}
pub fn reference_exists(store: &Store, id: &str) -> bool {
    if let Some((doc, section)) = split_section_ref(id) {
        return store
            .docs
            .get(&doc)
            .is_some_and(|r| r.sections.contains_key(&section));
    }
    let id = store.resolve_id(id);
    id.starts_with("scope:")
        || store.graph.entities.contains_key(id)
        || store.graph.requirements.contains_key(id)
        || store.graph.criteria.contains_key(id)
        || store.graph.views.contains_key(id)
}

pub fn rewrite_authored(fields: &mut BTreeMap<String, Value>, rewrite: &impl Fn(&mut String)) {
    fn walk(v: &mut Value, rewrite: &impl Fn(&mut String)) {
        match v {
            Value::String(s)
                if s.starts_with("ent:") || s.starts_with("req:") || s.starts_with("view:") =>
            {
                rewrite(s)
            }
            Value::Array(a) => {
                for v in a {
                    walk(v, rewrite)
                }
            }
            Value::Object(o) => {
                for (k, v) in o {
                    if ![
                        "text",
                        "purpose",
                        "description",
                        "title",
                        "reason",
                        "reasoning",
                    ]
                    .contains(&k.as_str())
                    {
                        walk(v, rewrite);
                    }
                }
            }
            _ => {}
        }
    }
    for (key, value) in fields {
        if !["purpose", "description"].contains(&key.as_str()) {
            walk(value, rewrite);
        }
    }
}

pub fn validate_members(
    members: &[String],
    fields: &BTreeMap<String, Value>,
) -> Result<(), String> {
    for group in fields
        .get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for m in group["members"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !members.iter().any(|id| id == m) {
                return Err(format!("Group names nonmember {m}"));
            }
        }
    }
    fn fragments(
        fs: &[Value],
        members: &[String],
        used: &mut BTreeSet<String>,
    ) -> Result<Vec<usize>, String> {
        let mut positions = Vec::new();
        for f in fs {
            let mut local = Vec::new();
            for o in f["operands"].as_array().into_iter().flatten() {
                for id in o["members"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    let position = members
                        .iter()
                        .position(|m| m == id)
                        .ok_or_else(|| format!("Fragment names nonmember {id}"))?;
                    if !used.insert(id.to_string()) {
                        return Err(format!("Message {id} occurs in several fragments"));
                    }
                    local.push(position);
                }
                if let Some(nested) = o["fragments"].as_array() {
                    local.extend(fragments(nested, members, used)?);
                }
            }
            if let (Some(min), Some(max)) = (local.iter().min(), local.iter().max()) {
                if max - min + 1 != local.len() {
                    return Err("Fragment crosses unrelated messages".into());
                }
            }
            positions.extend(local);
        }
        Ok(positions)
    }
    if let Some(fs) = fields.get("fragments").and_then(Value::as_array) {
        fragments(fs, members, &mut BTreeSet::new())?;
    }
    Ok(())
}

pub fn effective_members(store: &Store, view: &View) -> Vec<String> {
    let mut members = view
        .members
        .iter()
        .map(|id| store.resolve_id(id).to_string())
        .collect::<Vec<_>>();
    if let Some(query) = &view.query {
        for id in crate::derive::query_matches(
            store,
            query,
            &view.excluded,
            &crate::derive::instance_types(store),
        ) {
            if !members.contains(&id) {
                members.push(id);
            }
        }
    }
    members.retain(|id| {
        !view.excluded.iter().any(|e| store.resolve_id(&e.id) == id) && reference_exists(store, id)
    });
    members
}

#[cfg(test)]
mod coverage_tests {
    use super::*;

    fn specimen(family: &str, n: usize) -> Store {
        let mut s = Store::default();
        s.graph.entities.insert(
            "ent:subject".into(),
            Entity {
                name: "Subject".into(),
                stereotype: (family == "system").then(|| "system".into()),
                ..Default::default()
            },
        );
        for i in 0..n {
            let id = format!("ent:item-{i}");
            s.graph.entities.insert(
                id.clone(),
                Entity {
                    name: format!("Item {i}"),
                    parent: Some("ent:subject".into()),
                    ..Default::default()
                },
            );
            if family == "object" {
                s.graph.requirements.insert(
                    format!("req:instance-{i}"),
                    Requirement {
                        statement: format!("Item {i} is a Subject"),
                        entities: vec![id.clone(), "ent:subject".into()],
                        edges: vec![ReqEdge {
                            a: id.clone(),
                            b: "ent:subject".into(),
                            rel_type: Some("instantiation".into()),
                            cardinality: None,
                        }],
                        ..Default::default()
                    },
                );
            }
            if family == "deployment" {
                s.graph
                    .entities
                    .get_mut(&id)
                    .unwrap()
                    .attributes
                    .push(Attribute {
                        name: "region".into(),
                        r#type: None,
                        value: Some("EU".into()),
                        provenance: Provenance::Derived {
                            from: vec![id],
                            reasoning: "Fixture placement".into(),
                        },
                    });
            }
            if family == "flow" {
                s.graph.requirements.insert(
                    format!("req:behavior-{i}"),
                    Requirement {
                        statement: format!("Item {i} performs an operation"),
                        entities: vec![format!("ent:item-{i}")],
                        sources: vec![SourceRef {
                            doc: "process.md".into(),
                            section: "/process".into(),
                            quote: format!("operation {i}"),
                        }],
                        ..Default::default()
                    },
                );
                s.graph.criteria.insert(
                    format!("crit:scenario-{i}"),
                    Criterion {
                        requirement: format!("req:behavior-{i}"),
                        statement: format!("Operation {i} completes"),
                        when: Some(format!("Operation {i} starts")),
                        ..Default::default()
                    },
                );
            }
        }
        crate::derive::recompute_relationships(&mut s);
        if family == "state" {
            s.graph.state_machines.insert(
                "sm:subject".into(),
                StateMachine {
                    subject: "ent:subject".into(),
                    states: (0..n).map(|i| format!("state-{i}")).collect(),
                    initial: None,
                    transitions: vec![],
                },
            );
        }
        s
    }
    #[test]
    fn review_thresholds_change_advice_to_required_without_creating_diagrams() {
        for (family, soft, hard) in [
            ("structure", 3, 9),
            ("system", 0, 7),
            ("flow", 3, 11),
            ("state", 2, 7),
            ("object", 1, 7),
            ("deployment", 0, 5),
        ] {
            for (n, exists, mandatory) in [
                (soft, false, false),
                (soft + 1, true, false),
                (hard, true, false),
                (hard + 1, true, true),
            ] {
                let s = specimen(family, n);
                let goals = crate::goals::kind("diagram-coverage")
                    .unwrap()
                    .derive_goals(
                        &s,
                        &crate::gen::GenSettings::resolve(&crate::project::Project::default()),
                    );
                let goal = goals.iter().find(|g| g.change["family"] == family);
                // A system with no internal units still deserves a boundary suggestion.
                let expected = exists || family == "system";
                assert_eq!(goal.is_some(), expected, "{family} at {n}: {goals:?}");
                if let Some(g) = goal {
                    assert_eq!(g.mandatory, mandatory, "{family} at {n}");
                }
                assert!(s.graph.views.is_empty());
            }
        }
    }
    #[test]
    fn coverage_uses_query_matches_and_honors_exclusions() {
        let mut s = specimen("structure", 5);
        let f = findings(&s)
            .into_iter()
            .find(|f| f.family == "structure")
            .unwrap();
        let mut v = View {
            kind: "class".into(),
            query: Some(ViewQuery {
                parent: Some("ent:subject".into()),
                ..Default::default()
            }),
            authored: BTreeMap::from([(
                "area".into(),
                json!({"target":"ent:subject","extent":"area"}),
            )]),
            ..Default::default()
        };
        s.graph.views.insert("view:class/parts".into(), v.clone());
        assert!(f.covered(&s));
        v.excluded = (0..2)
            .map(|i| Exclusion {
                id: format!("ent:item-{i}"),
                note: "Outside this question".into(),
            })
            .collect();
        s.graph.views.insert("view:class/parts".into(), v);
        assert!(!f.covered(&s));
    }
    #[test]
    fn timing_suggestions_use_criterion_bounds_without_requirement_classification() {
        let mut s = specimen("state", 3);
        s.graph.requirements.insert(
            "req:deadline".into(),
            Requirement {
                statement: "Subject reaches ready before the deadline".into(),
                entities: vec!["ent:subject".into()],
                ..Default::default()
            },
        );
        assert!(!findings(&s).iter().any(|f| f.family == "timing"));
        s.graph.criteria.insert(
            "crit:deadline".into(),
            Criterion {
                requirement: "req:deadline".into(),
                statement: "Ready is reached within2seconds".into(),
                measure: Some("2s".into()),
                kind: "quality".into(),
                ..Default::default()
            },
        );
        let f = findings(&s)
            .into_iter()
            .find(|f| f.family == "timing")
            .unwrap();
        assert!(f.candidates.contains_key("crit:deadline"));
        assert_eq!(f.hard, usize::MAX);
        assert!(s.graph.views.is_empty());
    }
    #[test]
    fn object_type_context_and_boundary_changes_are_visible() {
        let mut s = specimen("object", 5);
        s.graph.views.insert(
            "view:object/examples".into(),
            View {
                kind: "object".into(),
                members: vec!["ent:item-0".into(), "ent:item-1".into()],
                ..Default::default()
            },
        );
        assert_eq!(
            references(&s, "ent:subject")[0]["reasons"],
            json!(["type-subject"])
        );
        let f = findings(&s)
            .into_iter()
            .find(|f| f.family == "object")
            .unwrap();
        let mut d = f.value();
        d["ruleVersion"] = json!(1);
        s.decisions.diagram_coverage.insert(f.id.clone(), d);
        assert!(f.suppressed(&s));
        s.graph.entities.get_mut("ent:subject").unwrap().scope = "private".into();
        let moved = findings(&s)
            .into_iter()
            .find(|f| f.family == "object")
            .unwrap();
        assert!(!moved.suppressed(&s));
    }
}
