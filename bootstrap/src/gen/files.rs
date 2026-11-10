use super::*;

// Where a test artifact lives on disk: llm criteria are metadata (under <out>/gen/),
// programmatic artifacts are part of the deliverable.
pub fn artifact_path(out: &Path, gs: &GenSettings, test: &TestRef) -> PathBuf {
    if test.kind == "llm" {
        // Criteria live in the out directory when the built-in worker wrote them; an
        // external worker writes them where it writes everything else, under the
        // deliverable. The recorded path resolves against both, out first, so neither
        // home reads as a gone artifact.
        let in_out = out.join("gen").join(&test.artifact);
        if in_out.vexists() {
            return in_out;
        }
        let in_deliverable = gs.deliverable.join(&test.artifact);
        if in_deliverable.vexists() {
            return in_deliverable;
        }
        in_out
    } else {
        gs.deliverable.join(&test.artifact)
    }
}

// A criteria file is executable judgment setup, not an arbitrary existing file.
pub(crate) fn criteria_front_matter(rid: &str, criterion: Option<&str>, statement: &str) -> String {
    let mut fields = serde_json::Map::new();
    fields.insert("requirement".into(), json!(rid));
    fields.insert("hash".into(), json!(hash_hex(statement)));
    if let Some(cid) = criterion {
        fields.insert("criterion".into(), json!(cid));
    }
    format!(
        "---\n{}---\n",
        serde_norway::to_string(&fields).expect("criteria metadata serializes")
    )
}

pub(crate) fn validate_criteria_file(
    store: &Store,
    rid: &str,
    criterion: Option<&str>,
    path: &Path,
) -> Result<(), String> {
    use pulldown_cmark::{Event, MetadataBlockKind, Options, Parser, Tag, TagEnd};
    #[derive(Deserialize)]
    struct Header {
        requirement: String,
        hash: String,
        #[serde(default)]
        criterion: Option<String>,
    }
    let check = || -> Result<(), String> {
        if !path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e, "md" | "markdown"))
        {
            return Err("use a separate Markdown criteria file, not executable test source".into());
        }
        let content = crate::vfs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut parser = Parser::new_ext(&content, Options::ENABLE_YAML_STYLE_METADATA_BLOCKS)
            .into_offset_iter();
        if !matches!(
            parser.next(),
            Some((
                Event::Start(Tag::MetadataBlock(MetadataBlockKind::YamlStyle)),
                _
            ))
        ) {
            return Err("YAML front matter is required".into());
        }
        let mut yaml = String::new();
        let mut end = None;
        for (event, range) in parser {
            match event {
                Event::Text(text) => yaml.push_str(&text),
                Event::End(TagEnd::MetadataBlock(_)) => {
                    end = Some(range.end);
                    break;
                }
                _ => {}
            }
        }
        let header: Header =
            serde_norway::from_str(&yaml).map_err(|e| format!("invalid criteria metadata: {e}"))?;
        let local_id = |id: &str| -> Result<String, String> {
            let (set, local) = crate::webapi::sets::unqualify(id);
            if set
                .as_deref()
                .is_some_and(|set| Some(set) != store.out.file_name().and_then(|s| s.to_str()))
            {
                return Err(
                    "criteria IDs belong to a different document set; copy criteriaFrontMatter"
                        .into(),
                );
            }
            Ok(store.resolve_id(&local).to_string())
        };
        if local_id(&header.requirement)? != rid {
            return Err(format!("requirement must be {rid}"));
        }
        let cid = header.criterion.as_deref().map(local_id).transpose()?;
        if let Some(c) = criterion.filter(|c| cid.as_deref() != Some(*c)) {
            return Err(format!("criterion must be {}", c));
        }
        let statement = if let Some(cid) = cid.as_deref() {
            &store
                .graph
                .criteria
                .get(cid)
                .filter(|c| c.requirement == rid)
                .ok_or("criterion is not owned by this requirement")?
                .statement
        } else {
            &store
                .graph
                .requirements
                .get(rid)
                .ok_or("unknown requirement")?
                .statement
        };
        if header.hash != hash_hex(statement) {
            return Err("hash must match the current statement; copy criteriaFrontMatter".into());
        }
        let body = content
            .get(end.ok_or("unclosed criteria front matter")?..)
            .unwrap_or_default()
            .trim();
        let words: Vec<_> = body.split(|c: char| !c.is_alphabetic()).collect();
        if body.is_empty()
            || !["PASS", "FAIL"]
                .iter()
                .all(|expected| words.iter().any(|word| word.eq_ignore_ascii_case(expected)))
        {
            return Err(
                "the body must contain inspection steps and explicit PASS and FAIL outcomes".into(),
            );
        }
        Ok(())
    };
    check().map_err(|e| format!("invalid llm criteria artifact `{}`: {e}. Write a valid criteria file before recording; never substitute a programmatic test file", path.display()))
}

