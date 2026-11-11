// The node files: file ids, reading and loading the store, writing whole files,
// and saving the graph. Mirrors docs/compiler/graph.md#node-files.
use super::*;

pub(super) fn yaml_to<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    yaml_read(path).ok().flatten()
}

// A missing file reads as `None`; a file that exists but does not parse is an error
// naming it, never an empty value.
// Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
pub(super) fn yaml_read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let text = match crate::vfs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {}", path.display(), e)),
    };
    if text.trim().is_empty() {
        return Err(format!("{}: empty file", path.display()));
    }
    serde_norway::from_str(&text)
        .map(Some)
        .map_err(|e| format!("{}: {}", path.display(), e))
}

// A shard read at load: missing reads as empty, unparseable is recorded and reads as
// empty for this load only (the store refuses to write while `errors` is non-empty).
pub(super) fn shard<T: serde::de::DeserializeOwned + Default>(
    path: &Path,
    errors: &mut Vec<String>,
) -> T {
    match yaml_read::<T>(path) {
        Ok(Some(v)) => v,
        Ok(None) => T::default(),
        Err(e) => {
            errors.push(e);
            T::default()
        }
    }
}

// ---- node files ----
// One file per node under graph/<kind>/, named by its file id.
// Mirrors docs/compiler/graph.md#node-files.

// The node kinds a repository commits, as directory names under graph/.
pub const COMMITTED_KINDS: [&str; 6] = [
    "entities",
    "requirements",
    "criteria",
    "views",
    "diagnostics",
    "redirects",
];

// The derived kinds: local, recomputed at load when missing and on every commit.
pub const DERIVED_KINDS: [&str; 3] = ["default-views", "relationships", "state-machines"];

// The file id of a node id: `:` as `~`, `/` as `+`, lowercase ASCII letters, digits,
// `-`, `_`, `.` kept, every other byte (and a leading `.`) as `%XX` in uppercase hex.
// Reversible, and no two ids fold into one name on a case-insensitive file system.
pub fn file_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for (i, b) in id.bytes().enumerate() {
        match b {
            b':' => out.push('~'),
            b'/' => out.push('+'),
            b'.' if i == 0 => out.push_str("%2E"),
            b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// The node id a file id spells; None for a name `file_id` never writes.
pub fn id_from_file(stem: &str) -> Option<String> {
    let bytes = stem.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'~' => out.push(b':'),
            b'+' => out.push(b'/'),
            b'%' => {
                let hex = stem.get(i + 1..i + 3)?;
                if !hex
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c))
                {
                    return None;
                }
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            b @ (b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.') => out.push(b),
            _ => return None,
        }
        i += 1;
    }
    let id = String::from_utf8(out).ok()?;
    (!id.is_empty() && file_id(&id) == stem).then_some(id)
}

// The longest file id written whole; a longer one is shortened to a prefix, `%~`, and
// a hash, and its file carries the full id under `id`.
pub const FILE_ID_MAX: usize = 180;
const FILE_ID_PREFIX: usize = 160;

// 64-bit FNV-1a: fixed by the docs, so a name never changes between builds.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// The file stem of a node: its file id, or the shortened form past FILE_ID_MAX.
// The second value says whether the file must carry its id.
// Mirrors docs/compiler/graph.md#node-files.
pub fn node_stem(id: &str) -> (String, bool) {
    let full = file_id(id);
    if full.len() <= FILE_ID_MAX {
        return (full, false);
    }
    let b = full.as_bytes();
    let mut cut = FILE_ID_PREFIX;
    if b[cut - 1] == b'%' {
        cut -= 1;
    } else if b[cut - 2] == b'%' {
        cut -= 2;
    }
    (
        format!("{}%~{:016x}", &full[..cut], fnv1a64(id.as_bytes())),
        true,
    )
}

// The content of one redirect file.
#[derive(Clone, Serialize, Deserialize)]
struct RedirectFile {
    to: String,
}

// The id a shortened file carries, read from its content.
fn carried_id(path: &Path) -> Result<String, String> {
    let v: serde_norway::Value =
        yaml_read(path)?.ok_or_else(|| format!("{}: missing", path.display()))?;
    v.get("id")
        .and_then(|i| i.as_str())
        .map(String::from)
        .ok_or_else(|| format!("{}: a shortened file name carries no id", path.display()))
}

// The node files of one kind directory, `(id, path)` in id order. A plain name decodes
// to its id; a shortened one (`%~`) carries it inside and must match its own name. Any
// other name is an error naming the file. Temporary and hidden files are skipped.
fn node_files(dir: &Path, errors: &mut Vec<String>) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(rd) = crate::vfs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let Some(stem) = name.strip_suffix(".yaml") else {
            continue;
        };
        if let Some(id) = id_from_file(stem) {
            out.push((id, e.path()));
            continue;
        }
        if stem.contains("%~") {
            match carried_id(&e.path()) {
                Ok(id) if node_stem(&id).0 == stem => out.push((id, e.path())),
                Ok(id) => errors.push(format!(
                    "{}: carries id {} whose file name differs",
                    e.path().display(),
                    id
                )),
                Err(err) => errors.push(err),
            }
            continue;
        }
        errors.push(format!(
            "{}: the file name spells no node id",
            e.path().display()
        ));
    }
    out.sort();
    out
}

