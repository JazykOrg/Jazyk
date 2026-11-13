use super::document::path_to_uri;
use super::Lsp;
use crate::md;
use crate::model::VIEW_KINDS;
use crate::vfs::VPathExt;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

impl Lsp {
    // ---- rendered views ----

    // The rendered `.svg` of a view, only when the build output exists on disk. The
    // hover never renders a view itself. Mirrors docs/frontends/lsp.md#capabilities.
    fn view_svg_path(&self, view_id: &str) -> Option<PathBuf> {
        let rel = view_id.strip_prefix("view:")?;
        let (kind, slug) = rel.split_once('/')?;
        let p = crate::layout::diagrams_dir(&self.out)
            .join(kind)
            .join(format!("{}.svg", slug));
        p.vexists().then_some(p)
    }

    // ---- the walk ----
    // The pages docsgen writes under <out>/docsgen/, by the same slugs it uses, so a
    // hover link and a type definition land on the file the last commit rendered.
    // Mirrors docs/consumers/docsgen.md#entity-cards and #diagram-pages.

    // An entity's card: `docsgen/entities/<slug>.md`.
    fn card_path(&self, id: &str) -> PathBuf {
        crate::layout::docsgen_dir(&self.out)
            .join("entities")
            .join(format!("{}.md", crate::ids::entity_slug(id)))
    }

    // An entity's requirements document: `docsgen/<slug>.md`.
    fn requirements_doc_path(&self, id: &str) -> PathBuf {
        crate::layout::docsgen_dir(&self.out).join(format!("{}.md", crate::ids::entity_slug(id)))
    }

    // A view's diagram page: `docsgen/diagrams/<kind>/<slug>.md`.
    fn diagram_page_path(&self, view_id: &str) -> Option<PathBuf> {
        let rel = view_id.strip_prefix("view:")?;
        let (kind, slug) = rel.split_once('/')?;
        Some(
            crate::layout::docsgen_dir(&self.out)
                .join("diagrams")
                .join(kind)
                .join(format!("{}.md", slug)),
        )
    }

    // A level's page: `docsgen/levels/<slug>.md`, or `levels/scope-<scope>.md` for a
    // scope root.
    fn level_page_path(&self, target: &str) -> PathBuf {
        crate::layout::docsgen_dir(&self.out).join(crate::docsgen::level_page(target))
    }

    // The entity's card in short, read from the shared model (card.rs) so the hover
    // shows what docsgen and the GUI show. In order: the header, `Sits in`, the
    // in-context image (the parent level's structural view), the `Inside` link with
    // the child count (never a second image), one line of links to the long reads,
    // and a derived entity's pending proposal. Blocks are separated by blank lines,
    // so every markdown renderer keeps them apart. None for an unknown id.
    // Mirrors docs/frontends/lsp.md#capabilities.
    pub(super) fn entity_card_short(&self, id: &str) -> Option<String> {
        let walk = crate::card::Walk::new(&self.store);
        let card = crate::card::entity_card(&self.store, &walk, id)?;
        let mut s = format!("**{}**", card.name);
        if let Some(st) = &card.stereotype {
            s.push_str(&format!(" «{}»", st));
        }
        if !card.definition.is_empty() {
            s.push_str(&format!(" · {}", card.definition));
        }
        // Sits in: the breadcrumb as text, the entity itself last and bold.
        let n = card.breadcrumb.len();
        let crumbs: Vec<String> = card
            .breadcrumb
            .iter()
            .enumerate()
            .map(|(i, c)| {
                if i + 1 == n {
                    format!("**{}**", c.name)
                } else {
                    c.name.clone()
                }
            })
            .collect();
        s.push_str(&format!("\n\nSits in: {}", crumbs.join(" › ")));
        // In context: the build output the last commit rendered, captioned with the
        // view id. A missing rendering leaves the caption alone, linked to the
        // view's page; the hover never renders a view itself.
        if let Some(ctx) = &card.context {
            match self.view_svg_path(ctx) {
                Some(svg) => s.push_str(&format!("\n\n![{}]({})", ctx, path_to_uri(&svg))),
                None => {
                    let page = self
                        .diagram_page_path(ctx)
                        .map(|p| path_to_uri(&p))
                        .unwrap_or_default();
                    s.push_str(&format!(
                        "\n\nIn context: [{}]({}) (not rendered)",
                        ctx, page
                    ));
                }
            }
        }
        // Inside: the entity's own level, as a link to its diagram page.
        if let Some(inside) = &card.inside {
            let page = self
                .diagram_page_path(inside)
                .map(|p| path_to_uri(&p))
                .unwrap_or_default();
            let k = card.children.len();
            s.push_str(&format!(
                "\n\nInside: [{}]({}) ({} {})",
                inside,
                page,
                k,
                if k == 1 { "child" } else { "children" }
            ));
        }
        // The long reads: the card, the requirements document, the level page of the
        // parent's level (only when the parent has one, as its context view says).
        let mut links = vec![
            format!("[card]({})", path_to_uri(&self.card_path(&card.id))),
            format!(
                "[requirements ({})]({})",
                card.requirement_count,
                path_to_uri(&self.requirements_doc_path(&card.id))
            ),
        ];
        if card.context.is_some() && n >= 2 {
            let parent = &card.breadcrumb[n - 2];
            links.push(format!(
                "[level: {}]({})",
                parent.name,
                path_to_uri(&self.level_page_path(&parent.id))
            ));
        }
        s.push_str(&format!("\n\n{}", links.join(" · ")));
        // A derived or decreed entity says so and names its pending proposal.
        if card.provenance != "quote" {
            match &card.proposal {
                Some(did) => s.push_str(&format!(
                    "\n\n{} · proposal pending: `{}`",
                    card.provenance, did
                )),
                None => s.push_str(&format!("\n\n{} · no proposal pending", card.provenance)),
            }
        }
        Some(s)
    }