pub fn hash_file(path: &Path) -> String {
    crate::vfs::read(path)
        .map(|b| hash_hex(&String::from_utf8_lossy(&b)))
        .unwrap_or_default()
}

// Hash over a row's manifest files, sorted, concatenated. Deliverable-relative paths.
pub fn hash_files(gs: &GenSettings, files: &[String]) -> String {
    let mut sorted: Vec<&String> = files.iter().collect();
    sorted.sort();
    let mut acc = String::new();
    for f in sorted {
        acc.push_str(f);
        acc.push('|');
        acc.push_str(&hash_file(&gs.deliverable.join(f)));
        acc.push('|');
    }
    hash_hex(&acc)
}

// Output is never implementation: a built medium's artifact and every path the build
// produces are rebuilt on every run (a deck's zip carries fresh timestamps), so a row
// listing them would read stale-code after each run and its verify goal would never
// close. Mirrors docs/consumers/gen.md#status-is-derived-never-stored.
pub fn build_outputs(ledger: &Ledger) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    if let Some(m) = &ledger.medium {
        if m.produced == "built" && !m.artifact.trim().is_empty() {
            out.insert(m.artifact.trim().trim_start_matches("./").to_string());
        }
    }
    if let Some(b) = &ledger.build {
        for p in &b.produces {
            out.insert(p.trim().trim_start_matches("./").to_string());
        }
    }
    out
}

// The files of a list that count as implementation: the build outputs set aside.
pub fn implementing_files(ledger: &Ledger, files: &[String]) -> Vec<String> {
    let outputs = build_outputs(ledger);
    files
        .iter()
        .filter(|f| !outputs.contains(f.trim().trim_start_matches("./")))
        .cloned()
        .collect()
}

// The files hash a row's stale-code check compares: implementing files alone.
pub fn hash_implementing(out: &Path, gs: &GenSettings, files: &[String]) -> String {
    hash_files(gs, &implementing_files(&Ledger::load(out), files))
}

// Scope is independent of byte freshness. Test artifacts and build outputs never
// count as implementation, including legacy manifests that named them in files.
pub(crate) fn implementation_outside_source(
    gs: &GenSettings,
    ledger: &Ledger,
    files: &[String],
) -> bool {
    let artifacts = test_artifacts(ledger);
    implementing_files(ledger, files)
        .iter()
        .any(|file| !artifacts.contains(file) && !gs.allows_source(file))
}

// A recorded binding must still describe this set's current deliverable. Internal
// LLM criteria remain metadata; external criteria and programmatic tests are Source.
pub(crate) fn binding_source_current(
    out: &Path,
    gs: &GenSettings,
    ledger: &Ledger,
    row: &ReqRow,
) -> bool {
    !implementation_outside_source(gs, ledger, &row.files)
        && std::iter::once(&row.test)
            .chain(row.tests.values().map(|test| &test.test))
            .all(|test| {
                (test.kind == "llm" && artifact_path(out, gs, test).starts_with(out.join("gen")))
                    || gs.allows_source(&test.artifact)
            })
}