// Every node of one kind directory. Missing reads as empty; a file that does not parse
// is recorded in `errors` (the store then refuses to write). A file starts with the id
// it is named by; one whose `id` names another node is an error too.
// Mirrors docs/compiler/graph.md#node-files.
fn load_nodes<T: serde::de::DeserializeOwned>(
    dir: &Path,
    errors: &mut Vec<String>,
) -> BTreeMap<String, T> {
    let mut nodes = BTreeMap::new();
    for (id, path) in node_files(dir, errors) {
        let read = yaml_read::<serde_norway::Value>(&path).and_then(|v| {
            let Some(mut v) = v else {
                return Ok(None);
            };
            if let serde_norway::Value::Mapping(m) = &mut v {
                if let Some(carried) = m.remove("id") {
                    if carried.as_str() != Some(id.as_str()) {
                        return Err(format!(
                            "{}: carries id {} but its file name spells {}",
                            path.display(),
                            carried.as_str().unwrap_or("?"),
                            id
                        ));
                    }
                }
            }
            serde_norway::from_value::<T>(v)
                .map(Some)
                .map_err(|e| format!("{}: {}", path.display(), e))
        });
        match read {
            Ok(Some(v)) => {
                nodes.insert(id, v);
            }
            Ok(None) => {}
            Err(e) => errors.push(e),
        }
    }
    nodes
}

// The nodes of one kind directory as JSON values, for readers outside the store
// (replays seed from a snapshot's graph).
pub fn read_node_values(dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    let mut errors = Vec::new();
    load_nodes::<serde_json::Value>(dir, &mut errors)
        .into_iter()
        .collect()
}

// A node as its file holds it: `id` first, then its own fields, the generation markers
// taken out into `markers`. Mirrors docs/compiler/graph.md#node-files.
fn node_value<T: Serialize>(id: &str, node: &T, markers: &mut Markers) -> serde_norway::Value {
    let mut m = serde_norway::Mapping::new();
    m.insert("id".into(), id.into());
    if let Ok(serde_norway::Value::Mapping(fields)) = serde_norway::to_value(node) {
        let mut fields = fields;
        let created = fields
            .shift_remove("created")
            .and_then(|v| v.as_str().map(String::from));
        let updated = fields
            .shift_remove("updated")
            .and_then(|v| v.as_str().map(String::from));
        if created.is_some() || updated.is_some() {
            markers.insert(id.to_string(), (created, updated));
        }
        m.extend(fields);
    }
    serde_norway::Value::Mapping(m)
}

// The generation markers a node file leaves to `markers.yaml`.
trait Marked {
    fn marks(&self) -> (Option<&String>, Option<&String>) {
        (None, None)
    }
}

macro_rules! marked {
    ($($t:ty),*) => {$(
        impl Marked for $t {
            fn marks(&self) -> (Option<&String>, Option<&String>) {
                (self.created.as_ref(), self.updated.as_ref())
            }
        }
    )*};
}
marked!(Entity, Requirement, Criterion, View, Diagnostic);
impl Marked for Relationship {}
impl Marked for StateMachine {}
impl Marked for RedirectFile {}

// Write every node of one kind whose bytes changed; return the files of nodes that
// left the graph, for the caller to delete after every write landed. The markers are
// taken out as `node_value` takes them.
fn save_kind<'a, T: Serialize + Marked + Clone + 'a>(
    dir: &Path,
    nodes: impl Iterator<Item = (&'a String, std::borrow::Cow<'a, T>)>,
    markers: &mut Markers,
) -> Vec<PathBuf> {
    let mut ids: BTreeSet<&String> = BTreeSet::new();
    for (id, node) in nodes {
        let (created, updated) = node.marks();
        if created.is_some() || updated.is_some() {
            markers.insert(id.clone(), (created.cloned(), updated.cloned()));
        }
        let path = dir.join(format!("{}.yaml", node_stem(id).0));
        write_tracked(&path, id, node.as_ref(), || {
            node_value(id, node.as_ref(), &mut Markers::new())
        });
        ids.insert(id);
    }
    node_files(dir, &mut Vec::new())
        .into_iter()
        .filter(|(id, _)| !ids.contains(id))
        .map(|(_, path)| path)
        .collect()
}

fn borrowed<'a, T: Clone>((id, n): (&'a String, &'a T)) -> (&'a String, Cow<'a, T>) {
    (id, Cow::Borrowed(n))
}

