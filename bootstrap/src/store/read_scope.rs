// The read indices of one derivation. Mirrors docs/compiler/graph.md#derived-data.
use super::*;

// The read indices of one derivation (docs/compiler/graph.md#derived-data). While a
// `ReadScope` borrows a store, nothing can change the store, so the whole-graph scans
// that a board derivation repeats per goal (`requirements_referencing`,
// `derive::instance_types`, `board::level_members`) answer from indices built on
// first use and dropped with the scope. Each index holds what the scan it replaces
// computes.
pub struct ReadScope<'a> {
    prev: Option<std::rc::Rc<ScopeIndex>>,
    _store: std::marker::PhantomData<&'a Store>,
}

#[derive(Default)]
pub(crate) struct ScopeIndex {
    store: usize,
    requirements_of: std::cell::OnceCell<BTreeMap<String, Vec<String>>>,
    instance_types: std::cell::OnceCell<BTreeMap<String, String>>,
    children: std::cell::OnceCell<BTreeMap<String, Vec<String>>>,
    scope_roots: std::cell::OnceCell<BTreeMap<String, Vec<String>>>,
}

thread_local! {
    static READ_SCOPE: std::cell::RefCell<Option<std::rc::Rc<ScopeIndex>>> =
        const { std::cell::RefCell::new(None) };
}

impl<'a> ReadScope<'a> {
    pub fn enter(store: &'a Store) -> ReadScope<'a> {
        let index = std::rc::Rc::new(ScopeIndex {
            store: store as *const Store as usize,
            ..Default::default()
        });
        let prev = READ_SCOPE.with(|s| s.borrow_mut().replace(index));
        ReadScope {
            prev,
            _store: std::marker::PhantomData,
        }
    }
}

impl Drop for ReadScope<'_> {
    fn drop(&mut self) {
        let prev = self.prev.take();
        READ_SCOPE.with(|s| *s.borrow_mut() = prev);
    }
}

impl ScopeIndex {
    // The index of the scope open over this very store, if one is.
    pub(crate) fn of(store: &Store) -> Option<std::rc::Rc<ScopeIndex>> {
        READ_SCOPE.with(|s| {
            s.borrow()
                .as_ref()
                .filter(|ix| ix.store == store as *const Store as usize)
                .cloned()
        })
    }

    // Entity id (redirects followed) to the requirements naming it, by id.
    pub(crate) fn requirements_of(&self, store: &Store) -> &BTreeMap<String, Vec<String>> {
        self.requirements_of.get_or_init(|| {
            let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (rid, r) in &store.graph.requirements {
                for e in &r.entities {
                    let list = out.entry(store.resolve_id(e).to_string()).or_default();
                    if list.last() != Some(rid) {
                        list.push(rid.clone());
                    }
                }
            }
            out
        })
    }

    pub(crate) fn instance_types(
        &self,
        compute: impl FnOnce() -> BTreeMap<String, String>,
    ) -> &BTreeMap<String, String> {
        self.instance_types.get_or_init(compute)
    }

    // Parent id to its direct children, by id.
    pub(crate) fn children(&self, store: &Store) -> &BTreeMap<String, Vec<String>> {
        self.children.get_or_init(|| {
            let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (id, e) in &store.graph.entities {
                if let Some(p) = &e.parent {
                    out.entry(p.clone()).or_default().push(id.clone());
                }
            }
            out
        })
    }

    // Scope to its parentless entities, by id.
    pub(crate) fn scope_roots(&self, store: &Store) -> &BTreeMap<String, Vec<String>> {
        self.scope_roots.get_or_init(|| {
            let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (id, e) in &store.graph.entities {
                if e.parent.is_none() {
                    out.entry(e.scope.clone()).or_default().push(id.clone());
                }
            }
            out
        })
    }
}
