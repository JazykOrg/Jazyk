// The entity card the explorer shows in the inspector: the same card docsgen renders
// (docs/consumers/docsgen.md#entity-cards), served live from the shared model
// (GET /api/entities/{id}/card). Every item is a link that moves the explorer: up
// along `Sits in`, down along `Inside` and the children, sideways along the
// relationships and the siblings (docs/frontends/gui.md#explore).
import { useExplorer } from '../lib/explore'
import { useEntityCard, useGraph, useTree } from '../lib/queries'
import { useInspector } from '../lib/nav'
import { pressable } from '../lib/a11y'
import type { CardKin, CardRelation } from '../lib/api'

const SCOPE_PREFIX = 'scope:'

// The arrow a relationship draws from the entity's side.
function arrow(r: CardRelation): string {
  return r.direction === 'a' ? '→' : r.direction === 'b' ? '←' : '↔'
}

export default function ExploreCard({ id }: { id: string }) {
  const card = useEntityCard(id)
  const { data: graph } = useGraph()
  const { data: tree } = useTree()
  const x = useExplorer()
  const { openNode } = useInspector()
  const viewTitle = (vid: string) => graph?.views[vid]?.title ?? vid.replace(/^view:/, '')

  if (card.isLoading) return <p className="muted">reading the card…</p>
  if (card.error) return <p className="error-inline">{card.error.message}</p>
  const c = card.data
  if (!c) return null

  const chipClass = (vid: string) => `xp-chip${x.view === vid ? ' active' : ''}`
  const kin = (k: CardKin, view?: string) => (
    <a key={k.id} className="xp-chip" title={k.id} {...pressable(() => x.goEntity(k.id, view ? { view } : undefined))}>
      {k.name}
      {k.childCount > 0 && <span className="xp-count">{k.childCount}</span>}
    </a>
  )
  const viewChip = (vid: string, ent?: string, label?: string) => (
    <a key={vid} className={chipClass(vid)} title={vid} {...pressable(() => x.goView(vid, ent))}>
      {label ?? viewTitle(vid)}
    </a>
  )

  return (
    <div className="card xp-card">
      <div className="xp-head">
        <span className="xp-nav">
          <button className="ide-mini" onClick={x.back} disabled={!x.canBack} title="back along the walk">
            ‹
          </button>
          <button className="ide-mini" onClick={x.forward} disabled={!x.canForward} title="forward along the walk">
            ›
          </button>
        </span>
        <h3 style={{ margin: 0 }}>
          {c.name}
          {c.stereotype && <span className="chip sev-info">«{c.stereotype}»</span>}
          {c.provenance !== 'quote' && (
            <span
              className={`chip ${c.provenance === 'derived' ? 'sev-info' : 'sev-warning'}`}
              title="no document states this entity; its ratification proposal is pending"
            >
              {c.provenance}
            </span>
          )}
        </h3>
      </div>
      {c.definition && <p style={{ margin: '4px 0' }}>{c.definition}</p>}
      {c.proposal && (
        <p style={{ margin: '2px 0' }}>
          <a className="xp-chip" {...pressable(() => openNode(c.proposal!))} title={c.proposal}>
            ratification proposal
          </a>
        </p>
      )}

      <div className="xp-row">
        <span className="xp-label">sits in</span>
        {c.breadcrumb.map((b, i) => {
          const last = i === c.breadcrumb.length - 1
          // The scope root has no card: its crumb overlays the root's level view.
          const rootView = b.id.startsWith(SCOPE_PREFIX)
            ? (tree?.roots.find((r) => r.target === b.id)?.levelView ?? null)
            : null
          return (
            <span key={b.id} style={{ display: 'contents' }}>
              {i > 0 && <span className="map-crumb-sep">›</span>}
              {last ? (
                <span className="xp-here" title={b.id}>
                  {b.name}
                </span>
              ) : rootView !== null || b.id.startsWith(SCOPE_PREFIX) ? (
                <a
                  className={`xp-chip${rootView ? '' : ' disabled'}`}
                  title={rootView ? `overlay ${rootView}` : `${b.id}: no level view`}
                  {...pressable(() => rootView && x.goView(rootView))}
                >
                  {b.name}
                </a>
              ) : (
                <a className="xp-chip" title={b.id} {...pressable(() => x.goEntity(b.id))}>
                  {b.name}
                </a>
              )}
            </span>
          )
        })}
      </div>

      <div className="xp-row">
        <span className="xp-label">in context</span>
        {c.context ? viewChip(c.context, c.id) : <span className="muted">no level view</span>}
        {c.flows.length > 0 && (
          <>
            <span className="xp-label">flows</span>
            {c.flows.map((f) => viewChip(f, c.id))}
          </>
        )}
      </div>

      <div className="xp-row">
        <span className="xp-label">inside</span>
        {c.inside ? (
          <>
            {viewChip(c.inside, c.id, 'structure')}
            {c.insideFlows.map((f) => viewChip(f, c.id))}
          </>
        ) : (
          <span className="muted">a leaf</span>
        )}
      </div>

      {c.relationships.length > 0 && (
        <div className="xp-row">
          <span className="xp-label">relationships</span>
          <div className="xp-rels">
            {c.relationships.map((r) => (
              <a
                key={`${r.other}|${r.type}`}
                className="xp-chip"
                title={`${r.other}: ${r.count} requirement${r.count === 1 ? '' : 's'}`}
                {...pressable(() => x.goEntity(r.other))}
              >
                <span className="muted mono">
                  {arrow(r)} {r.type}
                </span>{' '}
                {graph?.entities[r.other]?.name ?? r.other}
                <span className="xp-count">{r.count}</span>
              </a>
            ))}
          </div>
        </div>
      )}

      {c.siblings.length > 0 && (
        <div className="xp-row">
          <span className="xp-label">siblings</span>
          {c.siblings.map((s) => kin(s, c.context ?? undefined))}
        </div>
      )}

      {c.children.length > 0 && (
        <div className="xp-row">
          <span className="xp-label">children</span>
          {c.children.map((k) => kin(k, c.inside ?? undefined))}
        </div>
      )}

      <div className="xp-row">
        <span className="xp-label">more</span>
        <a className="xp-chip" href="#xp-detail" onClick={(e) => { e.preventDefault(); document.getElementById('xp-detail')?.scrollIntoView({ block: 'start' }) }}>
          {c.requirementCount} requirement{c.requirementCount === 1 ? '' : 's'}
        </a>
      </div>
    </div>
  )
}
