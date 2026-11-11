// Deterministic per-entity requirements documents, level pages, entity cards, and
// diagram pages: the reading surface between prose and graph. Rendered on every
// commit, no LLM. Renders the diagrams first so every image link resolves, embeds the
// views relevant to each entity, and cross-links the pages through the views they
// share. Mirrors docs/consumers/docsgen.md.
use crate::derive::instance_types;
use crate::gen::GenSettings;
use crate::store::Store;

mod documents;
mod index;
mod levels;
mod links;
mod walk;

#[cfg(test)]
mod tests;

use crate::card::Walk;
use documents::write_documents;
use index::{write_index, write_reports};
pub use levels::level_page;
use levels::write_level_pages;
use walk::{write_cards, write_view_pages};

pub fn write_all(store: &Store, gs: &GenSettings) -> usize {
    // Render the diagrams first, so every image link below resolves even when this
    // run is `jazyk docsgen` alone. Unchanged views are skipped by content.
    crate::render::render_all(store, &store.out);
    let vmap = crate::verify::status_map(store, gs);
    let ledger = crate::gen::Ledger::load(&store.out);
    let instances = instance_types(store);
    let dir = crate::layout::docsgen_dir(&store.out);
    crate::vfs::create_dir_all(&dir).ok();
    let mut written = write_documents(store, &dir, &instances, &ledger, &vmap);

    // The level pages: one per node with a level view and one per scope root with
    // one, nested as the containment tree is. Mirrors docs/consumers/docsgen.md#level-pages.
    written += write_level_pages(store, &dir);
    // The walk: one card per entity, one page per view, from the shared model.
    // Mirrors docs/consumers/docsgen.md#entity-cards.
    let walk = Walk::new(store);
    written += write_cards(store, &walk, &dir);
    written += write_view_pages(store, &walk, &dir);

    // The index: every entity linked, the default class and component views rendered
    // from the graph, every view listed, and the pending proposals grouped by target
    // document. Mirrors docs/consumers/docsgen.md#relationships-view.
    if !store.graph.entities.is_empty() {
        write_index(store, &dir);
        written += write_reports(store, &dir);
    }
    written
}