// Write a node file or doc record through `write_yaml`, unless the file still holds
// what this process last wrote or verified there from a value with the same
// fingerprint: then its bytes already equal the serialized value, and the save skips
// serializing it. Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
fn write_tracked<T: Serialize, V: Serialize>(
    path: &Path,
    key: &str,
    source: &T,
    value: impl FnOnce() -> V,
) {
    let fp = written::fingerprint(key, source);
    if fp.is_some_and(|fp| written::holds(path, fp)) {
        return;
    }
    #[cfg(test)]
    SERIALIZED.with(|n| n.set(n.get() + 1));
    let r = write_yaml(path, &value());
    match (&r, fp) {
        (Ok(()), Some(fp)) => written::remember(path, fp),
        _ => written::forget(path),
    }
    report_write(path, r);
}

// The node files and doc records this thread serialized, for the tests to count.
#[cfg(test)]
thread_local! {
    pub(super) static SERIALIZED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// What this process last wrote to or found in each node file and doc record: the
// fingerprint of the value the bytes serialize, and the file's identity (length,
// modification time, inode) then. Any other write to the file replaces its inode or
// moves its time, so a file whose identity still matches holds those bytes. An
// in-memory tree keeps none: its reads cost no disk.
mod written {
    use serde::Serialize;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{LazyLock, Mutex};

    #[derive(Clone, Copy, PartialEq)]
    struct Identity {
        len: u64,
        modified: std::time::SystemTime,
        inode: u64,
    }

    static SEEN: LazyLock<Mutex<HashMap<PathBuf, (u128, Identity)>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    fn tracked() -> bool {
        cfg!(not(target_arch = "wasm32")) && !crate::vfs::is_scoped()
    }

    fn identity(path: &Path) -> Option<Identity> {
        let m = std::fs::metadata(path).ok()?;
        #[cfg(unix)]
        let inode = std::os::unix::fs::MetadataExt::ino(&m);
        #[cfg(not(unix))]
        let inode = 0;
        Some(Identity {
            len: m.len(),
            modified: m.modified().ok()?,
            inode,
        })
    }

    struct HashWriter(siphasher::sip128::SipHasher13);
    impl std::io::Write for HashWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            std::hash::Hasher::write(&mut self.0, buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // A 128-bit hash of the key and the value's JSON form: equal fingerprints mean
    // equal values, so equal serialized bytes.
    pub(super) fn fingerprint<T: Serialize>(key: &str, value: &T) -> Option<u128> {
        use siphasher::sip128::Hasher128;
        if !tracked() {
            return None;
        }
        let mut w = HashWriter(siphasher::sip128::SipHasher13::new());
        std::hash::Hasher::write(&mut w.0, key.as_bytes());
        std::hash::Hasher::write_u8(&mut w.0, 0);
        serde_json::to_writer(&mut w, value).ok()?;
        Some(w.0.finish128().as_u128())
    }

    pub(super) fn holds(path: &Path, fp: u128) -> bool {
        let Some(seen) = SEEN.lock().unwrap().get(path).copied() else {
            return false;
        };
        seen.0 == fp && identity(path) == Some(seen.1)
    }

    pub(super) fn remember(path: &Path, fp: u128) {
        match identity(path) {
            Some(id) => {
                SEEN.lock().unwrap().insert(path.to_path_buf(), (fp, id));
            }
            None => forget(path),
        }
    }

    pub(super) fn forget(path: &Path) {
        if tracked() {
            SEEN.lock().unwrap().remove(path);
        }
    }
}

// Copy a directory tree, for snapshots.
fn copy_tree(from: &Path, to: &Path) {
    let Ok(rd) = crate::vfs::read_dir(from) else {
        return;
    };
    if crate::vfs::create_dir_all(to).is_err() {
        return;
    }
    for e in rd.flatten() {
        let p = e.path();
        let target = to.join(e.file_name());
        if p.vis_dir() {
            copy_tree(&p, &target);
        } else if p.extension().and_then(|x| x.to_str()) == Some("yaml") {
            crate::vfs::copy(&p, &target).ok();
        }
    }
}

// A file whose bytes already match is left in place, so a save rewrites only what
// changed. The bytes end in one newline. Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
pub(super) fn write_yaml<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let mut text = serde_norway::to_string(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if crate::vfs::read(path).is_ok_and(|on_disk| on_disk == text.as_bytes()) {
        return Ok(());
    }
    write_atomic(path, text.as_bytes())
}

// Write through a sibling temporary file renamed into place, so a reader or a crash
// never sees a half-written file. Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        crate::vfs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{}.{}.tmp", name, crate::sys::process_id()));
    write_durable(&tmp, bytes)?;
    // Native executable and private files retain their mode across replacement.
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        if let Ok(metadata) = std::fs::metadata(path) {
            if let Err(e) = std::fs::set_permissions(&tmp, metadata.permissions()) {
                crate::vfs::remove_file(&tmp).ok();
                return Err(e);
            }
        }
    }
    if let Err(e) = crate::vfs::rename(&tmp, path) {
        crate::vfs::remove_file(&tmp).ok();
        return Err(e);
    }
    if let Some(parent) = path.parent() {
        TOUCHED_DIRS.with(|t| t.borrow_mut().insert(parent.to_path_buf()));
    }
    Ok(())
}