    // The image of the smallest flow view listing a requirement as a member, so the
    // step shows in its flow above the card. Same tie rule as for entities.
    fn flow_view_image(&self, rid: &str) -> Option<String> {
        const FLOW: [&str; 4] = ["use-case", "activity", "sequence", "communication"];
        let mut best: Option<(usize, usize, String)> = None;
        for (vid, v) in &self.store.graph.views {
            if !FLOW.contains(&v.kind.as_str()) {
                continue;
            }
            if !v.members.iter().any(|m| self.store.resolve_id(m) == rid) {
                continue;
            }
            if self.view_svg_path(vid).is_none() {
                continue;
            }
            let pos = VIEW_KINDS
                .iter()
                .position(|k| *k == v.kind)
                .unwrap_or(usize::MAX);
            let key = (v.members.len(), pos, vid.clone());
            if best.as_ref().map(|b| key < *b).unwrap_or(true) {
                best = Some(key);
            }
        }
        let (_, _, vid) = best?;
        let v = &self.store.graph.views[&vid];
        let svg = self.view_svg_path(&vid)?;
        Some(format!(
            "![{} ({})]({})\n\n",
            v.title,
            v.kind,
            path_to_uri(&svg)
        ))
    }

    // ---- request handlers ----

    pub(super) fn on_definition(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else {
            return Value::Null;
        };
        if let Some(targets) = self.code_definition(&doc, line) {
            return targets;
        }
        let Some((_, e)) = self.entity_at(&doc, line, ch) else {
            return Value::Null;
        };
        // The defining mention is the first one recorded.
        let Some(m) = e.mentions.first() else {
            return Value::Null;
        };
        let r = self.anchor(&m.doc, &m.quote, &e.name, Some(&m.section));
        json!({ "uri": self.doc_to_uri(&m.doc), "range": self.range(r) })
    }

    pub(super) fn on_references(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else {
            return json!([]);
        };
        let Some((_, e)) = self.entity_at(&doc, line, ch) else {
            return json!([]);
        };
        let mut locs: Vec<Value> = Vec::new();
        let mut seen: BTreeSet<(String, usize)> = BTreeSet::new();
        for m in &e.mentions {
            let r = self.anchor(&m.doc, &m.quote, &e.name, Some(&m.section));
            if seen.insert((m.doc.clone(), r.0)) {
                locs.push(json!({ "uri": self.doc_to_uri(&m.doc), "range": self.range(r) }));
            }
        }
        json!(locs)
    }

    // Go to type definition: the entity's card at line 0. Definition stays the
    // defining mention in the prose; the type definition is the walk.
    // Mirrors docs/frontends/lsp.md#capabilities.
    pub(super) fn on_type_definition(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else {
            return Value::Null;
        };
        let Some((id, _)) = self.entity_at(&doc, line, ch) else {
            return Value::Null;
        };
        json!({
            "uri": path_to_uri(&self.card_path(&id)),
            "range": self.range((0, 0, 0, 0))
        })
    }

