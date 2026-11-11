// The journal: one entry per generation, spend stamps, and the parent move history.
// Mirrors docs/compiler/graph.md#journal.
use super::*;

// The parent moves the journal entries record, oldest first. A mutation carrying both
// `parent` and `prior.parent` (null for parentless) moved its `id`; a `dissolve_entity`
// moved each of its `children` to its `parent`. Mirrors docs/compiler/graph.md#journal.
pub fn parent_moves_in(entries: &[JournalEntry]) -> Vec<ParentMove> {
    let id_or_none = |v: &serde_json::Value| v.as_str().map(String::from);
    let mut out = Vec::new();
    for entry in entries {
        for mv in &entry.mutations {
            let Some(id) = mv["id"].as_str() else {
                continue;
            };
            if mv["op"] == "dissolve_entity" {
                let to = id_or_none(&mv["parent"]);
                for child in mv["children"].as_array().into_iter().flatten() {
                    if let Some(child) = child.as_str() {
                        out.push(ParentMove {
                            generation: entry.generation,
                            child: child.to_string(),
                            from: Some(id.to_string()),
                            to: to.clone(),
                        });
                    }
                }
                continue;
            }
            let has =
                |v: &serde_json::Value, k: &str| v.as_object().is_some_and(|o| o.contains_key(k));
            // A clear journals `clear_parent: true` and no `parent`: a move to none.
            let cleared = mv["clear_parent"] == true;
            if (has(mv, "parent") || cleared) && has(&mv["prior"], "parent") {
                out.push(ParentMove {
                    generation: entry.generation,
                    child: id.to_string(),
                    from: id_or_none(&mv["prior"]["parent"]),
                    to: if cleared {
                        None
                    } else {
                        id_or_none(&mv["parent"])
                    },
                });
            }
        }
    }
    out
}

// The name and scope of every entity the journal created, by id: the natural key of
// a parent that no longer exists.
pub(super) fn created_names_in(entries: &[JournalEntry]) -> BTreeMap<String, (String, String)> {
    let mut out = BTreeMap::new();
    for entry in entries {
        for mv in &entry.mutations {
            if mv["op"] != "create_entity" {
                continue;
            }
            let (Some(id), Some(name)) = (mv["id"].as_str(), mv["entity"]["name"].as_str()) else {
                continue;
            };
            let scope = mv["entity"]["scope"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| Entity::default().scope);
            out.insert(id.to_string(), (name.to_string(), scope));
        }
    }
    out
}

impl Store {
    // The journal entry for one commit. Mirrors docs/compiler/graph.md#journal.
    pub(super) fn journal_entry(
        &self,
        build: &str,
        commit: &Commit,
        mutations: Vec<serde_json::Value>,
    ) -> JournalEntry {
        JournalEntry {
            build: build.to_string(),
            generation: self.status.generation,
            kind: commit.kind.clone(),
            batch: commit.batch.clone(),
            mutations,
            resolved_goals: commit.resolved.clone(),
            opened_goals: Vec::new(),
            rounds: commit.rounds,
            tokens: commit.tokens,
            cached_tokens: commit.cached,
            tokens_source: commit.tokens_source.clone(),
            ..Default::default()
        }
    }

    // Stamp the session's measured spend onto the entries a session just wrote: the
    // entry commits inside the serving, before the runner knows what the turn cost.
    // `cached` is the input among `tokens` the endpoint served from its prompt cache,
    // split the same way; 0 leaves the field absent.
    // Mirrors docs/compiler/graph.md#journal.
    pub fn stamp_tokens(out: &Path, from: u64, to: u64, tokens: u64, cached: u64) {
        if tokens == 0 || to <= from || newer_version(out).is_some() {
            return;
        }
        // A journal rewrite is a store write: under the lock, through the atomic
        // write. Mirrors docs/compiler/graph.md#concurrency.
        let _flock = FileLock::acquire(out);
        // Only the session entries the turn wrote take a share, so the parts sum to
        // the spend: a checks or settle entry between them bills nothing.
        let entries: Vec<(PathBuf, JournalEntry)> = (from + 1..=to)
            .filter_map(|g| {
                let p = crate::layout::journal_file(out, g);
                let text = crate::vfs::read_to_string(&p).ok()?;
                let entry = serde_norway::from_str::<JournalEntry>(&text).ok()?;
                (entry.kind == "session" && entry.tokens.is_none()).then_some((p, entry))
            })
            .collect();
        let n = entries.len() as u64;
        if n == 0 {
            return;
        }
        // The remainder rides on the last entry.
        let split = |total: u64, i: usize| {
            let share = total / n;
            if i as u64 + 1 == n {
                total - share * (n - 1)
            } else {
                share
            }
        };
        for (i, (p, mut entry)) in entries.into_iter().enumerate() {
            entry.tokens = Some(split(tokens, i));
            entry.cached_tokens = (cached > 0).then(|| split(cached, i));
            report_write(&p, write_yaml(&p, &entry));
        }
    }