// The directories renames landed in since the last flush.
thread_local! {
    pub(super) static TOUCHED_DIRS: std::cell::RefCell<BTreeSet<PathBuf>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

// A temporary file's bytes reach the disk before the rename publishes them, so a crash
// never leaves a renamed file empty. An in-memory tree has no disk.
// Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
pub(super) fn write_durable(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        use std::io::Write;
        let mut f = std::fs::File::create(path)?;
        let written = f.write_all(bytes).and_then(|()| sync_file(&f));
        if let Err(e) = written {
            drop(f);
            std::fs::remove_file(path).ok();
            return Err(e);
        }
        return Ok(());
    }
    crate::vfs::write(path, bytes)
}

// fsync. On Apple platforms `sync_all` issues F_FULLFSYNC, a drive cache flush per
// file; plain fsync gives the same ordering against a crash of the system.
#[cfg(not(target_arch = "wasm32"))]
fn sync_file(f: &std::fs::File) -> std::io::Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::fsync(f.as_raw_fd()) } == 0 {
            return Ok(());
        }
        Err(std::io::Error::last_os_error())
    }
    #[cfg(not(target_vendor = "apple"))]
    f.sync_all()
}

// Flush the directories renames landed in, so the renames themselves survive a crash.
// Windows has no directory handle to flush; an in-memory tree has no disk.
pub(super) fn sync_touched_dirs() {
    let dirs = TOUCHED_DIRS.with(|t| std::mem::take(&mut *t.borrow_mut()));
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    if !crate::vfs::is_scoped() {
        for d in dirs {
            if let Ok(f) = std::fs::File::open(&d) {
                if let Err(e) = sync_file(&f) {
                    eprintln!("[jazyk] error: cannot flush {}: {}", d.display(), e);
                }
            }
        }
    }
    #[cfg(not(all(unix, not(target_arch = "wasm32"))))]
    drop(dirs);
}

// The stored record of one document, as the last build left it; None when the
// document never reconciled or its record does not read.
pub fn stored_doc(out: &Path, rel: &str) -> Option<DocRecord> {
    let mut rec: DocRecord = yaml_to(&out.join("docs").join(format!("{}.yaml", rel)))?;
    if let Ok(Some(build)) = yaml_read::<BuildFile>(&out.join(BUILD_FILE)) {
        if let Some(h) = build.docs.get(rel) {
            rec.content_hash = h.clone();
        }
    }
    fill_section_lines(&mut rec);
    Some(rec)
}

// The sources of the requirements naming each entity: the mentions a commit adds by
// reference. Mirrors docs/compiler/graph.md#derived-data.
fn reference_sources(graph: &Graph) -> BTreeMap<String, Vec<SourceRef>> {
    let mut out: BTreeMap<String, Vec<SourceRef>> = BTreeMap::new();
    for r in graph.requirements.values() {
        let Some(src) = &r.source else { continue };
        for e in &r.entities {
            let list = out.entry(e.clone()).or_default();
            if !list.contains(src) {
                list.push(src.clone());
            }
        }
    }
    out
}

// The canonical order of an entity's mentions: the ones it states itself first, in
// their stated order, then the ones requirements add by reference, in document order
// (link level from the stamped roots, then path, then position).
// The same order holds in memory and after a load. With `add`, the by-reference
// mentions an entity file leaves out come back; a source whose section is known and no
// longer holds its quote is stale and stays out, as the sweep would prune it.
// Mirrors docs/compiler/graph.md#node-files.
pub(super) fn normalize_mentions(
    graph: &mut Graph,
    docs: &BTreeMap<String, DocRecord>,
    roots: &[String],
    add: bool,
) {
    let stale = |m: &SourceRef| {
        docs.get(&m.doc)
            .and_then(|d| d.sections.get(&m.section))
            .is_some_and(|s| !text_contains(&s.raw, &m.quote))
    };
    // Link levels cost a pass over every section's links: computed once, and only when
    // some entity holds by-reference mentions from more than one document.
    let levels: std::cell::OnceCell<BTreeMap<String, usize>> = std::cell::OnceCell::new();
    let level = |doc: &str| -> usize {
        levels
            .get_or_init(|| crate::derive::doc_levels_in(docs, roots))
            .get(doc)
            .copied()
            .unwrap_or(usize::MAX)
    };
    let place = |m: &SourceRef, several_docs: bool| -> (usize, String, usize, usize, String) {
        let sec = docs.get(&m.doc).and_then(|d| d.sections.get(&m.section));
        let line = sec.map(|s| s.lines[0]).unwrap_or(usize::MAX);
        let at = sec
            .and_then(|s| {
                let hay = s.raw.split_whitespace().collect::<Vec<_>>().join(" ");
                let needle = m.quote.split_whitespace().collect::<Vec<_>>().join(" ");
                hay.find(&needle)
            })
            .unwrap_or(usize::MAX);
        let lv = if several_docs { level(&m.doc) } else { 0 };
        (lv, m.doc.clone(), line, at, m.quote.clone())
    };
    let by_reference = reference_sources(graph);
    for (id, e) in graph.entities.iter_mut() {
        let derived = by_reference.get(id).map(Vec::as_slice).unwrap_or(&[]);
        let (own, mut by_ref): (Vec<SourceRef>, Vec<SourceRef>) = std::mem::take(&mut e.mentions)
            .into_iter()
            .partition(|m| !derived.contains(m));
        if add {
            for src in derived {
                if !by_ref.contains(src) && !stale(src) {
                    by_ref.push(src.clone());
                }
            }
        }
        let several_docs = by_ref.iter().any(|m| m.doc != by_ref[0].doc);
        by_ref.sort_by_cached_key(|m| place(m, several_docs));
        e.mentions = own;
        e.mentions.extend(by_ref);
    }
}