    // Hover on an entity renders its card in short, with a verification summary from
    // the ledger. Hovering inside a requirement's located quote shows that
    // requirement's own card. Mirrors docs/frontends/lsp.md#capabilities.
    pub(super) fn on_hover(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else {
            return Value::Null;
        };
        let cards: Vec<String> = self
            .code_requirements(&doc)
            .into_iter()
            .filter(|(_, l)| *l == line)
            .filter_map(|(rid, _)| {
                self.store
                    .graph
                    .requirements
                    .get(&rid)
                    .map(|r| self.requirement_card(&rid, r))
            })
            .collect();
        if !cards.is_empty() {
            let text = self.overlay.get(&doc).cloned().unwrap_or_else(|| {
                crate::vfs::read_to_string(self.root.join(&doc)).unwrap_or_default()
            });
            let len = text
                .lines()
                .nth(line)
                .map(|l| l.encode_utf16().count())
                .unwrap_or(0);
            return json!({"contents": {"kind": "markdown", "value": cards.join("\n\n---\n\n")},
                "range": self.range((line, 0, line, len))});
        }
        if let Some((id, _)) = self.entity_at(&doc, line, ch) {
            let Some(mut value) = self.entity_card_short(&id) else {
                return Value::Null;
            };
            let vmap = crate::verify::status_map(&self.store, &self.gen);
            let refs = self.store.requirements_referencing(&id);
            // Only requirements the ledger holds a row for count: a project that
            // never generated shows no verification line at all.
            let statuses: Vec<&str> = refs
                .iter()
                .filter_map(|r| vmap.get(r).and_then(|v| v["status"].as_str()))
                .filter(|s| *s != "missing")
                .collect();
            if !statuses.is_empty() {
                let ok = statuses.iter().filter(|s| **s == "verified").count();
                let bad = statuses.iter().filter(|s| **s == "failing").count();
                let stale = statuses.iter().filter(|s| s.starts_with("stale")).count();
                value.push_str(&format!(
                    "\n\n---\nverification: {}/{} verified, {} failing, {} stale",
                    ok,
                    statuses.len(),
                    bad,
                    stale
                ));
            }
            return json!({ "contents": { "kind": "markdown", "value": value } });
        }
        // No entity under the cursor: a requirement's quote, maybe.
        if let Some((rid, r)) = self.requirement_at(&doc, line, ch) {
            let value = self.requirement_card(&rid, r);
            let mut hover = json!({ "contents": { "kind": "markdown", "value": value } });
            // The hover range is the located quote, so the whole statement highlights.
            if let Some(q) = r
                .source
                .as_ref()
                .and_then(|s| md::locate(&self.doc_text(&doc), &s.quote))
            {
                hover["range"] = self.range(q);
            }
            return hover;
        }
        Value::Null
    }

