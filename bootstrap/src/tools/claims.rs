use super::*;

impl ToolSession {
    fn undecided_proposals(&self, proposals: &[String]) -> Vec<String> {
        crate::goals::undecided_proposals(&self.staged, proposals)
    }

    fn untouched_stale(&self, anchors: &[String]) -> Vec<String> {
        crate::goals::untouched_stale(&self.snapshot, &self.staged, anchors)
    }

    // A name that starts with an article is a proper name when a document writes it
    // capitalized mid-sentence ("upstairs at The Lantern Room"): the article is part of
    // the name, and the junk-name rule stands aside. Mirrors docs/compiler/tools.md.
    pub(super) fn proper_name_in_docs(&self, name: &str) -> bool {
        let capitalized_article = ["The ", "A ", "An "]
            .iter()
            .any(|a| name.starts_with(a) && name.len() > a.len());
        if !capitalized_article {
            return false;
        }
        self.snapshot.docs.values().any(|d| {
            d.sections.values().any(|sec| {
                sec.raw.match_indices(name).any(|(i, _)| {
                    let before: Vec<char> = sec.raw[..i].chars().rev().take(2).collect();
                    before.len() == 2 && before[0] == ' ' && before[1].is_lowercase()
                })
            })
        })
    }

    // A criterion id the store holds or this session staged.
    pub(super) fn criterion_known(&self, id: &str) -> bool {
        self.snapshot.graph.criteria.contains_key(id)
            || self
                .staged
                .iter()
                .any(|o| matches!(o, Op::CreateCriterion { id: cid, .. } if cid == id))
    }

    // The statement of a requirement, recorded or staged this session.
    pub(super) fn requirement_statement(&self, rid: &str) -> Option<String> {
        self.staged
            .iter()
            .rev()
            .find_map(|o| match o {
                Op::CreateRequirement { id, requirement } if id == rid => {
                    Some(requirement.statement.clone())
                }
                Op::UpdateRequirement {
                    id,
                    statement: Some(st),
                    ..
                } if id == rid => Some(st.clone()),
                _ => None,
            })
            .or_else(|| {
                self.snapshot
                    .graph
                    .requirements
                    .get(rid)
                    .map(|r| r.statement.clone())
            })
    }

    // Whether a requirement names an entity after this session's staged updates.
    fn requirement_names_entity(&self, rid: &str, entity: &str) -> bool {
        let names = |es: &[String]| es.iter().any(|e| self.snapshot.resolve_id(e) == entity);
        let staged = self.staged.iter().rev().find_map(|o| match o {
            Op::UpdateRequirement {
                id,
                entities: Some(es),
                ..
            } if id == rid => Some(names(es)),
            Op::CreateRequirement { id, requirement } if id == rid => {
                Some(names(&requirement.entities))
            }
            _ => None,
        });
        staged.unwrap_or_else(|| {
            self.snapshot
                .graph
                .requirements
                .get(rid)
                .is_some_and(|r| names(&r.entities))
        })
    }

    fn unmarked_sections(&self, doc: &str, sections: &[String]) -> Vec<String> {
        crate::goals::unmarked_sections(&self.snapshot, &self.staged, doc, sections)
    }

    // A `covered` claim is honest only when a requirement is sourced from that
    // section, staged or recorded.
    fn dishonest_covered(&self) -> Option<ToolError> {
        crate::goals::dishonest_covered(&self.snapshot, &self.staged)
            .map(|v| ToolError::new(&v.rule, v.message))
    }

    // One goal's gate over the store plus the staged work: the kind's batch gate and
    // the scope gates that key on the owning goal. mark_goal_done validates against
    // this and done re-validates every resolution.
    // Mirrors docs/compiler/tools.md#goal-tools.
    fn goal_gate(&self, gs: &GoalScope) -> Result<(), ToolError> {
        let Some(k) = crate::goals::kind(&gs.kind) else {
            return Ok(());
        };
        if let Some(v) = k.gates(&self.snapshot, &self.staged).into_iter().next() {
            return Err(ToolError::new(&v.rule, v.message));
        }
        let cx = crate::goals::GateContext {
            store: &self.snapshot,
            staged: &self.staged,
            gen: &self.gen,
            goal: gs,
            batch: &self.scope.goals,
        };
        match k.goal_gate(&cx) {
            Some(v) => Err(ToolError::new(&v.rule, v.message)),
            None => Ok(()),
        }
    }

