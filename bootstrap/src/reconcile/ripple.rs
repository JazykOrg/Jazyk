use super::*;

// ---- ripple: the causality DAG over the journal ----

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct RippleNode {
    pub generation: u64,
    pub kind: String,
    pub batch: Vec<String>,
    pub label: String,
    pub summary: String,
    pub resolved: Vec<Resolved>,
    pub opened: Vec<OpenedGoal>,
    pub recomputed: Vec<String>,
    pub children: Vec<RippleNode>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct RippleTree {
    pub root: RippleNode,
    pub sessions: u64,
    pub tokens: u64,
    pub recomputes: usize,
    pub by_kind: BTreeMap<String, CostLine>,
    pub parked: Vec<ParkedGoal>,
    pub failed: Vec<FailedGoal>,
    pub verdict: String,
}

fn entry_label(entry: &JournalEntry) -> String {
    match entry.kind.as_str() {
        "session" => entry
            .batch
            .iter()
            .filter_map(|g| goals::parse_goal_id(g))
            .map(|(k, t)| format!("{} {}", k, t))
            .collect::<Vec<_>>()
            .join(", "),
        "edit" => format!(
            "{} (human)",
            entry.dirtied.first().cloned().unwrap_or_default()
        ),
        k => k.to_string(),
    }
}

fn entry_summary(entry: &JournalEntry) -> String {
    if !entry.resolved_goals.is_empty() {
        return entry
            .resolved_goals
            .iter()
            .map(|r| r.justification.clone())
            .filter(|j| !j.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
    }
    let mut ops: BTreeMap<String, usize> = BTreeMap::new();
    for m in &entry.mutations {
        let name = m
            .as_object()
            .and_then(|o| o.keys().next().cloned())
            .or_else(|| m["op"].as_str().map(String::from))
            .unwrap_or_else(|| "mutation".into());
        *ops.entry(name).or_insert(0) += 1;
    }
    ops.iter()
        .map(|(k, n)| format!("{} {}", n, k))
        .collect::<Vec<_>>()
        .join(", ")
}

fn recomputed_in(entry: &JournalEntry) -> Vec<String> {
    entry
        .mutations
        .iter()
        .filter_map(|m| {
            let o = m.as_object()?;
            let (op, body) = o.iter().next()?;
            match op.as_str() {
                "CreateRequirement" | "UpdateRequirement" | "DeleteRequirement" => {
                    let id = body["id"].as_str()?;
                    let stem = id.strip_prefix("req:")?.rsplit_once('-')?.0.to_string();
                    Some(format!("relationships and machines of {}", stem))
                }
                _ => None,
            }
        })
        .take(1)
        .collect()
}

// The entry that resolved a goal, or the first session that took it, after `after`.
fn resolver<'a>(entries: &'a [JournalEntry], goal: &str, after: u64) -> Option<&'a JournalEntry> {
    entries
        .iter()
        .filter(|e| e.generation > after)
        .find(|e| e.resolved_goals.iter().any(|r| r.goal == goal))
        .or_else(|| {
            entries
                .iter()
                .filter(|e| e.generation > after && e.kind == "session")
                .find(|e| e.batch.iter().any(|b| b == goal))
        })
}

fn forward(entries: &[JournalEntry], entry: &JournalEntry, seen: &mut BTreeSet<u64>) -> RippleNode {
    seen.insert(entry.generation);
    let mut node = RippleNode {
        generation: entry.generation,
        kind: entry.kind.clone(),
        batch: entry.batch.clone(),
        label: entry_label(entry),
        summary: entry_summary(entry),
        resolved: entry.resolved_goals.clone(),
        opened: entry.opened_goals.clone(),
        recomputed: recomputed_in(entry),
        children: Vec::new(),
    };
    for o in &entry.opened_goals {
        if let Some(child) = resolver(entries, &o.goal, entry.generation) {
            if seen.contains(&child.generation) {
                continue;
            }
            node.children.push(forward(entries, child, seen));
        }
    }
    node
}

// The entry whose opened goals name what this entry resolved or took.
fn parent_of<'a>(entries: &'a [JournalEntry], entry: &JournalEntry) -> Option<&'a JournalEntry> {
    let mine: BTreeSet<&str> = entry
        .batch
        .iter()
        .map(String::as_str)
        .chain(entry.resolved_goals.iter().map(|r| r.goal.as_str()))
        .collect();
    entries
        .iter()
        .filter(|e| e.generation < entry.generation)
        .rev()
        .find(|e| {
            e.opened_goals
                .iter()
                .any(|o| mine.contains(o.goal.as_str()))
        })
}