    // Stamp an external agent's own report of its bill onto one committed session
    // entry (`jazyk spend`). Refuses a generation the journal does not hold, an entry
    // that is not a session, and an entry that already carries a figure, measured or
    // reported: a stamp never overwrites what the harness measured.
    // Mirrors docs/frontends/cli.md#jazyk-spend.
    pub fn stamp_reported(
        out: &Path,
        generation: u64,
        tokens: u64,
        cached: Option<u64>,
    ) -> Result<JournalEntry, String> {
        if tokens == 0 {
            return Err("tokens must be a whole number above zero".to_string());
        }
        if cached.is_some_and(|c| c > tokens) {
            return Err(format!(
                "cached ({}) cannot exceed tokens ({}): the cached input is a share of the bill",
                cached.unwrap_or(0),
                tokens
            ));
        }
        if let Some(why) = newer_store(out) {
            return Err(why);
        }
        let p = crate::layout::journal_file(out, generation);
        if !p.vis_file() {
            return Err(format!(
                "no journal entry g{} under {}",
                generation,
                out.display()
            ));
        }
        // The read, the check, and the write hold the lock, so two stamps never both
        // pass the no-figure check. Mirrors docs/compiler/graph.md#concurrency.
        let _flock = FileLock::acquire(out);
        let text = crate::vfs::read_to_string(&p)
            .map_err(|_| format!("no journal entry g{} under {}", generation, out.display()))?;
        let mut entry = serde_norway::from_str::<JournalEntry>(&text)
            .map_err(|e| format!("journal entry g{} does not parse: {}", generation, e))?;
        if entry.kind != "session" {
            return Err(format!(
                "g{} is a {} entry, not a session; only a session entry carries a spend",
                generation, entry.kind
            ));
        }
        if let Some(t) = entry.tokens {
            let source = match entry.tokens_source.as_deref() {
                Some(crate::model::TOKENS_SOURCE_AGENT) => "reported by the agent",
                _ => "measured by the harness",
            };
            return Err(format!(
                "g{} already carries {} tokens{} ({}); a stamp never overwrites a figure",
                generation,
                t,
                entry
                    .cached_tokens
                    .map(|c| format!(", {} cached", c))
                    .unwrap_or_default(),
                source
            ));
        }
        entry.tokens = Some(tokens);
        entry.cached_tokens = cached.filter(|c| *c > 0);
        entry.tokens_source = Some(crate::model::TOKENS_SOURCE_AGENT.to_string());
        write_yaml(&p, &entry).map_err(|e| format!("write {}: {}", p.display(), e))?;
        Ok(entry)
    }

    pub(super) fn journal_path(&self, generation: u64) -> PathBuf {
        self.out
            .join("local")
            .join("journal")
            .join(format!("g{}.yaml", generation))
    }

    pub(super) fn write_journal(&self, entry: &JournalEntry) {
        let path = self.journal_path(entry.generation);
        report_write(&path, write_yaml(&path, entry));
    }

    // The latest journal mutation that decreed over the given fact and recorded the
    // prior value it replaced. Reads newest first and stops at the first hit.
    // Mirrors docs/compiler/graph.md#journal: a decree entry carries, on a mutation
    // written over a quoted value, the prior value and source.
    pub(super) fn decree_prior(&self, id: &str, op: &str) -> Option<serde_json::Value> {
        for entry in self.journal_newest_first() {
            for mv in entry.mutations.iter().rev() {
                // Every parent move carries a prior; only a decree's prior restores.
                if mv["op"] == op
                    && mv["id"] == id
                    && mv["prior"].is_object()
                    && mv["provenance"]["decree"].is_object()
                {
                    return Some(mv["prior"].clone());
                }
            }
        }
        None
    }

    // The generations the journal holds on disk, newest first.
    fn journal_generations(&self) -> Vec<u64> {
        let dir = crate::layout::journal_dir(&self.out);
        let Ok(read) = crate::vfs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut gens: Vec<u64> = read
            .filter_map(|e| {
                let name = e.ok()?.file_name();
                let name = name.to_str()?;
                name.strip_prefix('g')?
                    .strip_suffix(".yaml")?
                    .parse::<u64>()
                    .ok()
            })
            .collect();
        gens.sort_unstable_by(|a, b| b.cmp(a));
        gens
    }

    // Every journal entry on disk, oldest first.
    pub(super) fn journal_entries(&self) -> Vec<JournalEntry> {
        let mut gens = self.journal_generations();
        gens.reverse();
        gens.into_iter()
            .filter_map(|g| crate::goals::read_journal_entry(&self.journal_path(g), g))
            .collect()
    }