// Set the local generation markers on every node that has an entry.
fn apply_markers(graph: &mut Graph, markers: &Markers) {
    macro_rules! mark {
        ($map:expr) => {
            for (id, n) in $map.iter_mut() {
                if let Some((c, u)) = markers.get(id) {
                    n.created = c.clone();
                    n.updated = u.clone();
                }
            }
        };
    }
    mark!(graph.entities);
    mark!(graph.requirements);
    mark!(graph.criteria);
    mark!(graph.views);
    mark!(graph.diagnostics);
}

// A store write that failed is reported with its path, never dropped.
pub(super) fn report_write(path: &Path, r: std::io::Result<()>) {
    if let Err(e) = r {
        eprintln!("[jazyk] error: cannot write {}: {}", path.display(), e);
    }
}

// The graph as a batch saw it: the shards and status.yaml copied under
// <out>/snapshots/<label>/ once per batch, for replays to seed from.
// Mirrors docs/compiler/sessions.md#snapshots. A hosted workspace keeps none
// (docs/compiler/hosted-runtime.md#workspace-snapshots).
pub fn snapshot_graph(out: &Path, label: &str) {
    if crate::vfs::is_scoped() {
        return;
    }
    let dir = out.join("snapshots").join(label.replace(['/', ':'], "_"));
    if dir.vexists() {
        return;
    }
    let g = out.join("graph");
    if !g.vis_dir() {
        return;
    }
    copy_tree(&g, &dir.join("graph"));
    crate::vfs::copy(out.join("status.yaml"), dir.join("status.yaml")).ok();
    for d in STATUS_DIRS {
        if out.join(d).vis_dir() {
            copy_tree(&out.join(d), &dir.join(d));
        }
    }
    crate::vfs::copy(out.join(BUILD_FILE), dir.join(BUILD_FILE)).ok();
}

impl Store {
    pub fn load(out: &Path) -> Store {
        // The browser engine's tree changes only through its own writes: a load of an
        // unchanged tree is the last one's copy (docs/frontends/wasm.md#filesystem).
        #[cfg(target_arch = "wasm32")]
        {
            thread_local! {
                static LAST: std::cell::RefCell<std::collections::BTreeMap<PathBuf, (u64, Store)>> =
                    const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
            }
            let now = crate::vfs::memory_generation();
            if let Some(store) = LAST.with(|l| {
                l.borrow()
                    .get(out)
                    .filter(|(g, _)| *g == now)
                    .map(|(_, s)| s.clone())
            }) {
                return store;
            }
            let store = Self::load_uncached(out);
            let now = crate::vfs::memory_generation();
            LAST.with(|l| {
                let mut l = l.borrow_mut();
                l.retain(|_, (g, _)| *g == now);
                l.insert(out.to_path_buf(), (now, store.clone()));
            });
            store
        }
        #[cfg(not(target_arch = "wasm32"))]
        Self::load_uncached(out)
    }