fn touches(entry: &JournalEntry, target: &str) -> bool {
    let goal_target = |g: &str| {
        goals::parse_goal_id(g)
            .map(|(_, t)| {
                t == target
                    || t.contains(&format!("{}~", target))
                    || t.ends_with(&format!("~{}", target))
            })
            .unwrap_or(false)
    };
    entry.batch.iter().any(|g| goal_target(g))
        || entry.resolved_goals.iter().any(|r| goal_target(&r.goal))
        || entry.opened_goals.iter().any(|o| goal_target(&o.goal))
        || entry
            .dirtied
            .iter()
            .any(|d| d == target || d.starts_with(&format!("{}#", target)))
        || entry.mutations.iter().any(|m| {
            let s = m.to_string();
            s.contains(&format!("\"{}\"", target))
        })
}

// The ripple rooted at a generation, a document, or a node. `back` walks causes
// instead of consequences. Mirrors docs/frontends/cli.md#jazyk-ripple.
pub fn ripple(store: &Store, root: &str, back: bool) -> Option<RippleTree> {
    let entries = goals::read_journal(&store.out);
    if entries.is_empty() {
        return None;
    }
    let by_gen = |n: u64| entries.iter().find(|e| e.generation == n);
    let generation = root
        .strip_prefix('g')
        .unwrap_or(root)
        .parse::<u64>()
        .ok()
        .filter(|_| root.chars().all(|c| c.is_ascii_digit() || c == 'g'));
    let start: &JournalEntry = if let Some(n) = generation {
        by_gen(n)?
    } else if crate::md::is_document_target(root) {
        entries.iter().rev().find(|e| {
            e.kind == "edit"
                && e.dirtied
                    .iter()
                    .any(|d| d.starts_with(&format!("{}#", root)) || d == root)
        })?
    } else {
        // A cascade roots at an entry that carries goals; a check run names the node
        // without explaining it (docs/frontends/cli.md#jazyk-ripple).
        let roots_a_cascade = |e: &JournalEntry| {
            e.kind == "edit"
                || !e.batch.is_empty()
                || !e.resolved_goals.is_empty()
                || !e.opened_goals.is_empty()
        };
        let last = entries
            .iter()
            .rev()
            .find(|e| touches(e, root) && roots_a_cascade(e))
            .or_else(|| entries.iter().rev().find(|e| touches(e, root)))?;
        // The last cascade that touched it: back to its root, then forward.
        let mut cur = last;
        let mut hops = 0;
        while let Some(p) = parent_of(&entries, cur) {
            cur = p;
            hops += 1;
            if hops > 64 {
                break;
            }
        }
        cur
    };
    let root_node = if back {
        let mut chain: Vec<&JournalEntry> = vec![start];
        let mut cur = start;
        let mut hops = 0;
        while let Some(p) = parent_of(&entries, cur) {
            chain.push(p);
            cur = p;
            hops += 1;
            if hops > 64 {
                break;
            }
        }
        // Render causes as a chain from the root cause down to the start.
        let mut node: Option<RippleNode> = None;
        for e in chain {
            let mut n = RippleNode {
                generation: e.generation,
                kind: e.kind.clone(),
                batch: e.batch.clone(),
                label: entry_label(e),
                summary: entry_summary(e),
                resolved: e.resolved_goals.clone(),
                opened: e.opened_goals.clone(),
                recomputed: recomputed_in(e),
                children: Vec::new(),
            };
            if let Some(child) = node.take() {
                n.children.push(child);
            }
            node = Some(n);
        }
        node.unwrap()
    } else {
        forward(&entries, start, &mut BTreeSet::new())
    };
    let mut sessions = 0u64;
    let mut tokens = 0u64;
    let mut recomputes = 0usize;
    let mut by_kind: BTreeMap<String, CostLine> = BTreeMap::new();
    fn walk(
        n: &RippleNode,
        sessions: &mut u64,
        tokens: &mut u64,
        recomputes: &mut usize,
        by_kind: &mut BTreeMap<String, CostLine>,
        entries: &[JournalEntry],
    ) {
        if n.kind == "session" {
            *sessions += 1;
            // An external session reports no tokens: it counts as a session and adds
            // nothing to the spend (docs/compiler/graph.md#journal).
            let t = entries
                .iter()
                .find(|e| e.generation == n.generation)
                .and_then(|e| e.tokens)
                .unwrap_or(0);
            *tokens += t;
            if let Some((k, _)) = n.batch.first().and_then(|g| goals::parse_goal_id(g)) {
                let line = by_kind.entry(k.to_string()).or_default();
                line.sessions += 1;
                line.tokens += t;
            }
        }
        *recomputes += n.recomputed.len();
        for c in &n.children {
            walk(c, sessions, tokens, recomputes, by_kind, entries);
        }
    }
    walk(
        &root_node,
        &mut sessions,
        &mut tokens,
        &mut recomputes,
        &mut by_kind,
        &entries,
    );
    Some(RippleTree {
        root: root_node,
        sessions,
        tokens,
        recomputes,
        by_kind,
        parked: store.status.parked.clone(),
        failed: store.status.failed.clone(),
        verdict: store.status.verdict.to_string(),
    })
}