    // The requirement card: the requirement, the code, and the test, each linked, with
    // the derived verification status. Links are absolute `file://` URIs with an
    // `#L<line>` fragment so any client navigates; the requirement link carries the id
    // as `?req=` for clients that route to the node itself.
    // Mirrors docs/frontends/lsp.md#capabilities.
    pub(super) fn requirement_card(&self, rid: &str, r: &crate::model::Requirement) -> String {
        if let (Some(id), local) = crate::webapi::sets::unqualify(rid) {
            if let Ok(p) = crate::project::Project::load_set(&self.root, &id) {
                let scoped = Self::new(
                    p.root.clone(),
                    p.out.clone(),
                    crate::gen::GenSettings::resolve(&p),
                );
                if let Some(r) = scoped.store.graph.requirements.get(&local) {
                    return format!(
                        "**{}**\n\n{}",
                        p.set_name.unwrap_or(id),
                        scoped.requirement_card(&local, r)
                    );
                }
            }
        }
        let ledger = crate::gen::Ledger::load(&self.out);
        let row = ledger.requirements.get(rid);
        let (status, reason) = match row {
            Some(row) => crate::verify::status_of(&self.store, rid, row, &self.gen),
            None => ("missing".to_string(), "not-generated".to_string()),
        };
        let mut s = String::new();
        // The smallest flow view listing this requirement sits above the card.
        if let Some(img) = self.flow_view_image(rid) {
            s.push_str(&img);
        }
        // The header reads the status word; the test part keeps the raw status.
        let word = crate::verify::status_word(
            &status,
            &reason,
            crate::verify::questioned(&self.store).contains(rid),
        );
        s.push_str(&format!(
            "**`{}`** · {} {}\n\n{}\n\n",
            rid,
            word_glyph(word),
            word,
            r.statement
        ));
        if let Some(link) = self.docsgen_link(rid, r) {
            s.push_str(&format!("[the requirement →]({})\n\n", link));
        }
        let criteria: Vec<_> = self
            .store
            .graph
            .criteria
            .iter()
            .filter(|(_, c)| self.store.resolve_id(&c.requirement) == rid)
            .collect();
        if !criteria.is_empty() {
            s.push_str("**acceptance criteria**\n\n");
            for (cid, criterion) in criteria {
                s.push_str(&format!("- `{}` · {}\n", cid, criterion.statement));
                if let Some(test) = row.and_then(|row| row.tests.get(cid)) {
                    let artifact = crate::gen::artifact_path(&self.out, &self.gen, &test.test);
                    let line = crate::vfs::read_to_string(&artifact).ok().and_then(|text| {
                        text.lines().position(|line| {
                            !test.test.name.is_empty() && line.contains(&test.test.name)
                        })
                    });
                    let target = match line {
                        Some(line) => format!("{}#L{}", path_to_uri(&artifact), line + 1),
                        None => path_to_uri(&artifact),
                    };
                    s.push_str(&format!(
                        "  - [test: `{}`]({}) · {}\n",
                        test.test.name, target, test.verdict
                    ));
                }
            }
            s.push('\n');
        }
        s.push_str("---\n\n");

        // The code: anchored sites first, each relocated against the file as it stands,
        // then manifest files that carry no site.
        let mut code: Vec<String> = Vec::new();
        if let Some(row) = row {
            let mut anchored: BTreeSet<&str> = BTreeSet::new();
            for site in &row.sites {
                anchored.insert(site.file.as_str());
                let abs = self.gen.deliverable.join(&site.file);
                let text = crate::vfs::read_to_string(&abs).unwrap_or_default();
                match crate::gen::locate_head(&text, &site.head, site.line) {
                    Some((l, exact)) => code.push(format!(
                        "- [`{}:{}`]({}#L{}){}",
                        site.file,
                        l,
                        path_to_uri(&abs),
                        l,
                        if exact { "" } else { " · moved" }
                    )),
                    None => code.push(format!(
                        "- [`{}`]({}) · site lost",
                        site.file,
                        path_to_uri(&abs)
                    )),
                }
            }
            for f in &row.files {
                if anchored.contains(f.as_str()) {
                    continue;
                }
                let abs = self.gen.deliverable.join(f);
                code.push(format!("- [`{}`]({})", f, path_to_uri(&abs)));
            }
        }
        if code.is_empty() {
            s.push_str("**code** · not generated\n\n");
        } else {
            s.push_str("**code**\n\n");
            s.push_str(&code.join("\n"));
            s.push_str("\n\n");
        }

        // The test: the artifact at the line its name sits on, then the status.
        s.push_str("---\n\n");
        let Some(row) = row else {
            s.push_str("**test** · not generated\n");
            return s;
        };
        let t = &row.test;
        let artifact = crate::gen::artifact_path(&self.out, &self.gen, t);
        let label = if t.label == t.kind {
            t.kind.clone()
        } else {
            format!("{} · {}", t.kind, t.label)
        };
        s.push_str(&format!("**test** · {}\n\n", label));
        let line = if t.name.is_empty() {
            None
        } else {
            crate::vfs::read_to_string(&artifact).ok().and_then(|text| {
                text.lines()
                    .position(|l| l.contains(&t.name))
                    .map(|i| i + 1)
            })
        };
        // An llm test's criteria are metadata under the out directory, not part of the
        // product: the link carries the id, so a client with no page for the artifact
        // lands on the requirement's verification detail instead.
        let target = if t.kind == "llm" {
            format!("{}?req={}", path_to_uri(&artifact), rid)
        } else {
            path_to_uri(&artifact)
        };
        match line {
            Some(l) => s.push_str(&format!(
                "- [`{}:{}`]({}#L{}) · `{}`\n",
                t.artifact, l, target, l, t.name
            )),
            None => s.push_str(&format!(
                "- [`{}`]({}){}\n",
                t.artifact,
                target,
                if artifact.vexists() {
                    ""
                } else {
                    " · artifact gone"
                }
            )),
        }
        s.push_str(&format!(
            "- {} {} · {}",
            status_glyph(&status),
            status,
            reason
        ));
        if let Some(last) = &row.last_run {
            s.push_str(&format!(" · last run {}", last));
        }
        s.push('\n');
        s.push_str(&format!("- run `{}`\n", t.run));
        if let Some(ev) = &row.evidence {
            s.push_str(&format!(
                "\n> {}\n",
                ev.split_whitespace().collect::<Vec<_>>().join(" ")
            ));
        }
        s
    }