    fn load_uncached(out: &Path) -> Store {
        // A newer store reads as empty and read-only, never as a mix of its files.
        // Mirrors docs/compiler/graph.md#store-version.
        if let Some(why) = newer_store(out) {
            return Store {
                out: out.to_path_buf(),
                load_errors: vec![why],
                ..Default::default()
            };
        }
        // Readers never take the lock. A read counts when no commit was in progress at
        // its start or its end and the generation held still across it; otherwise it
        // waits and retries. A store that never holds still within the wait loads
        // read-only with a `store-busy` error, never as a mix of two commits.
        // A store of another version reads as empty; only a build archives it.
        // Mirrors docs/compiler/graph.md#concurrency and #store-version.
        let started = crate::sys::Instant::now();
        let mut attempts = 0u32;
        let store = loop {
            if !commit_in_progress(out) {
                let before = read_generation(out);
                let store = Self::load_once(out);
                if !commit_in_progress(out) && read_generation(out) == before {
                    break store;
                }
            }
            attempts += 1;
            let wait = read_wait();
            if started.elapsed() >= wait || (cfg!(target_arch = "wasm32") && attempts >= 4) {
                let mut store = Self::load_once(out);
                store.load_errors.insert(
                    0,
                    format!(
                        "store-busy: {} kept changing for {} ms of reads ({} attempts); retry once its commit finishes",
                        out.display(),
                        wait.as_millis(),
                        attempts
                    ),
                );
                break store;
            }
            crate::sys::sleep(std::time::Duration::from_millis(
                u64::from(attempts.min(10)) * 10,
            ));
        };
        for e in &store.load_errors {
            eprintln!("[jazyk] error: {} did not load: {}", out.display(), e);
        }
        if !readable_version(store.status.version) {
            return Store {
                out: out.to_path_buf(),
                ..Default::default()
            };
        }
        store
    }

