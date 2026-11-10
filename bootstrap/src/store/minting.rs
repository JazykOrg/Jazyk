// Identity: id resolution, minting, natural keys, and the containment lookups.
// Mirrors docs/compiler/concepts/identity.md.
use super::*;

impl Store {
    // Follow merge redirects to the surviving id. A tombstone (empty target) stays dead.
    // A chain a store written before compaction still holds is followed to its end;
    // the hop bound only stops a cycle.
    pub fn resolve_id<'a>(&'a self, id: &'a str) -> &'a str {
        let mut cur = id;
        let mut hops = 0;
        while let Some(next) = self.graph.redirects.get(cur) {
            if next.is_empty() || hops >= self.graph.redirects.len() {
                return cur;
            }
            cur = next;
            hops += 1;
        }
        cur
    }

    // ---- id minting ----

    // Ids carry no counter: a slug of the natural key and, where the kind needs one or
    // the plain id is taken, a short hash of the key, so two branches never mint one id
    // for two nodes. Mirrors docs/compiler/model.md#identifiers.
    fn id_taken(&self, id: &str, taken: &BTreeSet<String>) -> bool {
        self.graph.entities.contains_key(id)
            || self.graph.requirements.contains_key(id)
            || self.graph.criteria.contains_key(id)
            || self.graph.views.contains_key(id)
            || self.graph.diagnostics.contains_key(id)
            || self.graph.redirects.contains_key(id)
            || taken.contains(id)
    }

    pub fn mint_entity_id(&self, name: &str, taken: &BTreeSet<String>) -> String {
        let plain = format!("ent:{}", crate::ids::slug(name));
        crate::ids::plain_or_unique(&plain, name, |id| self.id_taken(id, taken))
    }

    // The stem of a requirement id: the id slug of its document's file stem and the
    // last segment of its section reference (dropped for a root or preamble section),
    // `x` without a document. Mirrors docs/compiler/model.md#identifiers.
    pub fn req_id_stem(source: Option<&crate::model::SourceRef>) -> String {
        let Some(src) = source else {
            return "x".to_string();
        };
        let file = src.doc.rsplit('/').next().unwrap_or(&src.doc);
        let file = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
        let segments: Vec<&str> = src.section.split('/').filter(|s| !s.is_empty()).collect();
        let text = if segments.len() >= 2 {
            format!("{} {}", file, segments[segments.len() - 1])
        } else {
            file.to_string()
        };
        crate::ids::slug(&text)
    }

    // `req:<stem>-<hash>`, the hash of the document, section, and statement, or of the
    // sorted `from` ids and the statement for a derived or decreed requirement.
    pub fn mint_req_id(&self, r: &Requirement, taken: &BTreeSet<String>) -> String {
        let statement = normalize_statement(&r.statement);
        let key = match &r.source {
            Some(src) => format!("{}#{}\n{}", src.doc, src.section, statement),
            None => {
                let mut from: Vec<String> = match &r.provenance {
                    Some(Provenance::Derived { from, .. }) => from.clone(),
                    _ => Vec::new(),
                };
                from.sort();
                format!("x\n{}\n{}", from.join(","), statement)
            }
        };
        let base = format!("req:{}", Self::req_id_stem(r.source.as_ref()));
        crate::ids::unique(&base, &key, |id| self.id_taken(id, taken))
    }

    // The id stem of a requirement: its source document, or `x` without a quote.
    pub fn req_stem(r: &Requirement) -> &str {
        r.source.as_ref().map(|s| s.doc.as_str()).unwrap_or("x")
    }

    // `crit:<stem>-<hash>`: the stem of its requirement (else of its own quote, else
    // `x`), the hash of the requirement id and the statement.
    // Mirrors docs/compiler/model/criterion.md#identity.
    // `req_source` is the source of a requirement staged beside the criterion, not yet
    // in the graph.
    pub fn mint_crit_id(
        &self,
        c: &Criterion,
        req_source: Option<&crate::model::SourceRef>,
        taken: &BTreeSet<String>,
    ) -> String {
        let source = self
            .graph
            .requirements
            .get(&c.requirement)
            .and_then(|r| r.source.as_ref())
            .or(req_source)
            .or(c.source.as_ref());
        let key = format!("{}\n{}", c.requirement, normalize_statement(&c.statement));
        let base = format!("crit:{}", Self::req_id_stem(source));
        crate::ids::unique(&base, &key, |id| self.id_taken(id, taken))
    }

    // The id stem of a criterion: its quote's document, else its requirement's, so a
    // derived criterion reads as its requirement's kin (crit:slide-about-3), and `x`
    // only when neither has a document (docs/compiler/model/criterion.md#identity).
    pub fn crit_stem(&self, c: &Criterion) -> String {
        self.graph
            .requirements
            .get(&c.requirement)
            .and_then(|r| r.source.as_ref())
            .map(|s| s.doc.clone())
            .or_else(|| c.source.as_ref().map(|s| s.doc.clone()))
            .unwrap_or_else(|| "x".to_string())
    }