    // Link to a requirement's heading in its entity's requirements document. None when
    // the document does not exist, so the card never dangles.
    fn docsgen_link(&self, rid: &str, r: &crate::model::Requirement) -> Option<String> {
        let ent = r.entities.first()?;
        let slug = self
            .store
            .resolve_id(ent)
            .strip_prefix("ent:")
            .unwrap_or(ent)
            .to_string();
        let path = crate::layout::docsgen_dir(&self.out).join(format!("{}.md", slug));
        let text = crate::vfs::read_to_string(&path).ok()?;
        let heading = format!("### `{}`", rid);
        let line = text
            .lines()
            .position(|l| l.trim() == heading)
            .map(|i| i + 1)
            .unwrap_or(1);
        Some(format!("{}?req={}#L{}", path_to_uri(&path), rid, line))
    }

    // The requirement whose located quote contains the position, if any.
    pub(super) fn requirement_at(
        &self,
        doc: &str,
        line: usize,
        character: usize,
    ) -> Option<(String, &crate::model::Requirement)> {
        let text = self.doc_text(doc);
        for (rid, r) in &self.store.graph.requirements {
            let Some(src) = r.source.as_ref().filter(|s| s.doc == doc) else {
                continue;
            };
            if let Some((sl, sc, el, ec)) = md::locate(&text, &src.quote) {
                let after_start = line > sl || (line == sl && character >= sc);
                let before_end = line < el || (line == el && character <= ec);
                if after_start && before_end {
                    return Some((rid.clone(), r));
                }
            }
        }
        None
    }

    // Every whole-word occurrence of an entity name or alias links to that entity's
    // requirements document under <out>/docsgen/. Links are emitted only when the
    // target file exists, so they never dangle. Longest name wins on overlaps, like
    // entity_at; at most 200 links per document.
    pub(super) fn on_document_links(&self, params: &Value) -> Value {
        let Some(doc) = self.param_doc(params) else {
            return json!([]);
        };
        let text = self.doc_text(&doc);
        let docsgen = |id: &str| {
            let slug = id.strip_prefix("ent:").unwrap_or(id);
            crate::layout::docsgen_dir(&self.out).join(format!("{}.md", slug))
        };
        let links: Vec<Value> = self
            .entity_mentions(&text, |id| docsgen(id).vexists())
            .into_iter()
            .take(200)
            .map(|(line, col, len, id)| {
                let name = self
                    .store
                    .graph
                    .entities
                    .get(&id)
                    .map(|e| e.name.as_str())
                    .unwrap_or(&id);
                json!({
                    "range": self.range((line, col, line, col + len)),
                    "target": path_to_uri(&docsgen(&id)),
                    "tooltip": format!("{}: requirements document", name)
                })
            })
            .collect();
        json!(links)
    }
}

// One glyph per status word, so a word reads at a glance in clients that render no
// colour. Mirrors docs/consumers/gen.md#status-words.
fn word_glyph(word: &str) -> &'static str {
    match word {
        "verified" => "✓",
        "failing" => "✗",
        "stale" => "↻",
        "question" => "?",
        _ => "–",
    }
}

// One glyph per derived verification status, so the card reads at a glance in clients
// that render no colour. Mirrors docs/consumers/gen.md#status-is-derived-never-stored.
fn status_glyph(status: &str) -> &'static str {
    match status {
        "verified" => "✓",
        "failing" => "✗",
        "unverified" => "○",
        s if s.starts_with("stale") => "↻",
        _ => "–",
    }
}