    // A sentence boundary is [.!?] followed by the end of the text, or by whitespace
    // and a sentence opener (an uppercase letter, a digit, a quote, a bracket), so a
    // dot inside `customer.md` or a versioned id never counts a phantom sentence and
    // an abbreviation running into lowercase (`vs. its member`, `e.g. the cart`)
    // never ends one (a naive split trained models to strip document names and
    // plain English from justifications, three bounced claims for one `vs.`).
    pub(super) fn sentence_count(text: &str) -> usize {
        let chars: Vec<char> = text.chars().collect();
        let mut count = 0usize;
        let mut has_content = false;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if matches!(c, '.' | '!' | '?') {
                let mut j = i + 1;
                while j < chars.len() && matches!(chars[j], '.' | '!' | '?') {
                    j += 1;
                }
                let at_end = j >= chars.len();
                let boundary = at_end
                    || (chars[j].is_whitespace() && {
                        let mut k = j;
                        while k < chars.len() && chars[k].is_whitespace() {
                            k += 1;
                        }
                        k >= chars.len()
                            || chars[k].is_uppercase()
                            || chars[k].is_ascii_digit()
                            || matches!(chars[k], '"' | '\'' | '(' | '[' | '`' | '«')
                    });
                if boundary {
                    if has_content {
                        count += 1;
                        has_content = false;
                    }
                    i = j;
                    continue;
                }
            }
            if !c.is_whitespace() {
                has_content = true;
            }
            i += 1;
        }
        if has_content {
            count += 1;
        }
        count
    }

    // An evidence entry's id: its first token, so "ent:backend (Backend)" names
    // ent:backend, resolved through merges.
    fn evidence_id(e: &Value, key: &str) -> String {
        let raw = match &e[key] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        raw.split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches(|c: char| matches!(c, ',' | ';' | '"' | '\''))
            .to_string()
    }

    // What arrived where evidence was expected, for a refusal that says so.
    fn evidence_shape(evidence: &Value) -> String {
        match evidence {
            Value::Null => "no evidence".to_string(),
            Value::String(s) => format!("the string `{}`", crate::llm::truncate(s, 60)),
            Value::Array(_) => "a bare array".to_string(),
            Value::Object(m) => format!(
                "an object with keys {}",
                m.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
            other => other.to_string(),
        }
    }

    // A verdict handed bare ("declared") or as {verdict: "declared"}.
    fn evidence_verdict(evidence: &Value) -> Option<String> {
        evidence
            .as_str()
            .or_else(|| evidence["verdict"].as_str())
            .map(|s| s.trim().to_string())
    }

    // Whether a reason says something: a placeholder counts as absent.
    fn reason_given(e: &Value) -> bool {
        let r = e["reason"].as_str().unwrap_or("").trim().to_lowercase();
        !r.is_empty() && !matches!(r.as_str(), "none" | "n/a" | "na" | "-" | "null" | "tbd")
    }

    fn staged_resolves(&self, did: &str) -> bool {
        self.staged
            .iter()
            .any(|o| matches!(o, Op::ResolveDiagnostic { id, .. } if id == did))
    }

    // The open diagnostics, recorded and staged, that no staged resolve closes.
    fn open_diagnostics(&self) -> Vec<(String, Diagnostic)> {
        self.snapshot
            .graph
            .diagnostics
            .iter()
            .chain(self.staged_diags.iter())
            .filter(|(id, d)| d.lifecycle == "open" && !self.staged_resolves(id))
            .map(|(id, d)| (id.clone(), d.clone()))
            .collect()
    }

    fn staged_merge_of(&self, a: &str, b: &str) -> bool {
        let r = |x: &str| self.snapshot.resolve_id(x).to_string();
        let (a, b) = (r(a), r(b));
        self.staged.iter().any(|o| match o {
            Op::MergeEntities { keep, absorb, .. } => {
                let (k, x) = (r(keep), r(absorb));
                (k == a && x == b) || (k == b && x == a)
            }
            _ => false,
        })
    }

    // Whether the part sits under the whole once the staged work applies.
    fn under_after(&self, part: &str, whole: &str) -> bool {
        let whole = self.snapshot.resolve_id(whole).to_string();
        let mut cur = part.to_string();
        for _ in 0..64 {
            match self.parent_of(&cur) {
                Some(p) => {
                    if self.snapshot.resolve_id(&p) == whole {
                        return true;
                    }
                    cur = p;
                }
                None => return false,
            }
        }
        false
    }

    // A view as the staged work leaves it: the stored or staged-created view with
    // every staged update replayed in order; None when deleted or unknown.
    fn view_after(&self, id: &str) -> Option<View> {
        let mut v = self.snapshot.graph.views.get(id).cloned().or_else(|| {
            self.staged.iter().find_map(|o| match o {
                Op::CreateView { id: c, view } if c == id => Some(view.clone()),
                _ => None,
            })
        })?;
        for o in &self.staged {
            match o {
                Op::DeleteView { id: d, .. } if d == id => return None,
                Op::UpdateView {
                    id: u,
                    members,
                    add_members,
                    remove_members,
                    exclude,
                    ..
                } if u == id => {
                    if let Some(ms) = members {
                        v.members = ms.clone();
                    }
                    for m in add_members {
                        if !v.members.contains(m) {
                            v.members.push(m.clone());
                        }
                    }
                    v.members.retain(|m| !remove_members.contains(m));
                    for x in exclude {
                        v.members.retain(|m| m != &x.id);
                        v.excluded.retain(|y| y.id != x.id);
                        v.excluded.push(x.clone());
                    }
                }
                _ => {}
            }
        }
        Some(v)
    }

    // The review's evidence beyond lint: the definition claim agrees with the staged
    // work, and every lookalike, open diagnostic, unparented part, and unreferenced
    // statement the hints list carries a verdict the staged work backs. Mirrors
    // docs/compiler/goals/review-entity.md#gate.
    fn review_evidence(&self, goal: &str, target: &str, evidence: &Value) -> Result<(), ToolError> {
        let target = self.snapshot.resolve_id(target).to_string();
        let shape = "{definition: current | refreshed, lookalikes: [...], unreferenced: [...], diagnostics: [...], parts: [...], lint: [...]}";
        let refreshed_here = self.staged.iter().any(|o| {
            matches!(o, Op::UpdateEntity { id, definition: Some(_), .. } if self.snapshot.resolve_id(id) == target)
        });
        match evidence["definition"].as_str().map(str::trim) {
            Some("current") if refreshed_here => {
                return Err(ToolError::new(
                    "definition-unbacked",
                    format!("{} claims the definition current, but this session stages a new definition on {}; claim refreshed", goal, target),
                ));
            }
            Some("refreshed") if !refreshed_here => {
                return Err(ToolError::new(
                    "definition-unbacked",
                    format!("{} claims the definition refreshed, but no update_entity with definition on {} is staged; stage it, or claim current", goal, target),
                ));
            }
            Some("current") | Some("refreshed") => {}
            _ => {
                return Err(ToolError::new(
                    "bad-evidence",
                    format!(
                        "{} carries evidence {} with definition current or refreshed; received {}",
                        goal,
                        shape,
                        Self::evidence_shape(evidence)
                    ),
                ));
            }
        }
        let find = |key: &str, field: &str, id: &str| -> Option<Value> {
            evidence[key].as_array().and_then(|a| {
                a.iter()
                    .find(|e| {
                        let eid = Self::evidence_id(e, field);
                        eid == id || self.snapshot.resolve_id(&eid) == id
                    })
                    .cloned()
            })
        };
        // Lookalikes: one verdict per listed candidate.
        let (similar, related) = crate::goals::lookalikes(&self.snapshot, &target);
        for (lid, name) in similar.iter().chain(related.iter()) {
            let Some(e) = find("lookalikes", "id", lid) else {
                return Err(ToolError::new(
                    "lookalike-unjudged",
                    format!("{} has no lookalikes entry for {} ({}); judge it and add {{id: {}, verdict: merged | separate, reason}}", goal, lid, name, lid),
                ));
            };
            match e["verdict"].as_str() {
                Some("merged") => {
                    if !self.staged_merge_of(&target, lid) {
                        return Err(ToolError::new(
                            "merge-unbacked",
                            format!("{} claims {} merged, but no merge_entities naming {} and {} is staged; merge them, or verdict separate with the reason", goal, lid, target, lid),
                        ));
                    }
                }
                Some("separate") => {
                    let filed = self.open_diagnostics().iter().any(|(_, d)| {
                        d.rule == "duplicate-entity"
                            && d.subjects
                                .iter()
                                .any(|s| self.snapshot.resolve_id(s) == target)
                            && d.subjects
                                .iter()
                                .any(|s| self.snapshot.resolve_id(s) == lid.as_str())
                    });
                    if !Self::reason_given(&e) && !filed {
                        return Err(ToolError::new(
                            "reason-required",
                            format!("{} keeps {} separate without a reason; say what keeps it apart (a part, a role, a threshold, an instance), or file duplicate-entity naming both when the merge is not certain", goal, lid),
                        ));
                    }
                }
                other => {
                    return Err(ToolError::new(
                        "bad-verdict",
                        format!(
                            "lookalike verdict `{}` for {}: merged or separate",
                            other.unwrap_or("?"),
                            lid
                        ),
                    ));
                }
            }
        }
        // Open diagnostics on the entity and its statements, as committed when the
        // batch was claimed.
        let alive = |s: &str| {
            let r = self.snapshot.resolve_id(s);
            self.snapshot.graph.entities.contains_key(r)
                || self.snapshot.graph.requirements.contains_key(r)
        };
        for did in crate::goals::review_diagnostics(&self.snapshot, &target) {
            let Some(e) = find("diagnostics", "id", &did) else {
                return Err(ToolError::new(
                    "diagnostic-unjudged",
                    format!("{} has no diagnostics entry for {}, open on {} or its statements; judge it and add {{id: {}, verdict: stands | resolved | refiled}}", goal, did, target, did),
                ));
            };
            let d = &self.snapshot.graph.diagnostics[&did];
            let resolved = self.staged_resolves(&did);
            match e["verdict"].as_str() {
                Some("stands") => {
                    if resolved {
                        return Err(ToolError::new(
                            "diagnostic-unbacked",
                            format!("{} claims {} stands, but this session resolves it; verdict resolved or refiled", goal, did),
                        ));
                    }
                    if let Some(dead) = d.subjects.iter().find(|s| !alive(s)) {
                        return Err(ToolError::new(
                            "diagnostic-unbacked",
                            format!("{} names {}, which is deleted, so it cannot stand as filed; resolve_diagnostic it and, when the finding stands, report it again against the surviving statements (verdict refiled)", did, dead),
                        ));
                    }
                }
                Some("resolved") => {
                    if !resolved {
                        return Err(ToolError::new(
                            "diagnostic-unbacked",
                            format!("{} claims {} resolved, but no resolve_diagnostic on it is staged; resolve it with the reason, or verdict stands", goal, did),
                        ));
                    }
                }
                Some("refiled") => {
                    let refiled = self.staged.iter().any(|o| {
                        matches!(o, Op::ReportDiagnostic { diagnostic, .. } if diagnostic.rule == d.rule)
                    });
                    if !resolved || !refiled {
                        return Err(ToolError::new(
                            "diagnostic-unbacked",
                            format!("{} claims {} refiled, which takes resolve_diagnostic on it and a new report_diagnostic rule {} against the surviving statements, both staged", goal, did, d.rule),
                        ));
                    }
                }
                other => {
                    return Err(ToolError::new(
                        "bad-verdict",
                        format!(
                            "diagnostic verdict `{}` for {}: stands, resolved, or refiled",
                            other.unwrap_or("?"),
                            did
                        ),
                    ));
                }
            }
        }
        // Stated compositions whose part sits outside its whole.
        for (whole, part, line) in crate::goals::unparented_parts(&self.snapshot, &target) {
            let Some(e) = find("parts", "id", &part) else {
                return Err(ToolError::new(
                    "part-unjudged",
                    format!("{} has no parts entry for {}: the composition {} leaves it outside its stated whole; set parent on it with update_entity and add {{id: {}, verdict: parented}}, fix a reversed edge (verdict edge-fixed), or keep a generic rule-carrier parentless (verdict kept, with the reason)", goal, part, line, part),
                ));
            };
            match e["verdict"].as_str() {
                Some("parented") => {
                    if !self.under_after(&part, &whole) {
                        return Err(ToolError::new(
                            "part-unbacked",
                            format!("{} claims {} parented, but after this session's staged work its parent is not {} or under it; update_entity {} with parent {}", goal, part, whole, part, whole),
                        ));
                    }
                }
                Some("edge-fixed") => {
                    let fixed = self.staged.iter().any(|o| match o {
                        Op::UpdateRequirement {
                            id,
                            edges: Some(es),
                            ..
                        } => {
                            self.snapshot.graph.requirements.get(id).is_some_and(|r| {
                                r.edges.iter().any(|x| {
                                    x.rel_type.as_deref() == Some("composition")
                                        && x.a == whole
                                        && x.b == part
                                })
                            }) && !es.iter().any(|x| {
                                x.rel_type.as_deref() == Some("composition")
                                    && x.a == whole
                                    && x.b == part
                            })
                        }
                        _ => false,
                    });
                    if !fixed {
                        return Err(ToolError::new(
                            "part-unbacked",
                            format!("{} claims the composition {} fixed, but no staged update_requirement replaces that edge; pass the requirement's corrected edges, or set parent (verdict parented)", goal, line),
                        ));
                    }
                }
                Some("kept") => {
                    if !Self::reason_given(&e) {
                        return Err(ToolError::new(
                            "reason-required",
                            format!("{} keeps {} outside {} without a reason; say why the part is a generic rule-carrier (aggregation, several wholes), or set parent", goal, part, whole),
                        ));
                    }
                }
                other => {
                    return Err(ToolError::new(
                        "bad-verdict",
                        format!(
                            "part verdict `{}` for {}: parented, edge-fixed, or kept",
                            other.unwrap_or("?"),
                            part
                        ),
                    ));
                }
            }
        }
        // Unreferenced statements: one verdict per listed statement, referenced
        // backed by the staged update that adds the entity.
        let listed = crate::goals::unreferenced_statements(&self.snapshot, &target);
        for (rid, _) in &listed {
            if find("unreferenced", "id", rid).is_none() {
                return Err(ToolError::new(
                    "unreferenced-unjudged",
                    format!("{} has no unreferenced entry for {}, which names {} without referencing it; add {{id: {}, verdict: referenced | word-match, reason}}", goal, rid, target, rid),
                ));
            }
        }
        for e in evidence["unreferenced"].as_array().into_iter().flatten() {
            let rid = Self::evidence_id(e, "id");
            match e["verdict"].as_str() {
                Some("referenced") => {
                    if !self.requirement_names_entity(&rid, &target) {
                        return Err(ToolError::new(
                            "unreferenced-unbacked",
                            format!(
                                "{} claims {} referenced, but {} does not name {} after this session's staged work; update_requirement with id and the full entities list, or verdict word-match",
                                goal, rid, rid, target
                            ),
                        ));
                    }
                }
                Some("word-match") => {}
                other => {
                    return Err(ToolError::new(
                        "bad-verdict",
                        format!(
                            "unreferenced verdict `{}` for {}: referenced or word-match",
                            other.unwrap_or("?"),
                            rid
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    // The evidence of the kinds whose single verdict or per-node verdicts the page's
    // gate reads: each verdict named, each backed by the staged work. Mirrors the
    // `## Gate` of rejudge-pair, declare-edges, dedupe-candidates, curate-view,
    // conform-instance, and derive-criteria under docs/compiler/goals/.
    fn kind_evidence(&self, gs: &GoalScope, goal: &str, evidence: &Value) -> Result<(), ToolError> {
        let r = |x: &str| self.snapshot.resolve_id(x).to_string();
        match gs.kind.as_str() {
            "rejudge-pair" => {
                let Some((a, b)) = crate::goals::pair_members(&gs.target) else {
                    return Ok(());
                };
                let (a, b) = (r(a), r(b));
                let names_both = |d: &Diagnostic| {
                    d.subjects.iter().any(|s| r(s) == a) && d.subjects.iter().any(|s| r(s) == b)
                };
                let open = self.open_diagnostics();
                let pair_open =
                    |rule: &str| open.iter().any(|(_, d)| d.rule == rule && names_both(d));
                let deleted = |x: &str| {
                    self.staged
                        .iter()
                        .any(|o| matches!(o, Op::DeleteRequirement { id, .. } if r(id) == x))
                };
                // A finding naming a deleted side cannot stand as filed.
                let alive = |x: &str| {
                    let x = r(x);
                    self.snapshot.graph.requirements.contains_key(&x)
                        || self.snapshot.graph.entities.contains_key(&x)
                };
                if let Some((id, _)) = open.iter().find(|(_, d)| {
                    (d.rule == "contradiction" || d.rule == "duplicate-requirement")
                        && names_both(d)
                        && d.subjects.iter().any(|s| !alive(s))
                }) {
                    return Err(ToolError::new(
                        "dead-subject",
                        format!("{} names a deleted side of {}, so it cannot stand as filed; resolve_diagnostic it and, when the conflict still holds between surviving requirements, report a new one naming them", id, gs.target),
                    ));
                }
                match Self::evidence_verdict(evidence).as_deref() {
                    Some("duplicate") => {
                        if !deleted(&a) && !deleted(&b) && !pair_open("duplicate-requirement") {
                            return Err(ToolError::new(
                                "duplicate-unbacked",
                                format!("{} claims duplicate, but neither side is deleted in this session and no open duplicate-requirement names both; delete_requirement the worse-sourced side (one document), or report_diagnostic duplicate-requirement (two documents)", goal),
                            ));
                        }
                    }
                    Some("contradiction") => {
                        let carried = Self::evidence_id(evidence, "carried_by");
                        let named = open.iter().any(|(id, d)| {
                            *id == carried
                                && d.rule == "contradiction"
                                && d.subjects.iter().any(|s| r(s) == a || r(s) == b)
                        });
                        if !named && !pair_open("contradiction") {
                            return Err(ToolError::new(
                                "contradiction-unbacked",
                                format!("{} claims contradiction, but no open contradiction names both {} and {}; report_diagnostic rule contradiction on the pair, or name the standing one in carried_by", goal, a, b),
                            ));
                        }
                    }
                    Some("consistent") => {
                        if let Some((id, d)) = open.iter().find(|(_, d)| {
                            (d.rule == "contradiction" || d.rule == "duplicate-requirement")
                                && names_both(d)
                        }) {
                            return Err(ToolError::new(
                                "pair-diagnostic-open",
                                format!("{} claims consistent, but {} ({}) still stands on the pair; resolve_diagnostic it when its condition lapsed, or claim the verdict it carries", goal, id, d.rule),
                            ));
                        }
                    }
                    other => {
                        return Err(ToolError::new(
                            "bad-evidence",
                            format!("{} carries evidence {{verdict: duplicate | contradiction | consistent, carried_by?}}; received {}", goal, other.map(|v| format!("verdict `{}`", v)).unwrap_or_else(|| Self::evidence_shape(evidence))),
                        ));
                    }
                }
            }
            "declare-edges" => {
                let target = r(&gs.target);
                let staged_edges = self.staged.iter().any(|o| {
                    matches!(o, Op::UpdateRequirement { id, edges: Some(es), .. } if r(id) == target && !es.is_empty())
                });
                match Self::evidence_verdict(evidence).as_deref() {
                    Some("declared") => {
                        if !staged_edges {
                            return Err(ToolError::new(
                                "edges-unbacked",
                                format!("{} claims declared, but no update_requirement with edges on {} is staged; declare the typed edges, or claim not-structural with the sentence and why it relates no pair", goal, target),
                            ));
                        }
                    }
                    Some("not-structural") => {
                        if staged_edges {
                            return Err(ToolError::new(
                                "edges-staged",
                                format!("{} claims not-structural, but this session stages edges on {}; claim declared", goal, target),
                            ));
                        }
                    }
                    other => {
                        return Err(ToolError::new(
                            "bad-evidence",
                            format!("{} carries evidence \"declared\" or \"not-structural\"; received {}", goal, other.map(|v| format!("`{}`", v)).unwrap_or_else(|| Self::evidence_shape(evidence))),
                        ));
                    }
                }
            }
            "dedupe-candidates" => {
                let Some((a, b)) = crate::goals::pair_members(&gs.target) else {
                    return Ok(());
                };
                let merged = self.staged_merge_of(a, b);
                match Self::evidence_verdict(evidence).as_deref() {
                    Some("merged") => {
                        if !merged {
                            return Err(ToolError::new(
                                "merge-unbacked",
                                format!("{} claims merged, but no merge_entities naming {} and {} is staged; merge them with the reason, or claim separate", goal, a, b),
                            ));
                        }
                    }
                    Some("separate") => {
                        if merged {
                            return Err(ToolError::new(
                                "merge-staged",
                                format!("{} claims separate, but this session merges {} and {}; claim merged", goal, a, b),
                            ));
                        }
                    }
                    other => {
                        return Err(ToolError::new(
                            "bad-evidence",
                            format!(
                                "{} carries evidence \"merged\" or \"separate\"; received {}",
                                goal,
                                other
                                    .map(|v| format!("`{}`", v))
                                    .unwrap_or_else(|| Self::evidence_shape(evidence))
                            ),
                        ));
                    }
                }
            }
            "curate-view" => {
                let matched: Vec<(String, String)> = gs.change["matched"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| {
                        Some((
                            m["id"].as_str()?.to_string(),
                            m["why"].as_str().unwrap_or("").to_string(),
                        ))
                    })
                    .collect();
                if matched.is_empty() {
                    return Ok(());
                }
                let entries: Vec<Value> = match evidence {
                    Value::Array(a) => a.clone(),
                    Value::Object(m) => m
                        .values()
                        .find_map(|v| v.as_array().cloned())
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                let Some(view) = self.view_after(&gs.target) else {
                    return Ok(());
                };
                const PLACEHOLDERS: [&str; 5] = ["none", "n/a", "na", "-", "null"];
                for (id, why) in &matched {
                    let Some(e) = entries
                        .iter()
                        .find(|e| r(&Self::evidence_id(e, "id")) == r(id))
                    else {
                        return Err(ToolError::new(
                            "member-unjudged",
                            format!("{} has no verdict for {} ({}); add it with update_view add_members or exclude it with a note, then list {{id: {}, verdict: added | excluded}} in evidence, one entry per matched node; received {}", goal, id, why, id, Self::evidence_shape(evidence)),
                        ));
                    };
                    let excluded = view.excluded.iter().find(|x| r(&x.id) == r(id));
                    match e["verdict"].as_str() {
                        Some("added") => {
                            let listed = view.members.iter().any(|m| r(m) == r(id));
                            if !(listed
                                || (why == "query-match"
                                    && view.query.is_some()
                                    && excluded.is_none()))
                            {
                                return Err(ToolError::new(
                                    "added-unbacked",
                                    format!("{} claims {} added, but it is not a member of {} after this session's staged work; update_view add_members (at its position in a flow)", goal, id, gs.target),
                                ));
                            }
                        }
                        Some("excluded") => {
                            let Some(x) = excluded.filter(|x| {
                                let n = x.note.trim().to_lowercase();
                                !n.is_empty() && !PLACEHOLDERS.contains(&n.as_str())
                            }) else {
                                return Err(ToolError::new(
                                    "excluded-unbacked",
                                    format!("{} claims {} excluded, but {} does not exclude it with a note; update_view exclude [{{id: {}, note}}], the note naming the sentence or the rule", goal, id, gs.target, id),
                                ));
                            };
                            // A note sending the member to another flow names a view
                            // that lists it (curate-view.md#gate).
                            for other in Self::view_ids_in(&x.note) {
                                if other == gs.target {
                                    continue;
                                }
                                let Some(ov) = self.view_after(&other) else {
                                    continue;
                                };
                                if crate::derive::FLOW_KINDS.contains(&ov.kind.as_str())
                                    && !ov.members.iter().any(|m| r(m) == r(id))
                                {
                                    return Err(ToolError::new(
                                        "exclusion-points-nowhere",
                                        format!("the note excluding {} from {} names {}, which does not list it; add it there with update_view add_members first, or name the view that holds it", id, gs.target, other),
                                    ));
                                }
                            }
                        }
                        other => {
                            return Err(ToolError::new(
                                "bad-verdict",
                                format!(
                                    "curate verdict `{}` for {}: added or excluded",
                                    other.unwrap_or("?"),
                                    id
                                ),
                            ));
                        }
                    }
                }
            }
            "conform-instance" => {
                let inst = r(&gs.target);
                let Some(ty) = crate::derive::instance_types(&self.snapshot)
                    .get(&inst)
                    .cloned()
                else {
                    return Ok(());
                };
                let attrs = crate::goals::declared_attributes(&self.snapshot, &ty);
                let links = crate::goals::instance_links(&self.snapshot, &inst);
                let open = self.open_diagnostics();
                let filed = open.iter().any(|(_, d)| {
                    d.rule == "nonconformant-instance"
                        && d.subjects.iter().any(|s| r(s) == inst || r(s) == ty)
                });
                let repaired = self.staged.iter().any(|o| match o {
                    Op::UpdateEntity { id, .. } => r(id) == inst,
                    Op::UpdateRequirement { id, .. } => self
                        .snapshot
                        .graph
                        .requirements
                        .get(id)
                        .is_some_and(|q| q.entities.iter().any(|e| r(e) == inst)),
                    _ => false,
                });
                let shape = "{attributes: [{name, verdict: conforms | missing | nonconformant}], links: [{to, verdict, carried_by?}]}";
                let mut nonconformant = false;
                let mut check = |e: &Value, what: String| -> Result<(), ToolError> {
                    match e["verdict"].as_str() {
                        Some("conforms") | Some("missing") => Ok(()),
                        Some("nonconformant") => {
                            nonconformant = true;
                            if filed || repaired {
                                Ok(())
                            } else {
                                Err(ToolError::new(
                                    "nonconformant-unbacked",
                                    format!("{} claims {} nonconformant, but no nonconformant-instance diagnostic naming {} or {} is staged or open and no repair of the instance is staged; report_diagnostic rule nonconformant-instance, or conform the instance when the example sentence supports it", goal, what, inst, ty),
                                ))
                            }
                        }
                        other => Err(ToolError::new(
                            "bad-verdict",
                            format!("conformance verdict `{}` for {}: conforms, missing, or nonconformant", other.unwrap_or("?"), what),
                        )),
                    }
                };
                for name in &attrs {
                    let e = evidence["attributes"].as_array().and_then(|a| {
                        a.iter()
                            .find(|e| {
                                e["name"]
                                    .as_str()
                                    .is_some_and(|n| n.trim().to_lowercase() == *name)
                            })
                            .cloned()
                    });
                    let Some(e) = e else {
                        return Err(ToolError::new(
                            "conformance-unjudged",
                            format!("{} has no attributes entry for `{}`, which {} declares; compare the instance's value and add {{name: {}, verdict: conforms | missing | nonconformant}} to evidence {}", goal, name, ty, name, shape),
                        ));
                    };
                    check(&e, format!("attribute `{}`", name))?;
                }
                for (to, ty_) in &links {
                    let e = evidence["links"].as_array().and_then(|a| {
                        a.iter()
                            .find(|e| r(&Self::evidence_id(e, "to")) == r(to))
                            .cloned()
                    });
                    let Some(e) = e else {
                        return Err(ToolError::new(
                            "conformance-unjudged",
                            format!("{} has no links entry for {} ({}); compare it with the relationships of {} and add {{to: {}, verdict: conforms | missing | nonconformant}} to evidence {}", goal, to, ty_, ty, to, shape),
                        ));
                    };
                    check(&e, format!("the link to {}", to))?;
                }
                // A standing finding on the instance is backed by a verdict or resolved.
                if !nonconformant {
                    if let Some((id, _)) = self.snapshot.graph.diagnostics.iter().find(|(id, d)| {
                        d.lifecycle == "open"
                            && d.rule == "nonconformant-instance"
                            && d.subjects.iter().any(|s| r(s) == inst)
                            && !self.staged_resolves(id)
                    }) {
                        return Err(ToolError::new(
                            "nonconformant-open",
                            format!("{} stands open on {}, yet no verdict is nonconformant; resolve_diagnostic it when its condition lapsed, or claim the value or link it names nonconformant", id, inst),
                        ));
                    }
                }
            }
            "derive-criteria" => {
                let Some(list) = evidence["requirements"].as_array() else {
                    return Ok(());
                };
                for key in ["none", "revised"] {
                    for rid in gs.change[key]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|x| x.as_str())
                    {
                        if !list.iter().any(|e| Self::evidence_id(e, "id") == rid) {
                            return Err(ToolError::new(
                                "criteria-unjudged",
                                format!("{} has no entry for {}, a requirement the goal lists; write its criteria and add {{id: {}, criteria: [ids], verdict: complete | stated-only | open-question}}", goal, rid, rid),
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    // The view ids a note names (`view:usecase/returns`).
    fn view_ids_in(note: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = note;
        while let Some(pos) = rest.find("view:") {
            let tail = &rest[pos..];
            let end = tail
                .find(|c: char| !(c.is_alphanumeric() || matches!(c, ':' | '/' | '-' | '_' | '.')))
                .unwrap_or(tail.len());
            let id = tail[..end].trim_end_matches('.').to_string();
            if id.len() > "view:".len() && !out.contains(&id) {
                out.push(id);
            }
            rest = &tail[end.max(5)..];
        }
        out
    }

    fn lint_evidence(&self, goal: &str, target: &str, evidence: &Value) -> Result<(), ToolError> {
        let rules = self.lint.warnings.len() + self.lint.errors.len();
        if rules == 0 {
            return Ok(());
        }
        let entries: Vec<&Value> = evidence["lint"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        let target = self.snapshot.resolve_id(target).to_string();
        let mut subjects: BTreeSet<String> = self
            .snapshot
            .graph
            .requirements
            .iter()
            .filter(|(_, r)| {
                r.entities
                    .iter()
                    .any(|e| self.snapshot.resolve_id(e) == target)
            })
            .map(|(id, _)| id.clone())
            .collect();
        subjects.insert(target.clone());
        let on_entity = |subs: &[String]| {
            subs.iter()
                .any(|s| subjects.contains(self.snapshot.resolve_id(s)))
        };
        let filed = self.staged.iter().any(|o| {
            matches!(o, Op::ReportDiagnostic { diagnostic, .. }
                if diagnostic.rule == "lint" && on_entity(&diagnostic.subjects))
        }) || self
            .snapshot
            .graph
            .diagnostics
            .values()
            .any(|d| d.lifecycle == "open" && d.rule == "lint" && on_entity(&d.subjects));
        for n in 1..=rules {
            let Some(e) = entries.iter().find(|e| {
                e["rule"].as_u64() == Some(n as u64)
                    || e["rule"].as_str() == Some(n.to_string().as_str())
            }) else {
                return Err(ToolError::new(
                    "lint-unchecked",
                    format!(
                        "{} has no lint entry for rule [{}] on the ## Project lint line; read every quote on {} against it and add {{rule: {}, verdict: clean | filed}} to evidence.lint",
                        goal, n, target, n
                    ),
                ));
            };
            match e["verdict"].as_str() {
                Some("clean") => {}
                Some("filed") => {
                    if !filed {
                        return Err(ToolError::new(
                            "lint-unbacked",
                            format!(
                                "{} claims rule [{}] filed, but no lint diagnostic is staged or open on {} or its statements; report_diagnostic rule lint naming the requirement whose quote violates it, or verdict clean",
                                goal, n, target
                            ),
                        ));
                    }
                }
                other => {
                    return Err(ToolError::new(
                        "bad-verdict",
                        format!(
                            "lint verdict `{}` for rule [{}]: clean or filed",
                            other.unwrap_or("?"),
                            n
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    // The evidence checks of one claim, read-only over the store plus the staged work:
    // mark_goal_done runs them before anything stages, and done runs them again over
    // the final changeset. Mirrors docs/compiler/tools.md#goal-tools.
    fn claim_evidence(
        &self,
        gs: &GoalScope,
        goal: &str,
        evidence: &Value,
    ) -> Result<(), ToolError> {
        if gs.kind == "review-entity" {
            self.review_evidence(goal, &gs.target, evidence)?;
            // One lint entry per declared rule, filed backed by a lint finding on the
            // entity or its statements. Mirrors docs/compiler/goals/review-entity.md#gate.
            self.lint_evidence(goal, &gs.target, evidence)?;
        }
        self.kind_evidence(gs, goal, evidence)?;
        if gs.kind == "derive-criteria" {
            self.criteria_evidence(goal, evidence)?;
        }
        Ok(())
    }

    // Criterion ids and freshness once the staged work applies, in mutation order.
    fn criteria_after(&self, rid: &str) -> BTreeMap<String, bool> {
        let rid = self.snapshot.resolve_id(rid);
        let mut criteria: BTreeMap<String, bool> = self
            .snapshot
            .graph
            .criteria
            .iter()
            .filter(|(_, c)| c.requirement == rid)
            .map(|(id, c)| (id.clone(), c.stale))
            .collect();
        for op in &self.staged {
            match op {
                Op::CreateCriterion { id, criterion }
                    if self.snapshot.resolve_id(&criterion.requirement) == rid =>
                {
                    criteria.insert(id.clone(), criterion.stale);
                }
                Op::UpdateCriterion { id, .. } => {
                    if let Some(stale) = criteria.get_mut(id) {
                        *stale = false;
                    }
                }
                Op::DeleteCriterion { id, .. } => {
                    criteria.remove(id);
                }
                _ => {}
            }
        }
        criteria
    }

    // derive-criteria: one entry per requirement, complete backed by a criterion,
    // open-question backed by an invented-choice prompt, both read after the staged
    // work applies (docs/compiler/goals/derive-criteria.md#gate).
    fn criteria_evidence(&self, goal: &str, evidence: &Value) -> Result<(), ToolError> {
        let r = |x: &str| self.snapshot.resolve_id(x).to_string();
        {
            let Some(list) = evidence["requirements"].as_array() else {
                return Err(ToolError::new(
                    "bad-evidence",
                    format!(
                        "{} carries evidence {{requirements: [{{id, criteria: [ids], verdict: complete | stated-only | open-question}}]}}, one entry per requirement the goal listed; received {}",
                        goal,
                        match &evidence {
                            Value::Null => "no evidence".to_string(),
                            Value::String(_) => "a string that is not JSON".to_string(),
                            Value::Array(_) => "a bare array (wrap it: {requirements: [...]})".to_string(),
                            Value::Object(m) => format!("an object with keys {}", m.keys().cloned().collect::<Vec<_>>().join(", ")),
                            other => other.to_string(),
                        }
                    ),
                ));
            };
            for e in list {
                let rid = e["id"].as_str().unwrap_or("").to_string();
                let criteria = self.criteria_after(&rid);
                let stale: Vec<&str> = criteria
                    .iter()
                    .filter(|(_, stale)| **stale)
                    .map(|(id, _)| id.as_str())
                    .collect();
                if !stale.is_empty() {
                    return Err(ToolError::new(
                        "criterion-stale",
                        format!("{} claims {} resolved, but these criteria still need review against the revised requirement: {}. Refresh each still-valid criterion with update_criterion using its id and current or corrected statement, even if the wording is unchanged; replace or delete obsolete criteria before marking the goal done.", goal, rid, stale.join(", ")),
                    ));
                }
                let has_criterion = !criteria.is_empty();
                match e["verdict"].as_str() {
                    Some("complete") | Some("stated-only") => {
                        if !has_criterion {
                            return Err(ToolError::new(
                                "criterion-missing",
                                format!(
                                    "{} claims {} {}, but no criterion is recorded or staged on it; upsert_criterion, or verdict open-question with an invented-choice prompt",
                                    goal, rid, e["verdict"].as_str().unwrap_or("")
                                ),
                            ));
                        }
                    }
                    Some("open-question") => {
                        // Any open question naming the requirement backs the verdict:
                        // a standing contradiction with its prompt is the question
                        // (docs/compiler/goals/derive-criteria.md#gate).
                        let asks = crate::goals::asks_for_answer;
                        let names = |d: &Diagnostic| {
                            asks(&d.rule, &d.prompt) && d.subjects.iter().any(|s| r(s) == r(&rid))
                        };
                        let prompted = self.staged.iter().any(|o| matches!(o, Op::ReportDiagnostic { diagnostic, .. } if names(diagnostic)))
                            || self.open_diagnostics().iter().any(|(_, d)| names(d));
                        if !prompted {
                            return Err(ToolError::new(
                                "unbacked-question",
                                format!(
                                    "{} claims {} open-question, but no open diagnostic with a prompt names it; report_diagnostic rule invented-choice with a prompt carrying the options, unless a standing contradiction, decision, or ambiguity already asks",
                                    goal, rid
                                ),
                            ));
                        }
                    }
                    other => {
                        return Err(ToolError::new(
                            "bad-verdict",
                            format!(
                                "verdict `{}` for {}: complete, stated-only, or open-question",
                                other.unwrap_or("?"),
                                rid
                            ),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn mark_goal_done(&mut self, args: &Value) -> Result<Value, ToolError> {
        let goal = Self::str_arg(args, "goal")?;
        let Some(gs) = self.scope.goal(&goal).cloned() else {
            return Err(ToolError::new(
                "unknown-goal",
                format!(
                    "`{}` is not a goal of this batch ({}); only goals in the batch can be marked",
                    goal,
                    self.scope.goal_ids().join(", ")
                ),
            ));
        };
        let justification = Self::str_arg(args, "justification")?;
        let sentences = Self::sentence_count(&justification);
        if sentences > 2 {
            return Err(ToolError::new(
                "bad-justification",
                format!(
                    "the justification for {} is {} sentences; one or two saying why the gate holds, never an essay",
                    goal, sentences
                ),
            ));
        }
        self.goal_gate(&gs)?;
        // A text codec hands evidence as a JSON string: parse it, so the gates read the
        // object the session meant (docs/compiler/tools.md#goal-tools).
        let evidence: Value = match &args["evidence"] {
            Value::String(s) => {
                serde_json::from_str::<Value>(s).unwrap_or(Value::String(s.clone()))
            }
            other => other.clone(),
        };
        // The review's unreferenced verdicts: referenced is backed by the staged
        // update, word-match is remembered on the entity so the hint never returns.
        // Mirrors docs/compiler/goals/review-entity.md#gate.
        // Every check reads before anything stages, so a refused claim leaves the
        // changeset as it was.
        self.claim_evidence(&gs, &goal, &evidence)?;
        if gs.kind == "review-entity" {
            if let Some(list) = evidence["unreferenced"].as_array() {
                let target = self.snapshot.resolve_id(&gs.target).to_string();
                // A repeated claim stages only the word matches no earlier claim staged.
                let staged_already: BTreeSet<String> = self
                    .staged
                    .iter()
                    .filter_map(|o| match o {
                        Op::UpdateEntity {
                            id, not_about_add, ..
                        } if *id == target => Some(not_about_add.clone()),
                        _ => None,
                    })
                    .flatten()
                    .collect();
                let mut word_matches: Vec<String> = Vec::new();
                for id in list
                    .iter()
                    .filter(|e| e["verdict"].as_str() == Some("word-match"))
                    .map(|e| Self::evidence_id(e, "id"))
                {
                    if !staged_already.contains(&id) && !word_matches.contains(&id) {
                        word_matches.push(id);
                    }
                }
                if !word_matches.is_empty() {
                    self.review_staged_note = Some(format!(
                        "the word-match verdicts stage one update on {} (its not_about list); done counts it as applied",
                        target
                    ));
                    self.stage(Op::UpdateEntity {
                        id: target,
                        name: None,
                        definition: None,
                        add_aliases: Vec::new(),
                        remove_aliases: Vec::new(),
                        add_mention: None,
                        stereotype: None,
                        parent: None,
                        clear_parent: false,
                        not_about_add: word_matches,
                        set_attributes: None,
                        add_attributes: Vec::new(),
                        provenance: None,
                    })?;
                }
            }
        }
        self.outcomes.insert(
            goal.clone(),
            GoalOutcome::Done {
                justification,
                evidence,
            },
        );
        let open: Vec<&str> = self
            .scope
            .goals
            .iter()
            .filter(|g| !self.outcomes.contains_key(&g.goal))
            .map(|g| g.goal.as_str())
            .collect();
        let mut reply = json!({"marked": goal, "open": open});
        if let Some(n) = self.review_staged_note.take() {
            reply["note"] = json!(n);
        }
        Ok(reply)
    }

    fn done_with_goals(&mut self, args: &Value) -> Result<Value, ToolError> {
        let entries = args["goals"].as_array().ok_or_else(|| {
            ToolError::new(
                "bad-args",
                "done.goals takes a list of final {goal, justification, evidence?} claims".into(),
            )
        })?;
        if entries.len() > self.scope.goals.len() {
            return Err(ToolError::new(
                "bad-args",
                "done.goals cannot contain more claims than this batch has goals".into(),
            ));
        }
        let mut named = BTreeSet::new();
        for entry in entries {
            let goal = Self::str_arg(entry, "goal")?;
            if !named.insert(goal.clone()) {
                return Err(ToolError::new(
                    "duplicate-goal",
                    format!("done.goals names `{goal}` more than once"),
                ));
            }
        }
        // mark_goal_done can append a review mutation and replace an outcome.
        // Like settle_one, roll back this call's effects when a later claim or
        // batch gate refuses. The snapshot and earlier staged work stay intact.
        let staged_before = self.staged.len();
        let outcomes_before = self.outcomes.clone();
        let note_before = self.review_staged_note.clone();
        let done_before = self.done.clone();
        let result = (|| {
            for entry in entries {
                self.mark_goal_done(entry)?;
            }
            let mut finish = args.clone();
            finish.as_object_mut().unwrap().remove("goals");
            self.dispatch_inner("done", &finish)
        })();
        if result.is_err() {
            self.staged.truncate(staged_before);
            self.outcomes = outcomes_before;
            self.review_staged_note = note_before;
            self.done = done_before;
        }
        result
    }

    // The batch claim of a pair session: one entry per rejudge-pair goal, each doing
    // what the single calls it stands for would do (stage the diagnostic the verdict
    // carries or resolve the pair's open one, then mark the goal done) under the
    // same gates. A refused entry is reported in place, leaves nothing staged, and
    // stops none of the others. Mirrors docs/compiler/tools.md#goal-tools.
    pub(super) fn settle_pairs(&mut self, args: &Value) -> Result<Value, ToolError> {
        let Some(entries) = args["verdicts"].as_array().filter(|a| !a.is_empty()) else {
            return Err(ToolError::new(
                "bad-args",
                "verdicts takes a non-empty list of {goal, verdict, justification, ...} entries, one per rejudge-pair goal of the batch".into(),
            ));
        };
        let mut results = Vec::new();
        let mut settled = Vec::new();
        for entry in entries {
            let goal = entry["goal"].as_str().unwrap_or_default().to_string();
            // The guard counts accepted entries: a verbatim re-send of an accepted
            // entry is marked on the second and refused on the third, while a
            // rejected entry can be repaired and re-sent.
            // Mirrors docs/compiler/sessions.md#repeated-calls.
            let key = format!("{}|{}", PAIR_SETTLE_TOOL, entry);
            if self.repeats.get(&key).copied().unwrap_or(0) >= 2 {
                results.push(json!({"goal": goal, "error": {
                    "rule": "repeated-call",
                    "message": format!("this entry for `{}` was already accepted twice with identical arguments; its verdict stands, act on it", goal),
                }}));
                continue;
            }
            match self.settle_one(entry) {
                Ok(mut r) => {
                    let seen = {
                        let c = self.repeats.entry(key.clone()).or_insert(0);
                        *c += 1;
                        *c
                    };
                    if seen == 2 {
                        r["repeat"] = json!("this entry was already accepted with identical arguments; its verdict stands");
                    }
                    settled.push(goal);
                    results.push(r);
                }
                Err(e) => results
                    .push(json!({"goal": goal, "error": {"rule": e.rule, "message": e.message}})),
            }
        }
        let open: Vec<&str> = self
            .scope
            .goals
            .iter()
            .filter(|g| !self.outcomes.contains_key(&g.goal))
            .map(|g| g.goal.as_str())
            .collect();
        Ok(json!({"results": results, "settled": settled, "open": open}))
    }

    // One settle_pairs entry. Everything that can be refused without staging is
    // checked first; a refusal after staging rolls the entry's own work back, so a
    // reported error never leaves a half-landed verdict.
    fn settle_one(&mut self, entry: &Value) -> Result<Value, ToolError> {
        let goal = Self::str_arg(entry, "goal")?;
        let Some(gs) = self.scope.goal(&goal).cloned() else {
            return Err(ToolError::new(
                "unknown-goal",
                format!(
                    "`{}` is not a goal of this batch ({}); only goals in the batch can be settled",
                    goal,
                    self.scope.goal_ids().join(", ")
                ),
            ));
        };
        if gs.kind != "rejudge-pair" {
            return Err(ToolError::new(
                "wrong-kind",
                format!(
                    "`{}` is a {} goal; settle_pairs takes rejudge-pair goals only, mark the others with mark_goal_done",
                    goal, gs.kind
                ),
            ));
        }
        let Some((a, b)) = crate::goals::pair_members(&gs.target) else {
            return Err(ToolError::new(
                "bad-target",
                format!("`{}` names no pair", goal),
            ));
        };
        let (a, b) = (a.to_string(), b.to_string());
        let verdict = Self::str_arg(entry, "verdict")?;
        if !PAIR_VERDICTS.contains(&verdict.as_str()) {
            return Err(ToolError::new(
                "bad-verdict",
                format!(
                    "verdict `{}` for {} must be one of: {}",
                    verdict,
                    goal,
                    PAIR_VERDICTS.join(", ")
                ),
            ));
        }
        let justification = Self::str_arg(entry, "justification")?;
        let sentences = Self::sentence_count(&justification);
        if sentences > 2 {
            return Err(ToolError::new(
                "bad-justification",
                format!(
                    "the justification for {} is {} sentences; one or two saying why the verdict holds, never an essay",
                    goal, sentences
                ),
            ));
        }
        // The open pair diagnostics: what a resolved verdict settles, and the
        // contradiction a contradiction verdict carries instead of filing a twin.
        // Recorded or staged this session, not already resolved in the changeset.
        let names_pair = |d: &Diagnostic| {
            d.lifecycle == "open"
                && (d.rule == "contradiction" || d.rule == "duplicate-requirement")
                && d.subjects.contains(&a)
                && d.subjects.contains(&b)
        };
        let resolved_here: BTreeSet<&str> = self
            .staged
            .iter()
            .filter_map(|o| match o {
                Op::ResolveDiagnostic { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let mut pair_diags: Vec<String> = self
            .snapshot
            .graph
            .diagnostics
            .iter()
            .chain(self.staged_diags.iter())
            .filter(|(id, d)| names_pair(d) && !resolved_here.contains(id.as_str()))
            .map(|(id, _)| id.clone())
            .collect();
        pair_diags.sort();
        pair_diags.dedup();
        if verdict == "resolved" && pair_diags.is_empty() {
            return Err(ToolError::new(
                "no-pair-diagnostic",
                format!(
                    "no open contradiction or duplicate-requirement diagnostic names both {} and {}; a pair with nothing to resolve takes verdict consistent",
                    a, b
                ),
            ));
        }
        // A contradiction already filed on this pair (by an earlier batch, or by
        // this session against an unpaired statement) carries the verdict; a second
        // filing would be a twin. Mirrors docs/compiler/goals/rejudge-pair.md#tools.
        let standing_contradiction: Option<String> = pair_diags
            .iter()
            .find(|id| {
                self.staged_diags
                    .get(*id)
                    .or_else(|| self.snapshot.graph.diagnostics.get(*id))
                    .is_some_and(|d| d.rule == "contradiction")
            })
            .cloned();
        // The arguments a filing verdict needs, checked before anything stages; a
        // contradiction already standing on the pair, or one the entry names as
        // carried_by, needs none.
        let carried = Self::opt_str(entry, "carried_by").is_some_and(|cid| {
            self.staged_diags
                .get(&cid)
                .or_else(|| self.snapshot.graph.diagnostics.get(&cid))
                .is_some_and(|d| {
                    d.lifecycle == "open"
                        && d.rule == "contradiction"
                        && d.subjects.iter().any(|s| *s == a || *s == b)
                })
        });
        if verdict == "contradiction" && standing_contradiction.is_none() && !carried {
            match Self::opt_str(entry, "severity").as_deref() {
                None => {
                    return Err(ToolError::new(
                        "bad-args",
                        format!("a contradiction entry for {} carries severity (error when no reading lets both hold, warning otherwise)", goal),
                    ));
                }
                Some("error") | Some("warning") => {}
                Some(other) => {
                    return Err(ToolError::new(
                        "bad-severity",
                        format!("severity `{}` for {}: a contradiction is error when no reading lets both hold, warning otherwise", other, goal),
                    ));
                }
            }
            if Self::opt_str(entry, "message").is_none() {
                return Err(ToolError::new(
                    "bad-args",
                    format!("a contradiction entry for {} carries message quoting the conflicting claims", goal),
                ));
            }
        }
        // Stage what the verdict carries, rolling the entry's own work back when
        // the claim behind it is refused.
        let staged_before = self.staged.len();
        let diags_before = self.staged_diags.clone();
        let outcome = (|| -> Result<Value, ToolError> {
            let mut result = json!({"goal": goal, "verdict": verdict});
            let evidence = match verdict.as_str() {
                "consistent" => json!({"verdict": "consistent"}),
                "duplicate" => {
                    // Two quotes of one document are the same-document case: the
                    // worse-sourced side is deleted, never kept beside a finding.
                    // Mirrors docs/compiler/goals/rejudge-pair.md#tools.
                    let doc_of = |id: &str| {
                        self.snapshot
                            .graph
                            .requirements
                            .get(id)
                            .and_then(|r| r.source.as_ref())
                            .map(|s| s.doc.clone())
                    };
                    if let (Some(da), Some(db)) = (doc_of(&a), doc_of(&b)) {
                        if da == db {
                            return Err(ToolError::new(
                                "same-document-duplicate",
                                format!(
                                    "{} and {} both quote {}: a same-document duplicate is settled by delete_requirement on the worse-sourced side plus mark_goal_done with evidence {{verdict: duplicate, carried_by: <deleted id>}}, never by a duplicate-requirement finding that keeps both",
                                    a, b, da
                                ),
                            ));
                        }
                    }
                    let filed = self.dispatch_inner(
                        "report_diagnostic",
                        &json!({
                            "rule": "duplicate-requirement",
                            "severity": Self::opt_str(entry, "severity").unwrap_or_else(|| "info".into()),
                            "subjects": [a, b],
                            "message": Self::opt_str(entry, "message").unwrap_or_else(|| format!("{} and {} state the same obligation from different documents; both are kept.", a, b)),
                            "reasoning": Self::opt_str(entry, "reasoning").unwrap_or_else(|| justification.clone()),
                        }),
                    )?;
                    result["diagnostic"] = filed["id"].clone();
                    json!({"verdict": "duplicate", "carried_by": filed["id"]})
                }
                "contradiction" => {
                    // A standing contradiction the entry names carries the verdict even
                    // when it names one side only: the same threshold conflict across
                    // several pairs is one finding (rejudge-pair.md#tools).
                    let named = Self::opt_str(entry, "carried_by").filter(|cid| {
                        self.staged_diags
                            .get(cid)
                            .or_else(|| self.snapshot.graph.diagnostics.get(cid))
                            .is_some_and(|d| {
                                d.lifecycle == "open"
                                    && d.rule == "contradiction"
                                    && d.subjects.iter().any(|s| *s == a || *s == b)
                            })
                    });
                    if let Some(id) = named.as_ref().or(standing_contradiction.as_ref()) {
                        result["diagnostic"] = json!(id);
                        result["note"] = json!(
                            "the contradiction already stood on the pair; no second one filed"
                        );
                        json!({"verdict": "contradiction", "carried_by": id})
                    } else {
                        let mut call = json!({
                            "rule": "contradiction",
                            "severity": entry["severity"],
                            "subjects": [a, b],
                            "message": entry["message"],
                            "reasoning": Self::opt_str(entry, "reasoning").unwrap_or_else(|| justification.clone()),
                        });
                        if !entry["prompt"].is_null() {
                            call["prompt"] = entry["prompt"].clone();
                        }
                        let filed = self.dispatch_inner("report_diagnostic", &call)?;
                        result["diagnostic"] = filed["id"].clone();
                        json!({"verdict": "contradiction", "carried_by": filed["id"]})
                    }
                }
                _ => {
                    for id in &pair_diags {
                        self.dispatch_inner(
                            "resolve_diagnostic",
                            &json!({"id": id, "reason": justification}),
                        )?;
                    }
                    result["resolved"] = json!(pair_diags);
                    json!({"verdict": "consistent", "resolved": pair_diags})
                }
            };
            self.mark_goal_done(&json!({
                "goal": goal,
                "justification": justification,
                "evidence": evidence,
            }))?;
            Ok(result)
        })();
        if outcome.is_err() {
            self.staged.truncate(staged_before);
            self.staged_diags = diags_before;
        }
        outcome
    }

    pub(super) fn load_skill_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let name_arg = Self::str_arg(args, "name")?;
        match self.skills.activate(&name_arg) {
            Err(e) => Err(ToolError::new("unknown-skill", e)),
            Ok(SkillLoad::CapReached) => Err(ToolError::new(
                "skill-cap",
                format!(
                    "at most {} skills render in one session; already rendered: {}",
                    self.skills.cap,
                    self.skills.rendered_names().join(", ")
                ),
            )),
            Ok(SkillLoad::Rendered(payload)) => Ok(json!({"skill": name_arg, "payload": payload})),
            Ok(_) => Ok(json!({
                "skill": name_arg,
                "note": "already rendered this session; its text stands in the conversation"
            })),
        }
    }

    pub(super) fn mark_goal_failed_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let goal = Self::str_arg(args, "goal")?;
        if self.scope.goal(&goal).is_none() {
            return Err(ToolError::new(
                "unknown-goal",
                format!(
                    "`{}` is not a goal of this batch ({})",
                    goal,
                    self.scope.goal_ids().join(", ")
                ),
            ));
        }
        let reason = Self::str_arg(args, "reason")?;
        // A failed mandatory goal blocks convergence, so the reason has to
        // say what blocked it: a placeholder once landed by mistake.
        // Mirrors docs/compiler/tools.md#goal-tools.
        let lower = reason.trim().to_lowercase();
        if lower.len() < 12
            || ["placeholder", "todo", "tbd", "n/a", "none"].contains(&lower.as_str())
        {
            return Err(ToolError::new(
                "bad-reason",
                format!(
                    "the reason for failing {} is `{}`; say in a sentence what blocked the goal (a failed mandatory goal blocks convergence), or mark it done with its evidence",
                    goal, reason
                ),
            ));
        }
        self.outcomes
            .insert(goal.clone(), GoalOutcome::Failed { reason });
        Ok(
            json!({"failed": goal, "note": "recorded; a failed mandatory goal blocks convergence and stays visible on its target"}),
        )
    }

    pub(super) fn done_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        if args.get("goals").is_some() {
            return self.done_with_goals(args);
        }
        // A done that leaves a mandatory goal neither done nor failed is
        // rejected naming it; the implicit path commits around it and the
        // goal stays open for its retry. Mirrors docs/compiler/sessions.md#commit.
        if !self.implicit_done {
            let open: Vec<&str> = self
                .scope
                .goals
                .iter()
                .filter(|g| g.mandatory && !self.outcomes.contains_key(&g.goal))
                .map(|g| g.goal.as_str())
                .collect();
            if !open.is_empty() {
                return Err(ToolError::new(
                    "open-goal",
                    format!(
                        "goal(s) neither done nor failed: {}; mark_goal_done or mark_goal_failed each, then done",
                        open.join(", ")
                    ),
                ));
            }
        }
        // Batch gate: every proposal of the batch is decided.
        let undecided = self.undecided_proposals(&self.scope.proposals());
        if !undecided.is_empty() {
            return Err(ToolError::new(
                "undecided-proposal",
                format!(
                    "proposals left undecided: {}; for each, place_anchor with the section that now holds the statement (reevaluate true when its meaning may have changed), or orphan_anchor when no candidate states it",
                    undecided.join(", ")
                ),
            ));
        }
        // Batch gate: stale anchors are a contract the harness never commits
        // around, implicit path included.
        let untouched = self.untouched_stale(&self.scope.stale_anchors());
        if !untouched.is_empty() {
            return Err(ToolError::new(
                "stale-anchor",
                format!(
                    "stale anchors left untouched: {}; for each: if the document still states the fact, re-record it with upsert_requirement quoting the new sentence verbatim (it resolves to the anchor and updates in place); if the statement changed meaning, revise it with update_requirement carrying the id, the new statement, and the new section plus quote; if the fact is gone, delete_requirement",
                    untouched.join(", ")
                ),
            ));
        }
        // Batch gate: an explicit done finishes the coverage contract. Every
        // section of the batch carries a mark, staged or already recorded; the
        // implicit path commits without one and the section stays unprocessed.
        if !self.implicit_done {
            for gs in self.scope.reconcile_scopes() {
                if matches!(
                    self.outcomes.get(&gs.goal),
                    Some(GoalOutcome::Failed { .. })
                ) {
                    continue;
                }
                if let Some(doc) = &gs.doc {
                    let unmarked = self.unmarked_sections(doc, &gs.sections);
                    if !unmarked.is_empty() {
                        return Err(ToolError::new(
                            "unmarked-section",
                            format!(
                                "dirty section(s) without a coverage mark: {}; for each, set_coverage covered (a requirement must be sourced from it) or non-normative with a note",
                                unmarked.join(", ")
                            ),
                        ));
                    }
                }
            }
        }
        // Batch gate: every covered claim is honest, implicit path included
        // (finish_implicit drops the offending marks and retries).
        if let Some(e) = self.dishonest_covered() {
            return Err(e);
        }
        // Every resolution's gate and evidence still hold over the final
        // changeset: work staged after a claim cannot undo what backs it.
        let scopes: Vec<GoalScope> = self.scope.goals.clone();
        for gs in &scopes {
            if let Some(GoalOutcome::Done { evidence, .. }) = self.outcomes.get(&gs.goal) {
                let evidence = evidence.clone();
                self.goal_gate(gs)?;
                self.claim_evidence(gs, &gs.goal, &evidence)?;
            }
        }
        // The union of the batch's kinds' gates over the whole changeset.
        for kind in self.scope.kinds() {
            if let Some(k) = crate::goals::kind(&kind) {
                if let Some(v) = k.gates(&self.snapshot, &self.staged).into_iter().next() {
                    return Err(ToolError::new(&v.rule, v.message));
                }
            }
        }
        let summary = Self::opt_str(args, "summary").unwrap_or_default();
        self.done = Some(summary);
        Ok(json!({"ok": true}))
    }
}