    // The journal entries newest first, each parsed when the scan reaches it, so a
    // lookup that stops at its first hit parses no more than it needs.
    // Mirrors docs/compiler/graph.md#journal.
    fn journal_newest_first(&self) -> impl Iterator<Item = JournalEntry> + '_ {
        self.journal_generations()
            .into_iter()
            .filter_map(move |g| crate::goals::read_journal_entry(&self.journal_path(g), g))
    }

    // Every journaled parent move, oldest first: an `update_entity` or `retract_decree`
    // carrying `parent` and `prior.parent`, and each child of a `dissolve_entity` (a
    // tool's or the sweep's), which moved from the dissolved entity to its parent.
    // Mirrors docs/compiler/graph.md#journal.
    pub fn journaled_parent_moves(&self) -> Vec<ParentMove> {
        parent_moves_in(&self.journal_entries())
    }

    // The last journaled parent move of each child in `moves`, and the name and scope
    // the journal created for every parent those moves and the last ones name that
    // the graph no longer holds. The scan reads newest first and stops once every
    // child has its last move and every dead parent its create (an id is minted once,
    // so the first create found is the only one); a child never moved before and a
    // parent the journal never created run the scan to its end.
    // Mirrors docs/compiler/graph.md#journal.
    pub fn parent_history(
        &self,
        moves: &[(String, Option<String>, Option<String>, usize)],
    ) -> (
        BTreeMap<String, ParentMove>,
        BTreeMap<String, (String, String)>,
    ) {
        let mut pending: BTreeSet<String> = moves.iter().map(|(c, _, _, _)| c.clone()).collect();
        let mut last: BTreeMap<String, ParentMove> = BTreeMap::new();
        let mut names: BTreeMap<String, (String, String)> = BTreeMap::new();
        let mut dead: BTreeSet<String> = BTreeSet::new();
        let want_name = |p: &Option<String>, dead: &mut BTreeSet<String>| {
            if let Some(p) = p {
                if !self.graph.entities.contains_key(p) {
                    dead.insert(p.clone());
                }
            }
        };
        for (_, from, to, _) in moves {
            want_name(from, &mut dead);
            want_name(to, &mut dead);
        }
        for entry in self.journal_newest_first() {
            if pending.is_empty() && dead.is_empty() {
                break;
            }
            let one = std::slice::from_ref(&entry);
            // The last move within the entry is the latest, so the reversed walk's
            // first hit per child stands.
            for mv in parent_moves_in(one).into_iter().rev() {
                if pending.remove(&mv.child) {
                    want_name(&mv.from, &mut dead);
                    want_name(&mv.to, &mut dead);
                    last.insert(mv.child.clone(), mv);
                }
            }
            for (id, name) in created_names_in(one) {
                if dead.remove(&id) {
                    names.insert(id, name);
                }
            }
        }
        (last, names)
    }

    // The natural key a parent matches on across generations: the live entity's name
    // and scope, the same from the journal's create for a dead one (a grouping
    // dissolved and re-minted under a new id counts as the same parent), the id when
    // neither knows it; the scope root for a parentless child.
    // Mirrors docs/compiler/reconciler.md#flip-detection.
    fn parent_key(
        &self,
        parent: Option<&str>,
        child_scope: &str,
        names: &BTreeMap<String, (String, String)>,
    ) -> String {
        let Some(id) = parent else {
            return scope_root_target(child_scope);
        };
        let key = |name: &str, scope: &str| format!("{}|{}", normalize(name), scope);
        match self.graph.entities.get(id) {
            Some(e) => key(&e.name, &e.scope),
            None => names
                .get(id)
                .map(|(name, scope)| key(name, scope))
                .unwrap_or_else(|| id.to_string()),
        }
    }

    // The reparent flip: a commit moves a child to a parent it held before and away
    // from the parent it held last, the same two parents alternating across
    // generations. The journal's last move of the child went the other way between
    // the same keys. One `reparent-flip` record lands on the child, `between` naming
    // the parent it left and the one it returned to (a parentless side as the scope
    // root). Mirrors docs/compiler/reconciler.md#flip-detection.
    pub(super) fn record_reparent_flips(
        &self,
        moves: &[(String, Option<String>, Option<String>, usize)],
        batch: &mut RecordBatch,
    ) {
        if moves.is_empty() {
            return;
        }
        let (history, names) = self.parent_history(moves);
        for (child, from, to, m) in moves {
            let Some(e) = self.graph.entities.get(child) else {
                continue;
            };
            let scope = e.scope.clone();
            let key = |p: Option<&str>| self.parent_key(p, &scope, &names);
            let Some(last) = history.get(child) else {
                continue;
            };
            if key(last.from.as_deref()) != key(to.as_deref())
                || key(last.to.as_deref()) != key(from.as_deref())
            {
                continue;
            }
            let label = |p: &Option<String>| p.clone().unwrap_or_else(|| scope_root_target(&scope));
            batch.push(
                *m,
                CHANGE_REPARENT_FLIP,
                child,
                "parent",
                serde_json::json!({ "between": [label(from), label(to)] }),
            );
        }
    }
}