// A deliverable-relative path as the ledger records it: forward slashes, no leading
// `./`, no empty or `.` segments. An absolute path, or one that climbs out with
// `..`, is rejected naming it: nothing the ledger records, strips, hashes, or
// removes may reach outside the deliverable.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
pub fn confine_rel(path: &str) -> Result<String, String> {
    let raw = path.trim().replace('\\', "/");
    let drive = raw.as_bytes().get(1) == Some(&b':')
        && raw
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false);
    if raw.starts_with('/') || drive {
        return Err(format!(
            "`{}` is an absolute path; every recorded path is relative to the deliverable directory",
            path.trim()
        ));
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in raw.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                return Err(format!(
                "`{}` climbs out of the deliverable with `..`; every recorded path stays under it",
                path.trim()
            ))
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return Err("a path is required, and `.` names the deliverable itself, not a file".into());
    }
    Ok(parts.join("/"))
}

// A path that restates the deliverable directory's own name (`handbook/hiring.md`
// when the deliverable is `handbook`) while the deliverable holds no directory of
// that name: the path under the root it means (`hiring.md`; empty for the name
// alone). None when the path keeps as given.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
pub fn unroot(deliverable: &Path, rel: &str) -> Option<String> {
    let name = deliverable
        .vcanonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .or_else(|| {
            deliverable
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        })?;
    if name.is_empty() {
        return None;
    }
    let rel = rel.trim().replace('\\', "/");
    let rel = rel.trim_start_matches("./");
    let rest = if rel == name || rel == format!("{}/", name) {
        ""
    } else {
        rel.strip_prefix(&format!("{}/", name))?
    };
    if deliverable.join(&name).vis_dir() {
        return None;
    }
    Some(rest.trim_start_matches('/').to_string())
}

// The reply's note for a path `unroot` moved under the root.
pub fn unroot_note(from: &str, to: &str) -> String {
    format!(
        "`{}` restates the deliverable directory's own name; paths are relative to the deliverable, so this is `{}`. Write paths without that prefix",
        from.trim(),
        if to.is_empty() { "." } else { to }
    )
}

// `confine_rel`, then `unroot`: the path the ledger records, with a restated root
// stripped and noted.
pub fn confine_in(gs: &GenSettings, path: &str, notes: &mut Vec<String>) -> Result<String, String> {
    let rel = confine_rel(path)?;
    let relative = match unroot(&gs.deliverable, &rel) {
        Some(under) if !under.is_empty() => {
            let note = unroot_note(&rel, &under);
            if !notes.contains(&note) {
                notes.push(note);
            }
            under
        }
        _ => rel,
    };
    if !gs.allows_source(&relative) {
        return Err(format!(
            "{} is outside this document set's source selection or belongs to documents",
            relative
        ));
    }
    Ok(relative)
}

// A working directory the same way; the restated root alone is the deliverable.
pub fn confine_cwd_in(
    gs: &GenSettings,
    cwd: &str,
    notes: &mut Vec<String>,
) -> Result<String, String> {
    let c = confine_cwd(cwd)?;
    if c == "." {
        return Ok(c);
    }
    match unroot(&gs.deliverable, &c) {
        Some(under) => {
            let note = unroot_note(&c, &under);
            if !notes.contains(&note) {
                notes.push(note);
            }
            Ok(if under.is_empty() { ".".into() } else { under })
        }
        None => Ok(c),
    }
}

// Every path a list names, confined, deduplicated in first-seen order.
pub(super) fn confine_list_in(
    gs: &GenSettings,
    paths: &[String],
    notes: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for p in paths {
        out.push(confine_in(gs, p, notes)?);
    }
    Ok(dedup_keep_order(out))
}

// Every test artifact the ledger's rows name, headline and criterion tests alike.
// They are the rows', never an entity's implementation.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
pub fn test_artifacts(ledger: &Ledger) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for row in ledger.requirements.values() {
        out.insert(row.test.artifact.clone());
        for t in row.tests.values() {
            out.insert(t.test.artifact.clone());
        }
    }
    out.remove("");
    out
}