    // The criteria of one requirement, in id order (docs/compiler/model/criterion.md).
    pub fn criteria_of(&self, rid: &str) -> Vec<String> {
        let rid = self.resolve_id(rid);
        self.graph
            .criteria
            .iter()
            .filter(|(_, c)| c.requirement == rid)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn mint_view_id(&self, kind: &str, title: &str, taken: &BTreeSet<String>) -> String {
        let plain = format!("view:{}/{}", view_kind_slug(kind), crate::ids::slug(title));
        let key = format!("{}\n{}", kind, title);
        crate::ids::plain_or_unique(&plain, &key, |id| self.id_taken(id, taken))
    }

    // `diag:<rule>-<hash>`, the hash of the rule and the sorted subjects.
    pub fn mint_diag_id(
        &self,
        rule: &str,
        subjects: &[String],
        taken: &BTreeSet<String>,
    ) -> String {
        let mut sorted: Vec<&str> = subjects.iter().map(String::as_str).collect();
        sorted.sort();
        let key = format!("{}\n{}", rule, sorted.join("\n"));
        let base = format!("diag:{}", crate::ids::slug(rule));
        crate::ids::unique(&base, &key, |id| self.id_taken(id, taken))
    }

    // ---- lookups ----

    // The entity natural key: normalized name or alias within the scope, and `parent`
    // when the caller supplies it. Without a parent, exactly one match lands; several
    // is an error naming the candidates, so the caller can say which parent it means.
    // Mirrors docs/compiler/concepts/identity.md#the-natural-key-under-containment.
    pub fn find_natural(
        &self,
        name: &str,
        scope: &str,
        parent: Option<&str>,
    ) -> Result<Option<String>, Vec<String>> {
        let want = normalize(name);
        let parent = parent.map(|p| self.resolve_id(p).to_string());
        let mut hits: Vec<String> = self
            .graph
            .entities
            .iter()
            .filter(|(_, e)| e.scope == scope)
            .filter(|(_, e)| {
                normalize(&e.name) == want || e.aliases.iter().any(|a| normalize(a) == want)
            })
            .filter(|(_, e)| match &parent {
                Some(p) => e.parent.as_deref() == Some(p.as_str()),
                None => true,
            })
            .map(|(id, _)| id.clone())
            .collect();
        hits.sort();
        match hits.len() {
            0 => Ok(None),
            1 => Ok(hits.pop()),
            _ => Err(hits),
        }
    }

    // The view natural key: kind plus normalized title.
    pub fn find_view(&self, kind: &str, title: &str) -> Option<String> {
        let want = normalize(title);
        self.graph
            .views
            .iter()
            .find(|(_, v)| v.kind == kind && normalize(&v.title) == want)
            .map(|(id, _)| id.clone())
    }

    // Whether `ancestor` is on `node`'s parent chain (bounded against a cycle).
    pub fn is_ancestor(&self, ancestor: &str, node: &str) -> bool {
        let mut cur = node;
        for _ in 0..64 {
            match self
                .graph
                .entities
                .get(cur)
                .and_then(|e| e.parent.as_deref())
            {
                Some(p) if p == ancestor => return true,
                Some(p) => cur = p,
                None => return false,
            }
        }
        false
    }

    // The parent chain of an entity, nearest first.
    pub(super) fn ancestors(&self, node: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = node;
        for _ in 0..64 {
            match self
                .graph
                .entities
                .get(cur)
                .and_then(|e| e.parent.as_deref())
            {
                Some(p) => {
                    out.push(p.to_string());
                    cur = p;
                }
                None => break,
            }
        }
        out
    }

    pub(super) fn children_of(&self, id: &str) -> Vec<String> {
        self.graph
            .entities
            .iter()
            .filter(|(_, e)| e.parent.as_deref() == Some(id))
            .map(|(cid, _)| cid.clone())
            .collect()
    }

    // A grouping: an entity no document states, holding a level. Derived provenance,
    // no mentions, and no requirements of its own: higher levels carry none, so a
    // derived entity holding requirements is a caps-variant sub-entity, never a
    // grouping. An entity a document states holds children in role, not in
    // provenance. Mirrors docs/compiler/concepts/levels.md#groupings.
    pub fn is_grouping(&self, id: &str) -> bool {
        self.graph.entities.get(id).is_some_and(|e| {
            matches!(e.provenance, Some(Provenance::Derived { .. }))
                && e.mentions.is_empty()
                && self.requirements_referencing(id).is_empty()
        })
    }

    // Dissolve one entity: its children reparent to its parent (parentless when it was
    // top-level, or when that parent dissolved in the same sweep), and it tombstones
    // with a redirect to that parent, so anything holding the old id resolves there.
    // Returns the parent and the children moved. Mirrors docs/compiler/graph.md#the-sweep.
    pub(super) fn dissolve(&mut self, id: &str, build: &str) -> (Option<String>, Vec<String>) {
        let parent = self
            .graph
            .entities
            .get(id)
            .and_then(|e| e.parent.clone())
            .map(|p| self.resolve_id(&p).to_string())
            .filter(|p| self.graph.entities.contains_key(p));
        let children = self.children_of(id);
        for c in &children {
            let e = self.graph.entities.get_mut(c).unwrap();
            e.parent = parent.clone();
            e.updated = Some(build.to_string());
        }
        self.graph.entities.remove(id);
        self.redirect(id, parent.as_deref().unwrap_or_default());
        (parent, children)
    }

    // Redirect `from` to the surviving `to`, one hop: every redirect that named `from`
    // as its survivor now names `to`, so no chain grows across merges. A tombstone
    // (empty `to`) leaves the redirects into `from` alone; they resolve to the dead id.
    // Mirrors docs/compiler/graph.md#storage-layout.
    pub(super) fn redirect(&mut self, from: &str, to: &str) {
        if !to.is_empty() {
            for target in self.graph.redirects.values_mut() {
                if target == from {
                    *target = to.to_string();
                }
            }
        }
        self.graph
            .redirects
            .insert(from.to_string(), to.to_string());
    }

    // Whether a node id names an existing entity or requirement.
    pub(super) fn node_exists(&self, id: &str) -> bool {
        self.graph.entities.contains_key(id)
            || self.graph.requirements.contains_key(id)
            || self.graph.criteria.contains_key(id)
    }
}