    // The build's opening: an out directory whose status.yaml lacks the version or
    // carries another one is archived whole to `<out>.bak` (an earlier archive is
    // replaced) and the store starts empty. Mirrors docs/compiler/graph.md#store-version.
    pub fn open_for_build(out: &Path) -> Store {
        // A newer store is never archived: it opens read-only.
        // Mirrors docs/compiler/graph.md#store-version.
        if let Some(why) = newer_store(out) {
            eprintln!("[jazyk] error: {}; the store stays read-only", why);
            return Store {
                out: out.to_path_buf(),
                load_errors: vec![why],
                ..Default::default()
            };
        }
        let status_path = out.join("status.yaml");
        if status_path.vexists() {
            // A status.yaml that does not parse is a broken file, not another version:
            // the store is read-only and nothing is archived.
            // Mirrors docs/compiler/graph.md#store-version.
            let version = match yaml_read::<Status>(&status_path) {
                Ok(Some(s)) => s.version,
                Ok(None) => 0,
                Err(e) => {
                    eprintln!("[jazyk] error: {}; the store stays read-only", e);
                    return Store {
                        out: out.to_path_buf(),
                        load_errors: vec![e],
                        ..Default::default()
                    };
                }
            };
            if !readable_version(version) {
                let name = out
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| ".jazyk".to_string());
                let bak = out.with_file_name(format!("{}.bak", name));
                crate::vfs::remove_dir_all(&bak).ok();
                match crate::vfs::rename(out, &bak) {
                    Ok(()) => eprintln!(
                        "[jazyk] store version {} is not {}; archived {} to {} and starting empty",
                        version,
                        STORE_VERSION,
                        out.display(),
                        bak.display()
                    ),
                    Err(e) => {
                        let why = format!(
                            "could not archive {} to {}: {}",
                            out.display(),
                            bak.display(),
                            e
                        );
                        eprintln!("[jazyk] error: {}; the store stays read-only", why);
                        return Store {
                            out: out.to_path_buf(),
                            load_errors: vec![why],
                            ..Default::default()
                        };
                    }
                }
                return Store {
                    out: out.to_path_buf(),
                    ..Default::default()
                };
            }
        }
        let mut store = Self::load(out);
        // A version 3 store is rewritten whole in the version 4 layout before the build
        // begins, ids and facts kept, so even a build with nothing to do leaves no file
        // in the old form. Mirrors docs/compiler/graph.md#store-version.
        if store.status.version == UPGRADABLE_VERSION && store.load_errors.is_empty() {
            let _flock = FileLock::acquire(out);
            store.save();
            let _ledger_lock = crate::gen::Ledger::lock(out);
            let ledger = crate::gen::Ledger::load(out);
            if ledger.unreadable.is_none() {
                // A failed split is reported by save; the inline ledger still loads.
                let _ = ledger.save(out);
            }
            store.status.version = STORE_VERSION;
            eprintln!(
                "[jazyk] upgraded {} from store version {} to {}",
                out.display(),
                UPGRADABLE_VERSION,
                STORE_VERSION
            );
        }
        store
    }

    fn load_once(out: &Path) -> Store {
        let g = out.join("graph");
        let mut errors: Vec<String> = Vec::new();
        let mut views: BTreeMap<String, View> = load_nodes(&g.join("views"), &mut errors);
        views.extend(load_nodes::<View>(&g.join("default-views"), &mut errors));
        let mut store = Store {
            out: out.to_path_buf(),
            graph: Graph {
                entities: load_nodes(&g.join("entities"), &mut errors),
                requirements: load_nodes(&g.join("requirements"), &mut errors),
                criteria: load_nodes(&g.join("criteria"), &mut errors),
                views,
                relationships: load_nodes(&g.join("relationships"), &mut errors),
                state_machines: load_nodes(&g.join("state-machines"), &mut errors),
                diagnostics: load_nodes(&g.join("diagnostics"), &mut errors),
                redirects: load_nodes::<RedirectFile>(&g.join("redirects"), &mut errors)
                    .into_iter()
                    .map(|(id, r)| (id, r.to))
                    .collect(),
            },
            docs: BTreeMap::new(),
            status: read_status(out, &mut errors),
            align: Default::default(),
            load_errors: Vec::new(),
        };
        // The local half of the status, raised to the committed floor.
        // Mirrors docs/compiler/graph.md#the-generation.
        let build: BuildFile = shard(&out.join(BUILD_FILE), &mut errors);
        let docs_dir = out.join("docs");
        let mut files = Vec::new();
        collect_yaml(&docs_dir, &mut files);
        for f in files {
            if let Ok(rel) = f.strip_prefix(&docs_dir) {
                let doc = rel.to_string_lossy().replace('\\', "/");
                let doc = doc.strip_suffix(".yaml").unwrap_or(&doc).to_string();
                match yaml_read::<DocRecord>(&f) {
                    Ok(Some(mut rec)) => {
                        // The content hash is local; a version 3 record still carries one.
                        if let Some(h) = build.docs.get(&doc) {
                            rec.content_hash = h.clone();
                        }
                        fill_section_lines(&mut rec);
                        store.docs.insert(doc, rec);
                    }
                    Ok(None) => {}
                    Err(e) => errors.push(e),
                }
            }
        }
        store.status.generation = build.generation.max(generation_floor(&store.status));
        store.status.verdict = build.verdict;
        store.status.costs = build.costs;
        store.status.spent = build.spent;
        store.status.diagnostics = build.diagnostics;
        // The generation markers are local; a version 3 node file still carries its own.
        // Mirrors docs/compiler/graph.md#storage-layout.
        let markers: Markers = shard(&out.join(MARKERS_FILE), &mut errors);
        apply_markers(&mut store.graph, &markers);
        let roots = store.status.roots.clone();
        normalize_mentions(&mut store.graph, &store.docs, &roots, true);
        // The derived kinds are local: a checkout carries none, so a load that finds
        // any of their directories missing recomputes them in memory and the next
        // commit writes them. Mirrors docs/compiler/graph.md#a-fresh-checkout.
        if DERIVED_KINDS.iter().any(|k| !g.join(k).vis_dir()) {
            crate::derive::recompute_relationships(&mut store);
            crate::derive::recompute_state_machines(&mut store);
            let build = format!("g{}", store.status.generation);
            let mut batch = RecordBatch::new(store.status.generation);
            crate::derive::recompute_default_views(&mut store, &build, &mut batch);
        }
        store.load_errors = errors;
        store
    }

    // Why this store must not write: a file that did not parse at load.
    // Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
    pub(super) fn unreadable(&self) -> Option<String> {
        self.load_errors.first().map(|e| {
            if e.starts_with("store-busy:") || e.starts_with("store-newer:") {
                return e.clone();
            }
            format!(
                "store-unreadable: {} (repair or remove the file, then retry)",
                e
            )
        })
    }

    // Why this store must not commit: it did not load cleanly, the directory holds a
    // store of another version, or another writer landed a generation since it loaded.
    // Called under the lock. Mirrors docs/compiler/graph.md#concurrency.
    fn refusal(&self) -> Option<String> {
        if let Some(why) = self.unreadable() {
            return Some(why);
        }
        if let Some(why) = newer_store(&self.out) {
            return Some(why);
        }
        match yaml_read::<Status>(&self.out.join("status.yaml")) {
            Ok(Some(disk)) if !readable_version(disk.version) => Some(format!(
                "store-version: {} holds a version {} store and this is version {}; a build archives it",
                self.out.display(),
                disk.version,
                STORE_VERSION
            )),
            Err(e) => Some(format!(
                "store-unreadable: {} (repair or remove the file, then retry)",
                e
            )),
            _ => match disk_generation(&self.out) {
                Ok(Some(g)) if g != self.status.generation => Some(format!(
                    "store-moved: {} is at g{} on disk and g{} in memory; reload and restage",
                    self.out.display(),
                    g,
                    self.status.generation
                )),
                Ok(_) => None,
                Err(e) => Some(format!(
                    "store-unreadable: {} (repair or remove the file, then retry)",
                    e
                )),
            },
        }
    }

    // The refusal, reported. Every writer calls this under the lock before it mutates.
    pub(super) fn refuse(&self, what: &str) -> Option<String> {
        let why = self.refusal()?;
        eprintln!("[jazyk] error: {} refused: {}", what, why);
        Some(why)
    }

    // Node files and doc records first, then the files of removed nodes, the status
    // last: the generation in `build.yaml` is the commit point a reader checks. The
    // commit marker spans every write. Callers hold the lock and checked the refusal;
    // outside this module `write_status` is the only way to write.
    // Mirrors docs/compiler/graph.md#writes-and-unreadable-files and #node-files.
    pub(super) fn save(&self) {
        if let Some(why) = self.unreadable() {
            eprintln!(
                "[jazyk] error: refusing to write {}: {}",
                self.out.display(),
                why
            );
            return;
        }
        let _marker = CommitMarker::begin(&self.out);
        self.save_nodes();
        self.save_status_files();
    }

    // A test's seed of a store on disk, written as a commit writes it.
    #[cfg(test)]
    pub(crate) fn save_for_test(&self) {
        self.save();
    }

    pub(super) fn save_nodes(&self) {
        let g = self.out.join("graph");
        let mut markers = Markers::new();
        let redirects: BTreeMap<String, RedirectFile> = self
            .graph
            .redirects
            .iter()
            .map(|(from, to)| (from.clone(), RedirectFile { to: to.clone() }))
            .collect();
        // An entity file holds only the mentions its own facts state: a mention that
        // is the source of a requirement naming the entity is derived and comes back at
        // load. Mirrors docs/compiler/graph.md#node-files.
        let by_reference = reference_sources(&self.graph);
        let entities = self
            .graph
            .entities
            .iter()
            .map(|(id, e)| match by_reference.get(id) {
                Some(d) if e.mentions.iter().any(|m| d.contains(m)) => {
                    let mut own = e.clone();
                    own.mentions.retain(|m| !d.contains(m));
                    (id, Cow::Owned(own))
                }
                _ => (id, Cow::Borrowed(e)),
            });
        let mut gone = Vec::new();
        gone.extend(save_kind(&g.join("entities"), entities, &mut markers));
        gone.extend(save_kind(
            &g.join("requirements"),
            self.graph.requirements.iter().map(borrowed),
            &mut markers,
        ));
        gone.extend(save_kind(
            &g.join("criteria"),
            self.graph.criteria.iter().map(borrowed),
            &mut markers,
        ));
        gone.extend(save_kind(
            &g.join("views"),
            self.graph
                .views
                .iter()
                .filter(|(_, v)| !v.default)
                .map(borrowed),
            &mut markers,
        ));
        gone.extend(save_kind(
            &g.join("default-views"),
            self.graph
                .views
                .iter()
                .filter(|(_, v)| v.default)
                .map(borrowed),
            &mut markers,
        ));
        gone.extend(save_kind(
            &g.join("diagnostics"),
            self.graph.diagnostics.iter().map(borrowed),
            &mut markers,
        ));
        gone.extend(save_kind(
            &g.join("redirects"),
            redirects.iter().map(borrowed),
            &mut Markers::new(),
        ));
        gone.extend(save_kind(
            &g.join("relationships"),
            self.graph.relationships.iter().map(borrowed),
            &mut Markers::new(),
        ));
        gone.extend(save_kind(
            &g.join("state-machines"),
            self.graph.state_machines.iter().map(borrowed),
            &mut Markers::new(),
        ));
        // A derived directory exists once saved, even empty, so a load recomputes the
        // derived kinds only on a checkout that lacks them.
        for kind in DERIVED_KINDS {
            crate::vfs::create_dir_all(g.join(kind)).ok();
        }
        for (doc, rec) in &self.docs {
            let p = self.out.join("docs").join(format!("{}.yaml", doc));
            write_tracked(&p, doc, rec, || doc_record_value(rec));
        }
        for p in gone {
            if let Err(e) = crate::vfs::remove_file(&p) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("[jazyk] error: cannot remove {}: {}", p.display(), e);
                }
            }
        }
        let path = self.out.join(MARKERS_FILE);
        report_write(&path, write_yaml(&path, &markers));
        // A `.jazyk` folder never exists without its ignore file.
        // Mirrors docs/compiler/document-sets.md#persistence-and-identity.
        if let Some(jazyk) = self
            .out
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == ".jazyk"))
        {
            if let Some(root) = jazyk.parent() {
                if let Err(e) = crate::document_sets::ensure_ignore(root) {
                    eprintln!(
                        "[jazyk] error: cannot write {}/.gitignore: {}",
                        jazyk.display(),
                        e
                    );
                }
            }
        }
    }

    // A commit's write: the mentions in their canonical order, then the files.
    pub(super) fn persist(&mut self) {
        let roots = self.status.roots.clone();
        normalize_mentions(&mut self.graph, &self.docs, &roots, false);
        self.save();
    }
}

pub(super) fn collect_yaml(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = crate::vfs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.vis_dir() {
                collect_yaml(&p, out);
            } else if p.extension().map(|e| e == "yaml").unwrap_or(false) {
                out.push(p);
            }
        }
    }
}