// Plain text, one line per journal entry, indented per goal.
pub fn render_ripple(tree: &RippleTree) -> String {
    // A batch that resolved several goals prints a header and one line per goal:
    // joining them put the whole batch past any terminal's width.
    // Mirrors docs/frontends/cli.md#jazyk-ripple.
    fn many(n: &RippleNode) -> bool {
        n.resolved.len() > 1
    }
    fn header(n: &RippleNode) -> String {
        let mut kinds: Vec<&str> = n
            .batch
            .iter()
            .filter_map(|g| goals::parse_goal_id(g).map(|(k, _)| k))
            .collect();
        kinds.dedup();
        format!(
            "{} x{} g{}",
            kinds.join(", "),
            n.resolved.len(),
            n.generation
        )
    }
    fn line(n: &RippleNode) -> String {
        if many(n) {
            return header(n);
        }
        let mut s = format!("{} g{}", n.label, n.generation);
        if !n.summary.is_empty() {
            s.push_str(&format!(": {}", n.summary));
        }
        s
    }
    fn goal_lines(n: &RippleNode, prefix: &str, out: &mut String) {
        if !many(n) {
            return;
        }
        for r in &n.resolved {
            let target = goals::parse_goal_id(&r.goal)
                .map(|(_, t)| t.to_string())
                .unwrap_or_else(|| r.goal.clone());
            if r.justification.is_empty() {
                out.push_str(&format!("{}  {}\n", prefix, target));
            } else {
                out.push_str(&format!("{}  {}: {}\n", prefix, target, r.justification));
            }
        }
    }
    fn walk(n: &RippleNode, prefix: &str, out: &mut String) {
        for r in &n.recomputed {
            out.push_str(&format!("{}│  recomputed at commit: {}\n", prefix, r));
        }
        let count = n.children.len();
        for (i, c) in n.children.iter().enumerate() {
            let last = i + 1 == count;
            out.push_str(&format!(
                "{}{} {}\n",
                prefix,
                if last { "└─" } else { "├─" },
                line(c)
            ));
            let next = format!("{}{}", prefix, if last { "   " } else { "│  " });
            goal_lines(c, &next, out);
            walk(c, &next, out);
        }
    }
    let mut out = String::new();
    out.push_str(&format!(
        "{} {}\n",
        tree.root.kind,
        line(&tree.root).trim_start_matches(&format!("{} ", tree.root.kind))
    ));
    goal_lines(&tree.root, "", &mut out);
    walk(&tree.root, "", &mut out);
    let gc: usize = tree
        .by_kind
        .iter()
        .filter(|(k, _)| goals::kind(k).is_some_and(|x| x.class() == goals::Class::Gc))
        .map(|(_, l)| l.sessions as usize)
        .sum();
    out.push_str(&if gc == 0 {
        "gc: no goals derived\n".to_string()
    } else {
        format!("gc: {} session(s)\n", gc)
    });
    let tokens = if tree.tokens >= 1000 {
        format!("{}k", tree.tokens / 1000)
    } else {
        tree.tokens.to_string()
    };
    out.push_str(&format!(
        "{}: {} sessions, {} recomputes, {} tokens\n",
        tree.verdict, tree.sessions, tree.recomputes, tokens
    ));
    for p in &tree.parked {
        match &p.reason {
            Some(r) => out.push_str(&format!("parked: {} ({})\n", p.goal.id, r)),
            None => out.push_str(&format!("parked: {}\n", p.goal.id)),
        }
    }
    for f in &tree.failed {
        out.push_str(&format!("failed: {} ({})\n", f.goal.id, f.reason));
    }
    if tree.parked.is_empty() && tree.failed.is_empty() {
        out.push_str("nothing parked or failed\n");
    }
    out
}

#[cfg(test)]
mod tests;
