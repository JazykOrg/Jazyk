//! Workspace file entries. Link contents are their relative target, like Git blobs.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Deref, DerefMut};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    files: BTreeMap<String, Vec<u8>>,
    symlinks: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FileEntry {
    File { bytes: Vec<u8> },
    Symlink { target: String },
}

impl Snapshot {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&mut self, path: String, bytes: Vec<u8>) -> Option<Vec<u8>> {
        self.symlinks.remove(&path);
        self.files.insert(path, bytes)
    }
    pub fn insert_symlink(&mut self, path: String, target: String) -> Option<Vec<u8>> {
        self.symlinks.insert(path.clone());
        self.files.insert(path, target.into_bytes())
    }
    pub fn mark_symlink(&mut self, path: &str) {
        if self.files.contains_key(path) {
            self.symlinks.insert(path.into());
        }
    }
    pub fn extend<T: IntoIterator<Item = (String, Vec<u8>)>>(&mut self, entries: T) {
        for (p, b) in entries {
            self.insert(p, b);
        }
    }
    pub fn clear(&mut self) {
        self.files.clear();
        self.symlinks.clear();
    }
    pub fn retain<F: FnMut(&String, &mut Vec<u8>) -> bool>(&mut self, predicate: F) {
        self.files.retain(predicate);
        let files = &self.files;
        self.symlinks.retain(|path| files.contains_key(path));
    }
    pub fn is_symlink(&self, path: &str) -> bool {
        self.symlinks.contains(path) && self.files.contains_key(path)
    }
    pub fn remove(&mut self, path: &str) -> Option<Vec<u8>> {
        self.symlinks.remove(path);
        self.files.remove(path)
    }
    pub fn symlinks(&self) -> impl Iterator<Item = (&String, &Vec<u8>)> {
        self.symlinks
            .iter()
            .filter_map(|path| self.files.get_key_value(path))
    }
    pub fn entries(&self) -> impl Iterator<Item = (&String, FileEntry)> {
        self.files.iter().map(|(path, bytes)| {
            (
                path,
                if self.is_symlink(path) {
                    FileEntry::Symlink {
                        target: String::from_utf8_lossy(bytes).into_owned(),
                    }
                } else {
                    FileEntry::File {
                        bytes: bytes.clone(),
                    }
                },
            )
        })
    }
    pub fn from_entries(entries: BTreeMap<String, FileEntry>) -> Self {
        let mut out = Self::new();
        for (path, entry) in entries {
            match entry {
                FileEntry::File { bytes } => {
                    out.insert(path, bytes);
                }
                FileEntry::Symlink { target } => {
                    out.insert_symlink(path, target);
                }
            }
        }
        out
    }
}
impl Deref for Snapshot {
    type Target = BTreeMap<String, Vec<u8>>;
    fn deref(&self) -> &Self::Target {
        &self.files
    }
}
impl DerefMut for Snapshot {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.files
    }
}
impl From<BTreeMap<String, Vec<u8>>> for Snapshot {
    fn from(files: BTreeMap<String, Vec<u8>>) -> Self {
        Self {
            files,
            symlinks: BTreeSet::new(),
        }
    }
}
impl FromIterator<(String, Vec<u8>)> for Snapshot {
    fn from_iter<T: IntoIterator<Item = (String, Vec<u8>)>>(iter: T) -> Self {
        BTreeMap::from_iter(iter).into()
    }
}
impl<'a> IntoIterator for &'a Snapshot {
    type Item = (&'a String, &'a Vec<u8>);
    type IntoIter = std::collections::btree_map::Iter<'a, String, Vec<u8>>;
    fn into_iter(self) -> Self::IntoIter {
        self.files.iter()
    }
}
impl IntoIterator for Snapshot {
    type Item = (String, Vec<u8>);
    type IntoIter = std::collections::btree_map::IntoIter<String, Vec<u8>>;
    fn into_iter(self) -> Self::IntoIter {
        self.files.into_iter()
    }
}
impl Serialize for Snapshot {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.entries()
            .map(|(p, e)| (p.clone(), e))
            .collect::<BTreeMap<_, _>>()
            .serialize(s)
    }
}
impl<'de> Deserialize<'de> for Snapshot {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        BTreeMap::<String, FileEntry>::deserialize(d).map(Self::from_entries)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait FileTree {
    fn files(&self) -> &BTreeMap<String, Vec<u8>>;
    fn is_symlink(&self, _path: &str) -> bool {
        false
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl FileTree for BTreeMap<String, Vec<u8>> {
    fn files(&self) -> &BTreeMap<String, Vec<u8>> {
        self
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl FileTree for Snapshot {
    fn files(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.files
    }
    fn is_symlink(&self, path: &str) -> bool {
        self.is_symlink(path)
    }
}
impl<const N: usize> From<[(String, Vec<u8>); N]> for Snapshot {
    fn from(entries: [(String, Vec<u8>); N]) -> Self {
        BTreeMap::from(entries).into()
    }
}
