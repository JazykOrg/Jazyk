use super::*;

impl ToolSession {
    pub(super) fn gen_settings(&self) -> crate::gen::GenSettings {
        self.gen.clone()
    }

    // The generation turn's file and command tools, sandboxed to the deliverable.
    // In-process only. Mirrors docs/compiler/sessions.md#generation-sessions.
    fn deliverable_path(&self, rel: &str) -> Result<std::path::PathBuf, ToolError> {
        let rel = rel.trim().trim_start_matches("./");
        if rel.is_empty() || rel.starts_with('/') || rel.split('/').any(|c| c == "..") {
            return Err(ToolError::new(
                "bad-path",
                format!(
                    include_str!(concat!(
                        env!("OUT_DIR"),
                        "/payloads/goals/prompts/inline/tools-files/must-be-a-relative-path-under-the-cd80b.md"
                    )),
                    rel
                ),
            ));
        }
        // Symlinks resolve before the path is trusted: the deepest existing ancestor
        // is canonicalized and must stay under the canonical deliverable, so a link
        // inside the deliverable pointing outside is refused like `..`. The part that
        // does not exist yet (a file about to be written) rides on the resolved
        // ancestor. Mirrors docs/compiler/tools.md#generation-tools.
        let root = self.deliverable_root();
        let joined = root.join(rel);
        let mut probe = joined.clone();
        let mut tail: Vec<std::ffi::OsString> = Vec::new();
        while !probe.vexists() {
            let Some(name) = probe.file_name().map(|n| n.to_os_string()) else {
                break;
            };
            tail.push(name);
            probe = probe
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| root.clone());
        }
        let mut resolved = probe.vcanonicalize().unwrap_or(probe);
        if !resolved.starts_with(&root) {
            return Err(ToolError::new(
                "bad-path",
                format!(
                    "`{}` resolves outside the deliverable directory through a symlink; the deliverable is the sandbox",
                    rel
                ),
            ));
        }
        for name in tail.into_iter().rev() {
            resolved.push(name);
        }
        Ok(resolved)
    }

    // The deliverable directory with its own symlinks resolved, the prefix every
    // sandboxed path must keep.
    fn deliverable_root(&self) -> std::path::PathBuf {
        self.gen
            .deliverable
            .vcanonicalize()
            .unwrap_or_else(|_| self.gen.deliverable.clone())
    }

    // A path that restates the deliverable directory's own name means the path under
    // the root; the note tells the model so it learns the convention.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    fn unroot_rel(&self, rel: &str) -> (String, Option<String>) {
        match crate::gen::unroot(&self.gen.deliverable, rel) {
            Some(under) => {
                let note = crate::gen::unroot_note(rel, &under);
                (under, Some(note))
            }
            None => (rel.to_string(), None),
        }
    }

    // What one read_text_file reply holds at most: lines by default, bytes always.
    // Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
    pub(super) const READ_FILE_LINES: usize = 400;

    pub(super) const READ_FILE_BYTES: usize = 64 * 1024;

    // Whether a file tool path, as named or after its links resolve, is a protected
    // path: credentials, hidden paths, Git internals, compiler state, nested projects.
    fn protected_file(&self, rel: &str, path: &std::path::Path) -> bool {
        let base = self.deliverable_root();
        let open = |relative: &str| {
            crate::task_runtime::safe_path(relative)
                && crate::document_sets::allowed_components(relative)
                && !crate::task_runtime::protected(relative)
                && crate::document_sets::contained_relative(&base, relative)
        };
        let resolved = path
            .strip_prefix(&base)
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        !open(rel.trim().trim_start_matches("./")) || !open(&resolved)
    }

    pub(super) fn file_tool(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        match name {
            "read_text_file" => {
                let (rel, note) = self.unroot_rel(&Self::str_arg(args, "path")?);
                let path = self.deliverable_path(&rel)?;
                // Listing and task snapshots exclude credentials and internal state.
                // A guessed path, including an alias through a symlink, keeps that fence.
                if self.protected_file(&rel, &path) {
                    return Err(ToolError::new("protected-path", format!(
                        "`{rel}` is a protected path; file reads exclude credentials, hidden paths, Git internals, private compiler state and nested projects"
                    )));
                }
                let text = crate::vfs::read_to_string(&path).map_err(|e| {
                    ToolError::new(
                        "not-found",
                        format!(include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-files/cannot-read-list-files-shows-what-exists-37a1f.md")), rel, e),
                    )
                })?;
                let start = args["line"]
                    .as_u64()
                    .map(|l| (l as usize).saturating_sub(1))
                    .unwrap_or(0);
                // Bounded by lines and by bytes: what the session reads is resent
                // on every later round. Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
                let limit = args["limit"]
                    .as_u64()
                    .map(|l| l as usize)
                    .unwrap_or(Self::READ_FILE_LINES);
                let total = text.split_inclusive('\n').count();
                let mut content = String::new();
                let mut taken = 0usize;
                let mut truncated = false;
                let mut truncated_line = None;
                for line in text.split_inclusive('\n').skip(start).take(limit) {
                    if content.len() + line.len() > Self::READ_FILE_BYTES {
                        truncated = true;
                        if taken == 0 {
                            let mut end = Self::READ_FILE_BYTES.min(line.len());
                            while !line.is_char_boundary(end) {
                                end -= 1;
                            }
                            content.push_str(&line[..end]);
                            truncated_line = Some(start + 1);
                            taken = 1;
                        }
                        break;
                    }
                    content.push_str(line);
                    taken += 1;
                }
                let more_lines = start + taken < total;
                truncated |= more_lines;
                let hash = crate::task_runtime::digest(text.as_bytes());
                self.file_hashes.insert(rel.clone(), hash.clone());
                let mut reply = json!({"content": content, "totalLines": total, "hash": hash});
                let ledger = crate::gen::Ledger::load(&self.snapshot.out);
                if let Some(owner) = crate::gen::file_owner(&self.snapshot, &ledger, "", &rel) {
                    let entity = format!("ent:{owner}");
                    reply["owner"] = json!(entity);
                    let needs_claim = crate::gen::file_owner(
                        &self.snapshot,
                        &ledger,
                        &self.scope.gen_target(),
                        &rel,
                    )
                    .is_some();
                    reply["requires_generation"] = json!(needs_claim);
                    if needs_claim {
                        reply["begin_generation"] = json!({"entity":entity});
                        reply["repair"] = json!(include_str!(concat!(
                            env!("OUT_DIR"),
                            "/payloads/goals/prompts/inline/tools-files/call-begin-generation-for-this-owner-before-a6647.md"
                        )));
                    }
                }

                if truncated {
                    reply["truncated"] = json!(true);
                }
                if more_lines {
                    reply["nextLine"] = json!(start + taken + 1);
                }
                if let Some(line) = truncated_line {
                    reply["truncatedLine"] = json!(line);
                    reply["notice"] = json!(format!("Line {line} exceeds the 64 KiB response limit. Only a UTF-8-safe prefix is returned; the rest of that line is omitted."));
                }
                if let Some(n) = note {
                    reply["note"] = json!(n);
                }
                Ok(reply)
            }
            "write_text_file" => {
                let raw = Self::str_arg(args, "path")?;
                let (rel, note) = self.unroot_rel(raw.trim().trim_start_matches("./"));
                let content = args["content"]
                    .as_str()
                    .ok_or_else(|| ToolError::new("bad-args", include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-files/content-must-be-a-string-c4b52.md")).into()))?
                    .to_owned();
                if !self.gen.allows_source(&rel) {
                    return Err(ToolError::new("source-selection",format!("{} is outside this document set's source selection or belongs to documents. Allowed source patterns: {:?}. Inspect list_document_sets; choose an included path or use update_document_set to include the intended source or criteria file. Do not substitute an unrelated file after a rejected write",rel,self.gen.code)));
                }
                let path = self.deliverable_path(&rel)?;
                let lexical = self.deliverable_root().join(&rel);
                if lexical
                    .ancestors()
                    .take_while(|p| *p != self.deliverable_root())
                    .any(crate::vfs::is_symlink)
                {
                    return Err(ToolError::new(
                        "asset-link-write",
                        "remove the link before writing; text writes cannot modify its target"
                            .into(),
                    ));
                }
                // The protected-path fence holds with or without a workspace.
                // Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
                if self.protected_file(&rel, &path) {
                    return Err(ToolError::new("protected-path", format!(
                        "`{rel}` is a protected path; file writes exclude credentials, hidden paths, Git internals, private compiler state and nested projects"
                    )));
                }
                // File ownership: a path recorded for an entity outside this session's
                // component group is off limits; the group shares its files.
                // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
                let ledger = crate::gen::Ledger::load(&self.snapshot.out);
                if let Some(owner) =
                    crate::gen::file_owner(&self.snapshot, &ledger, &self.scope.gen_target(), &rel)
                {
                    if self.scope.gen_target().is_empty() {
                        return Err(ToolError::new(
                            "file-owned",
                            format!(
                                include_str!(concat!(
                                    env!("OUT_DIR"),
                                    "/payloads/goals/prompts/inline/tools-files/rel-is-recorded-for-entity-ent-owner-41ef5.md"
                                )),
                                rel = rel,
                                owner = owner
                            ),
                        ));
                    }
                    return Err(ToolError::new(
                        "file-owned",
                        format!(
                            include_str!(concat!(
                                env!("OUT_DIR"),
                                "/payloads/goals/prompts/inline/tools-files/belongs-to-entity-outside-your-component-group-8fc74.md"
                            )),
                            rel, owner
                        ),
                    ));
                }
                // Literal edits reuse the same Source and ownership gates as full
                // writes. Check the complete file before committing any bytes.
                let old_text = args
                    .get("old_text")
                    .map(|value| {
                        value.as_str().map(str::to_owned).ok_or_else(|| {
                            ToolError::new("bad-args", include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-files/old-text-must-be-a-string-bb5cb.md")).into())
                        })
                    })
                    .transpose()?;
                let mut expected = args
                    .get("expectedHash")
                    .map(|_| Self::str_arg(args, "expectedHash"))
                    .transpose()?;
                let append = args["append"].as_bool() == Some(true);
                let replace_all = args["replaceAll"].as_bool() == Some(true);
                if old_text.is_some() && append {
                    return Err(ToolError::new(
                        "invalid-edit",
                        include_str!(concat!(
                            env!("OUT_DIR"),
                            "/payloads/goals/prompts/inline/tools-files/old-text-requires-expectedhash-from-the-last-03189.md"
                        ))
                        .into(),
                    ));
                }
                // A literal edit applies to the version this serving last returned
                // for the path; with none, it is refused rather than applied blind.
                // Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
                if old_text.is_some() && expected.is_none() {
                    expected = Some(self.file_hashes.get(&rel).cloned().ok_or_else(|| {
                        ToolError::new(
                            "edit-needs-read",
                            format!(
                                include_str!(concat!(
                                    env!("OUT_DIR"),
                                    "/payloads/goals/prompts/inline/tools-files/edit-needs-a-read-first-5c1e2.md"
                                )),
                                rel = rel
                            ),
                        )
                    })?);
                }
                let current = if old_text.is_some()
                    || expected.is_some()
                    || (append && path.vis_file())
                {
                    Some(crate::vfs::read_to_string(&path).map_err(|e| {
                            ToolError::new(
                                "not-found",
                                format!(
                                    include_str!(concat!(
                                    env!("OUT_DIR"),
                                    "/payloads/goals/prompts/inline/tools-files/cannot-read-rel-before-editing-e-no-f958a.md"
                                )),
                                    rel = rel,
                                    e = e
                                ),
                            )
                        })?)
                } else {
                    None
                };
                if let Some(expected) = expected {
                    let actual = crate::task_runtime::digest(
                        current.as_deref().unwrap_or_default().as_bytes(),
                    );
                    if expected != actual {
                        return Err(ToolError::new(
                            "file-conflict",
                            format!(
                                include_str!(concat!(
                                    env!("OUT_DIR"),
                                    "/payloads/goals/prompts/inline/tools-files/rel-changed-since-the-supplied-expectedhash-current-f3116.md"
                                )),
                                rel = rel,
                                actual = actual
                            ),
                        ));
                    }
                }
                let mut replaced = 0usize;
                let next = if let Some(ref old) = old_text {
                    let text = current.as_deref().unwrap_or_default();
                    let (next, count) = literal_edit(text, old, &content, replace_all)
                        .map_err(|found| edit_match_error(&rel, &found))?;
                    replaced = count;
                    next
                } else if append {
                    format!("{}{}", current.as_deref().unwrap_or_default(), content)
                } else {
                    content.clone()
                };
                crate::store::write_atomic(&path, next.as_bytes()).map_err(|e| {
                    ToolError::new("io-error", format!(include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-files/cannot-write-rel-e-fcde7.md")), rel = rel, e = e))
                })?;
                let hash = crate::task_runtime::digest(next.as_bytes());
                self.file_hashes.insert(rel.clone(), hash.clone());
                let mut reply = json!({"written": rel, "bytes": content.len(), "appended": append && current.is_some(), "fileBytes": next.len(), "hash": hash});
                if old_text.is_some() {
                    reply["replaced"] = json!(replaced);
                }
                if let Some(n) = note {
                    reply["note"] = json!(n);
                }
                Ok(reply)
            }
            "list_files" => {
                let (rel, note) = self.unroot_rel(&Self::opt_str(args, "path").unwrap_or_default());
                let filter = Self::opt_str(args, "pattern")
                    .map(|p| path_glob(&p))
                    .transpose()?;
                let root = if rel.is_empty() {
                    self.deliverable_root()
                } else {
                    self.deliverable_path(&rel)?
                };
                // Pages follow the deterministic walk, after the glob filter.
                // Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
                let offset = args["offset"].as_u64().unwrap_or(0) as usize;
                let limit = (args["limit"].as_u64().unwrap_or(LIST_FILES_LIMIT as u64) as usize)
                    .clamp(1, LIST_FILES_LIMIT);
                let mut out: Vec<String> = Vec::new();
                let mut eligible = 0usize;
                let mut truncated = false;
                self.walk_files(&root, &mut |_, relative| {
                    if filter.as_ref().is_some_and(|f| !f.is_match(relative)) {
                        return true;
                    }
                    eligible += 1;
                    if eligible <= offset {
                        return true;
                    }
                    if out.len() >= limit {
                        truncated = true;
                        return false;
                    }
                    out.push(relative.to_string());
                    true
                });
                let shown = out.len();
                out.sort();
                let mut reply = json!({"files": out});
                if truncated {
                    reply["truncated"] = json!(true);
                    reply["nextOffset"] = json!(offset + shown);
                }
                if let Some(n) = note {
                    reply["note"] = json!(n);
                }
                Ok(reply)
            }
            "search_files" => self.search_files(args),
            _ => unreachable!(),
        }
    }
}

impl ToolSession {
    // Every eligible file under `root`, breadth first in name order, each directory
    // once after its links resolve; `visit` gets the path and its deliverable-relative
    // name and answers whether to continue. A link pointing out of the deliverable is
    // not walked: what cannot be read is not offered.
    // Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
    fn walk_files(
        &self,
        root: &std::path::Path,
        visit: &mut dyn FnMut(&std::path::Path, &str) -> bool,
    ) {
        let base = self.deliverable_root();
        if root.vis_file() {
            if let Ok(relative) = root.strip_prefix(&base) {
                let relative = relative.to_string_lossy().replace('\\', "/");
                if crate::document_sets::safe_relative(&base, &relative) {
                    visit(root, &relative);
                }
            }
            return;
        }
        let mut seen: BTreeSet<std::path::PathBuf> = BTreeSet::new();
        if let Ok(c) = root.vcanonicalize() {
            seen.insert(c);
        }
        let mut queue = std::collections::VecDeque::from([root.to_path_buf()]);
        while let Some(dir) = queue.pop_front() {
            let Ok(entries) = crate::vfs::read_dir(&dir) else {
                continue;
            };
            let mut paths: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                let Ok(relative) = p.strip_prefix(&base) else {
                    continue;
                };
                let relative = relative.to_string_lossy().replace('\\', "/");
                if !crate::document_sets::safe_relative(&base, &relative) {
                    continue;
                }
                let canonical = p.vcanonicalize();
                if canonical.as_ref().is_ok_and(|c| !c.starts_with(&base)) {
                    continue;
                }
                if p.vis_dir() {
                    // A directory reached again through a link is a loop.
                    if let Ok(c) = canonical {
                        if !seen.insert(c) {
                            continue;
                        }
                    }
                    queue.push_back(p);
                } else if !visit(&p, &relative) {
                    return;
                }
            }
        }
    }

    // A content search over the eligible files, in process over the workspace's
    // filesystem. Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
    fn search_files(&mut self, args: &Value) -> Result<Value, ToolError> {
        let pattern = Self::str_arg(args, "pattern")?;
        let source = if args["literal"].as_bool() == Some(true) {
            regex::escape(&pattern)
        } else {
            pattern.clone()
        };
        let regex = regex::RegexBuilder::new(&source)
            .case_insensitive(args["ignoreCase"].as_bool() == Some(true))
            .build()
            .map_err(|e| {
                ToolError::new(
                    "bad-pattern",
                    format!("pattern is not a valid regular expression: {e}. Pass literal: true to search plain text"),
                )
            })?;
        let filter = Self::opt_str(args, "glob")
            .map(|g| path_glob(&g))
            .transpose()?;
        let (rel, note) = self.unroot_rel(&Self::opt_str(args, "path").unwrap_or_default());
        let root = if rel.is_empty() {
            self.deliverable_root()
        } else {
            let path = self.deliverable_path(&rel)?;
            if !path.vexists() {
                return Err(ToolError::new(
                    "not-found",
                    format!("`{rel}` does not exist; list_files shows what exists"),
                ));
            }
            path
        };
        let context = (args["context"].as_u64().unwrap_or(0) as usize).min(SEARCH_FILES_CONTEXT);
        let files_only = args["filesOnly"].as_bool() == Some(true);
        // The resumable cursor: a file in sorted path order and its first line to
        // search. Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
        let cursor_path = args["cursor"]["path"].as_str().map(str::to_owned);
        let cursor_line = args["cursor"]["line"].as_u64().unwrap_or(1).max(1) as usize;
        let mut files = Vec::new();
        self.walk_files(&root, &mut |path, relative| {
            if filter.as_ref().is_none_or(|f| f.is_match(relative))
                && cursor_path.as_deref().is_none_or(|c| relative >= c)
            {
                files.push((path.to_path_buf(), relative.to_string()));
            }
            true
        });
        files.sort_by(|a, b| a.1.cmp(&b.1));
        let mut searched = 0usize;
        let mut skipped = 0usize;
        let mut bytes = 0usize;
        let mut out: Vec<Value> = Vec::new();
        let mut read_total = 0usize;
        let mut next: Option<(String, usize)> = None;
        'files: for (path, relative) in files {
            if self.protected_file(&relative, &path) {
                continue;
            }
            let first = if cursor_path.as_deref() == Some(relative.as_str()) {
                cursor_line
            } else {
                1
            };
            // Size before bytes, and a scan budget per call: a stopped scan
            // returns its cursor, so it never reads as "no match".
            let size = crate::vfs::entry_metadata(&path).map(|m| m.len() as usize);
            if size.as_ref().is_ok_and(|n| *n > SEARCH_FILES_FILE_BYTES) {
                skipped += 1;
                continue;
            }
            if searched > 0 && read_total + size.unwrap_or(0) > SEARCH_FILES_SCAN_BYTES {
                next = Some((relative, first));
                break;
            }
            let text = match crate::vfs::read(&path) {
                Ok(raw) if raw.len() <= SEARCH_FILES_FILE_BYTES && !raw.contains(&0) => {
                    match String::from_utf8(raw) {
                        Ok(text) => text,
                        Err(_) => {
                            skipped += 1;
                            continue;
                        }
                    }
                }
                Ok(_) => {
                    skipped += 1;
                    continue;
                }
                Err(_) => continue,
            };
            searched += 1;
            read_total += text.len();
            let lines: Vec<&str> = text.lines().collect();
            let hits: Vec<usize> = lines
                .iter()
                .enumerate()
                .skip(first - 1)
                .filter(|(_, line)| regex.is_match(line))
                .map(|(i, _)| i)
                .collect();
            if hits.is_empty() {
                continue;
            }
            let entries: Vec<(usize, Value)> = if files_only {
                vec![(first - 1, json!({"path": relative, "count": hits.len()}))]
            } else {
                hits.iter()
                    .map(|&i| {
                        let column = regex
                            .find(lines[i])
                            .map_or(1, |m| lines[i][..m.start()].chars().count() + 1);
                        let mut entry = json!({"path": relative, "line": i + 1, "column": column, "text": excerpt(lines[i], &regex)});
                        if context > 0 {
                            let before: Vec<String> = lines[i.saturating_sub(context)..i]
                                .iter()
                                .map(|l| cut_line(l))
                                .collect();
                            let after: Vec<String> = lines[(i + 1).min(lines.len())..(i + 1 + context).min(lines.len())]
                                .iter()
                                .map(|l| cut_line(l))
                                .collect();
                            if !before.is_empty() {
                                entry["before"] = json!(before);
                            }
                            if !after.is_empty() {
                                entry["after"] = json!(after);
                            }
                        }
                        (i, entry)
                    })
                    .collect()
            };
            for (i, entry) in entries {
                let size = entry.to_string().len();
                if out.len() >= SEARCH_FILES_MATCHES || bytes + size > SEARCH_FILES_BYTES {
                    next = Some((relative, i + 1));
                    break 'files;
                }
                bytes += size;
                out.push(entry);
            }
        }
        let mut reply = json!({"matches": out, "files": searched, "scanComplete": next.is_none()});
        if skipped > 0 {
            reply["skipped"] = json!(skipped);
        }
        if let Some((path, line)) = next {
            reply["truncated"] = json!(true);
            reply["next"] = json!({"path": path, "line": line});
            reply["notice"] = json!(
                "Not every file was searched. Pass next unchanged as cursor, with the same pattern, path and glob, to continue."
            );
        }
        if let Some(n) = note {
            reply["note"] = json!(n);
        }
        Ok(reply)
    }
}

// One quoted line of a search reply, cut on a character boundary.
// A matching line, or for a long line an excerpt that starts shortly before the
// first match so the match itself is always quoted.
fn excerpt(line: &str, regex: &regex::Regex) -> String {
    if line.chars().count() <= SEARCH_FILES_LINE_CHARS {
        return line.to_string();
    }
    let at = regex.find(line).map_or(0, |m| m.start());
    let lead = line[..at]
        .chars()
        .count()
        .saturating_sub(SEARCH_FILES_LINE_CHARS / 3);
    let start = line.char_indices().nth(lead).map_or(0, |(i, _)| i);
    let body = cut_line(&line[start..]);
    if start > 0 {
        format!("…{body}")
    } else {
        body
    }
}

fn cut_line(line: &str) -> String {
    match line.char_indices().nth(SEARCH_FILES_LINE_CHARS) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line.to_string(),
    }
}

// A glob over deliverable-relative paths: `*` stays within one directory, `**`
// crosses them, and a pattern without `/` matches file names at any depth.
// Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
pub(crate) fn path_glob(pattern: &str) -> Result<globset::GlobMatcher, ToolError> {
    let pattern = pattern.trim().trim_start_matches("./");
    let pattern = if pattern.contains('/') {
        pattern.to_string()
    } else {
        format!("**/{pattern}")
    };
    globset::GlobBuilder::new(&pattern)
        .literal_separator(true)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|e| ToolError::new("bad-pattern", format!("invalid glob `{pattern}`: {e}")))
}

// Replace `old` in `text` with `new`: exactly one occurrence, or every occurrence
// with `all`. Answers the next text and the replacement count, or a description of
// why `old` selected no single site.
// Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
pub(crate) fn literal_edit(
    text: &str,
    old: &str,
    new: &str,
    all: bool,
) -> Result<(String, usize), String> {
    if old.is_empty() {
        return Err("old_text is empty".into());
    }
    let count = text.matches(old).count();
    match count {
        0 => Err(match near_match_line(text, old) {
            Some(line) => format!(
                "no exact match, but ignoring whitespace it matches starting at line {line}; copy the exact whitespace and line endings from a fresh read"
            ),
            None => "no match".into(),
        }),
        1 => Ok((text.replacen(old, new, 1), 1)),
        n if all => Ok((text.replace(old, new), n)),
        n => Err(format!("{n} matches")),
    }
}

pub(crate) fn edit_match_error(rel: &str, found: &str) -> ToolError {
    ToolError::new(
        "edit-match",
        format!(
            include_str!(concat!(
                env!("OUT_DIR"),
                "/payloads/goals/prompts/inline/tools-files/old-text-must-be-nonempty-and-match-2ba0a.md"
            )),
            rel = rel,
            found = found
        ),
    )
}

// The 1-based line where `old` matches `text` once runs of whitespace are
// collapsed on both sides, when it does.
fn near_match_line(text: &str, old: &str) -> Option<usize> {
    fn collapse(s: &str) -> (String, Vec<usize>) {
        let mut out = String::new();
        let mut offsets = Vec::new();
        let mut space = false;
        for (i, c) in s.char_indices() {
            if c.is_whitespace() {
                if !space {
                    out.push(' ');
                    offsets.push(i);
                }
                space = true;
            } else {
                out.push(c);
                offsets.extend(std::iter::repeat_n(i, c.len_utf8()));
                space = false;
            }
        }
        (out, offsets)
    }
    let (needle, _) = collapse(old.trim());
    if needle.trim().is_empty() {
        return None;
    }
    let (hay, offsets) = collapse(text);
    let at = hay.find(&needle)?;
    let origin = *offsets.get(at)?;
    Some(text[..origin].matches('\n').count() + 1)
}

impl ToolSession {
    pub(super) fn asset_link_tool(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        let project = crate::input::project_for_store(&self.snapshot).ok_or_else(|| {
            ToolError::new("no-project", "cannot resolve the document set".into())
        })?;
        let path = Self::str_arg(args, "path")?;
        let fail = |message| ToolError::new("asset-link", message);
        if !crate::task_runtime::safe_path(&path)
            || !crate::document_sets::safe_relative(&project.root, &path)
        {
            return Err(fail("path must stay inside the eligible project".into()));
        }
        let destination = project.root.join(&path);
        if name == "inspect_asset_link" {
            let (relative, target) =
                crate::asset_links::target(&project.root, &path).map_err(fail)?;
            let bytes = crate::vfs::read(&destination).map_err(|e| fail(e.to_string()))?;
            return Ok(
                json!({"path":path,"target":relative,"resolvedTarget":target,"sha256":crate::task_runtime::digest(&bytes),"status":"available"}),
            );
        }
        let relative = destination
            .strip_prefix(&self.gen.deliverable)
            .map_err(|_| fail("destination is outside the deliverable".into()))?
            .to_string_lossy()
            .replace('\\', "/");
        if !self.gen.allows_source(&relative) {
            return Err(ToolError::new(
                "source-selection",
                "link destination must be selected Source".into(),
            ));
        }
        let _lock = crate::gen::Ledger::lock(&self.snapshot.out);
        let ledger = crate::gen::Ledger::load(&self.snapshot.out);
        if let Some(error) = &ledger.unreadable {
            return Err(fail(error.clone()));
        }
        if let Some(owner) =
            crate::gen::file_owner(&self.snapshot, &ledger, &self.scope.gen_target(), &relative)
        {
            return Err(ToolError::new(
                "file-owned",
                format!("begin_generation for ent:{owner} before modifying its link"),
            ));
        }
        if name == "remove_asset_link" {
            let expected = Self::str_arg(args, "expectedTarget")?;
            let current = crate::vfs::read_link(&destination).map_err(|e| fail(e.to_string()))?;
            if current.to_str() != Some(expected.as_str()) {
                return Err(fail("link target changed since inspection".into()));
            }
            crate::vfs::remove_file(&destination).map_err(|e| fail(e.to_string()))?;
            return Ok(json!({"removed":path,"targetUnchanged":true}));
        }
        let target = Self::str_arg(args, "target")?;
        let target_path = project.root.join(&target);
        if !crate::asset_links::selected_asset(&project, &target)
            || crate::vfs::is_symlink(&target_path)
            || !target_path.vis_file()
        {
            return Err(fail(
                "target must be a direct, available selected asset in this set".into(),
            ));
        }
        if destination.vexists() || crate::vfs::is_symlink(&destination) {
            return Err(fail(
                "destination exists; inspect and remove the link or choose a new path".into(),
            ));
        }
        if destination
            .parent()
            .into_iter()
            .flat_map(|p| p.ancestors())
            .take_while(|p| *p != project.root)
            .any(crate::vfs::is_symlink)
        {
            return Err(fail("link destination cannot have a symlink parent".into()));
        }
        let bytes = crate::vfs::read(&target_path).map_err(|e| fail(e.to_string()))?;
        let sha = crate::task_runtime::digest(&bytes);
        if sha != Self::str_arg(args, "sha256")? {
            return Err(fail("target bytes changed since inspection".into()));
        }
        let link = crate::asset_links::relative_target(&path, &target);
        crate::vfs::create_dir_all(
            destination
                .parent()
                .ok_or_else(|| fail("destination has no parent".into()))?,
        )
        .map_err(|e| fail(e.to_string()))?;
        crate::vfs::symlink_file(&link, &destination).map_err(|e| fail(e.to_string()))?;
        Ok(json!({"path":path,"target":link,"resolvedTarget":target,"sha256":sha}))
    }
}