// The entity outside `id`'s component group that records `path`, by slug. Members
// of one group share their files; everyone else owns theirs alone. `id` may be a
// requirement (a bind goal's target), which belongs to no group, so every recorded
// file is someone else's.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
pub fn file_owner(store: &Store, ledger: &Ledger, id: &str, path: &str) -> Option<String> {
    if ledger.support.iter().any(|support| support == path) {
        return None;
    }
    let id = store.resolve_id(id);
    let group = group_root(store, id);
    ledger
        .entities
        .iter()
        .find(|(slug, e)| {
            let other = format!("ent:{}", slug);
            let other = store.resolve_id(&other);
            other != id && group_root(store, other) != group && e.files.iter().any(|f| f == path)
        })
        .map(|(slug, _)| slug.clone())
}

// The other members of `id`'s component group, the entities whose files it shares.
// A system, and an entity outside every group, shares with nobody.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
pub fn shared_with(store: &Store, id: &str) -> Vec<String> {
    let id = store.resolve_id(id);
    if is_system(store, id) {
        return Vec::new();
    }
    group_members(store, &group_root(store, id))
        .into_iter()
        .filter(|m| m != id)
        .collect()
}

// A working directory the same way, where `.` (or nothing) means the deliverable.
pub fn confine_cwd(cwd: &str) -> Result<String, String> {
    let c = cwd.trim();
    if c.is_empty() || c == "." || c == "./" {
        return Ok(".".into());
    }
    confine_rel(c)
}

fn dedup_keep_order(files: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    files
        .into_iter()
        .filter(|f| seen.insert(f.clone()))
        .collect()
}

// Every file a reply carries, in order. A step asks for one, but a model that writes
// its module and the entry point in a single answer has written both, and folding the
// second into the first leaves a file that cannot parse.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.

pub fn parse_file_replies(reply: &str) -> Result<Vec<(String, String)>, String> {
    let (first_path, rest) = parse_file_reply(reply)?;
    let mut out = Vec::new();
    let mut path = first_path;
    let mut body: Vec<&str> = Vec::new();
    for line in rest.lines() {
        match line.trim_start().strip_prefix("FILE:") {
            Some(next) if !next.trim().is_empty() => {
                out.push((path, body.join("\n")));
                path = next.trim().to_string();
                body = Vec::new();
            }
            _ => body.push(line),
        }
    }
    out.push((path, body.join("\n")));
    // The reply's outer layer is the FILE line, so a fence the model wrapped the file
    // in survives that first strip. Each file's own body gets unwrapped here, or the
    // artifact starts with ```python and parses as nothing.
    for (_, body) in out.iter_mut() {
        *body = strip_fences(body);
    }
    // A path that escapes the deliverable is not a file this harness writes.
    out.retain(|(p, _)| !p.is_empty() && !p.starts_with('/') && !p.contains(".."));
    if out.is_empty() {
        return Err("no usable file path in the reply".into());
    }
    Ok(out)
}

pub fn parse_file_reply(reply: &str) -> Result<(String, String), String> {
    let reply = strip_fences(reply);
    // The contract is a FILE line first. A model that opens with a sentence or a
    // fence has still answered, so the line is taken where it stands; anything before
    // it is preamble, and the file starts after it.
    let reply = match reply
        .lines()
        .position(|l| l.trim_start().starts_with("FILE:"))
    {
        Some(0) | None => reply.clone(),
        Some(n) => reply.lines().skip(n).collect::<Vec<_>>().join("\n"),
    };
    let mut lines = reply.splitn(2, '\n');
    let first = lines.next().unwrap_or("").trim();
    let Some(path) = first.strip_prefix("FILE:") else {
        return Err(format!(
            "first line must be `FILE: <path>`, got `{}`",
            crate::llm::truncate(first, 80)
        ));
    };
    let path = path.trim();
    if path.is_empty() || path.starts_with('/') || path.contains("..") {
        return Err(format!("bad file path `{}`", path));
    }
    Ok((path.to_string(), lines.next().unwrap_or("").to_string()))
}

// Models wrap code in markdown fences despite instructions; strip one outer fence.
pub fn strip_fences(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        if let Some(end) = rest.rfind("```") {
            let body = &rest[..end];
            return body
                .split_once('\n')
                .map(|(_, b)| b)
                .unwrap_or(body)
                .to_string();
        }
    }
    t.to_string()
}
