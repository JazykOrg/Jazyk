//! A redo image makes a staged graph commit recoverable across multi-file saves.
use super::*;
#[derive(Serialize, Deserialize)]
pub(super) struct Redo {
    pub store: Store,
    pub entry: JournalEntry,
}
pub(super) fn redo_path(out: &Path) -> PathBuf {
    out.join("local/pending-commit.json")
}
impl Store {
    pub(super) fn prepare_redo(&self, entry: &JournalEntry) -> Result<(), String> {
        let redo = Redo {
            store: self.clone(),
            entry: entry.clone(),
        };
        let bytes = serde_json::to_vec(&redo).map_err(|e| e.to_string())?;
        write_atomic(&redo_path(&self.out), &bytes).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub(super) fn recover_redo(out: &Path) -> Result<(), String> {
        let path = redo_path(out);
        if !crate::vfs::exists(&path) {
            return Ok(());
        }
        let _lock = FileLock::acquire(out);
        if _lock.busy() {
            return Err("store-busy: pending commit recovery".into());
        }
        // Another reader may already have recovered while this one waited.
        if !crate::vfs::exists(&path) {
            return Ok(());
        }
        let bytes = crate::vfs::read(&path).map_err(|e| e.to_string())?;
        let mut redo: Redo = serde_json::from_slice(&bytes)
            .map_err(|e| format!("pending commit is unreadable: {e}"))?;
        if redo.store.out != out || !readable_version(redo.store.status.version) {
            return Err("pending commit belongs to another store or version".into());
        }
        if read_generation(out) > redo.store.status.generation {
            return Err("pending commit precedes current store; retain for inspection".into());
        }
        redo.store.load_errors.clear();
        redo.store.write_journal(&redo.entry);
        redo.store.persist();
        redo.store.verify_publication()?;
        if read_generation(out) != redo.store.status.generation {
            return Err("pending commit save failed; retry recovery".into());
        }
        crate::vfs::remove_file(path).map_err(|e| e.to_string())?;
        Ok(())
    }
}
