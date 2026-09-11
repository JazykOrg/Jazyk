// The graph sidebar: one text filter plus facet lists over the live shards
// (docs/frontends/gui.md#graph). The entity list is the containment tree the
// `parent` field makes, one root per scope, each node with its child count and its
// level view ids. A row opens the node in the inspector and focuses it on the map;
// a view row, or a level view id on a tree node, overlays the view.
import { useEffect, useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'
import { useCoverage, useGraph, useMatrix, useTree, useViews } from '../lib/queries'
import type { Graph, TreeData, TreeNode, TreeRoot } from '../lib/api'
import { ancestorsOf, levelChain } from '../lib/levels'
import { selectNodeParams } from '../lib/nav'
import { pressable } from '../lib/a11y'
import { verifyClass } from '../components/Chip'
import { reverseIndex } from '../components/Cards'

const LISTS = ['entities', 'requirements', 'views', 'diagnostics', 'coverage'] as const
type ListKind = (typeof LISTS)[number]
const WINDOW = 200

// The nodes a filter keeps: a node matches on its id, name, definition, or aliases,
// and a node with a matching descendant stays to hold the path down to it.
function filterTree(tree: TreeData, graph: Graph, q: string): Set<string> {
  const keep = new Set<string>()
  const walk = (n: TreeNode): boolean => {
    const e = graph.entities[n.id]
    const self = `${n.id} ${n.name} ${e?.definition ?? ''} ${(e?.aliases ?? []).join(' ')}`
      .toLowerCase()
      .includes(q)
    let any = self
    for (const c of n.children) if (walk(c)) any = true
    if (any) keep.add(n.id)
    return any
  }
  for (const r of tree.roots) {
    let any = false
    for (const c of r.children) if (walk(c)) any = true
    if (any) keep.add(r.target)
  }
  return keep
}

// The open ratification proposal on a grouping, when one stands: the
// `ratification-pending` diagnostic naming it as a subject.
function proposalIndex(graph: Graph): Map<string, string> {
  const m = new Map<string, string>()
  for (const [did, d] of Object.entries(graph.diagnostics)) {
    if (d.rule !== 'ratification-pending' || d.lifecycle === 'resolved') continue
    for (const s of d.subjects ?? []) if (!m.has(s)) m.set(s, did)
  }
  return m
}

interface TreeProps {
  tree: TreeData
  graph: Graph
  q: string
  node: string | null
  view: string | null
  revIdx: Map<string, string[]>
  openAndFocus: (id: string) => void
  overlayView: (id: string) => void
}

// The containment tree (docs/frontends/gui.md#graph): collapsible, the inspected
// node highlighted, the node whose level is overlaid marked, level view ids as
// chips that overlay the view, groupings marked with their proposal one click away.
function ContainmentTree({ tree, graph, q, node, view, revIdx, openAndFocus, overlayView }: TreeProps) {
  const [sp, setSp] = useSearchParams()
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set())
  const [seeded, setSeeded] = useState(false)
  const keep = useMemo(() => (q ? filterTree(tree, graph, q) : null), [tree, graph, q])
  const proposals = useMemo(() => proposalIndex(graph), [graph])
  const chain = useMemo(() => (view ? levelChain(tree, view) : null), [tree, view])
  const levelNode = chain ? chain.crumbs[chain.crumbs.length - 1].target : null

  // The scope roots open on first render; the ancestors of the inspected node and
  // of the overlaid level open as they change, so the current position is in view.
  useEffect(() => {
    if (seeded) return
    setExpanded(new Set(tree.roots.map((r) => r.target)))
    setSeeded(true)
  }, [tree, seeded])
  useEffect(() => {
    const want = [...(node ? ancestorsOf(tree, node) : []), ...(chain ? chain.crumbs.map((c) => c.target) : [])]
    if (want.length === 0) return
    setExpanded((prev) => {
      if (want.every((t) => prev.has(t))) return prev
      const next = new Set(prev)
      for (const t of want) next.add(t)
      return next
    })
  }, [tree, node, chain])

  const toggle = (id: string) =>
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  const openDiagnostic = (id: string) => {
    const next = new URLSearchParams(sp)
    next.set('node', id)
    setSp(next)
  }

  const viewChips = (levelView: string | null, views: string[], depth: number) =>
    levelView ? (
      <div className="wb-tree-views" style={{ paddingLeft: 26 + depth * 14 }}>
        {[levelView, ...views].map((id) => (
          <a
            key={id}
            className={`wb-tree-view${view === id ? ' active' : ''}`}
            title={`overlay ${id}`}
            {...pressable(() => overlayView(id))}
          >
            {id.replace(/^view:/, '')}
          </a>
        ))}
      </div>
    ) : null

  const rows = (n: TreeNode, depth: number): React.ReactNode => {
    if (keep && !keep.has(n.id)) return null
    const open = keep ? true : expanded.has(n.id)
    const proposal = n.grouping ? proposals.get(n.id) : undefined
    return (
      <div key={n.id}>
        <div
          className={`wb-tree-row${node === n.id ? ' active' : ''}${levelNode === n.id ? ' level' : ''}`}
          style={{ paddingLeft: 8 + depth * 14 }}
        >
          <button
            className="wb-tree-caret"
            onClick={() => toggle(n.id)}
            disabled={n.children.length === 0}
            title={n.children.length === 0 ? 'a leaf' : open ? 'collapse' : 'expand'}
          >
            {n.children.length === 0 ? '·' : open ? '▾' : '▸'}
          </button>
          <a className="wb-tree-name" {...pressable(() => openAndFocus(n.id))} title={n.id}>
            {n.name}
            {n.stereotype ? <span className="muted"> «{n.stereotype}»</span> : null}
          </a>
          {n.grouping && (
            <span className="chip sev-none" title="a grouping: derived provenance, no document states it">
              grouping
            </span>
          )}
          {proposal && (
            <a className="wb-tree-proposal" title="the ratification proposal" {...pressable(() => openDiagnostic(proposal))}>
              ratify
            </a>
          )}
          <span className="sub">
            {n.count > 0 ? `${n.count} children · ` : ''}
            {(revIdx.get(n.id) ?? []).length} req
          </span>
        </div>
        {viewChips(n.levelView, n.views, depth)}
        {open && n.children.map((c) => rows(c, depth + 1))}
      </div>
    )
  }

  const root = (r: TreeRoot) => {
    if (keep && !keep.has(r.target)) return null
    const open = keep ? true : expanded.has(r.target)
    return (
      <div key={r.target}>
        <div className={`wb-tree-row root${levelNode === r.target ? ' level' : ''}`} style={{ paddingLeft: 8 }}>
          <button className="wb-tree-caret" onClick={() => toggle(r.target)} title={open ? 'collapse' : 'expand'}>
            {open ? '▾' : '▸'}
          </button>
          <a
            className="wb-tree-name mono"
            title={r.levelView ? `overlay ${r.levelView}` : 'the scope root'}
            {...pressable(() => r.levelView && overlayView(r.levelView))}
          >
            {r.target}
          </a>
          <span className="sub">{r.count} at the root</span>
        </div>
        {viewChips(r.levelView, r.views, 0)}
        {open && r.children.map((c) => rows(c, 1))}
      </div>
    )
  }

  if (tree.roots.length === 0) return <p className="muted wb-side-pad">no entities yet</p>
  if (keep && keep.size === 0) return <p className="muted wb-side-pad">nothing matches the filter</p>
  return <div className="wb-tree">{tree.roots.map(root)}</div>
}

export default function GraphSidebar() {
  const [sp, setSp] = useSearchParams()
  const list: ListKind = (LISTS as readonly string[]).includes(sp.get('list') ?? '')
    ? (sp.get('list') as ListKind)
    : 'entities'
  const q = (sp.get('q') ?? '').toLowerCase()
  const node = sp.get('node')
  const view = sp.get('view')

  const graph = useGraph()
  const tree = useTree()
  const matrix = useMatrix()
  const coverage = useCoverage()
  const views = useViews()
  const rows = matrix.data?.rows ?? {}
  const revIdx = useMemo(
    () => (graph.data ? reverseIndex(graph.data) : new Map<string, string[]>()),
    [graph.data],
  )

  const setParam = (k: string, v: string | null) => {
    const next = new URLSearchParams(sp)
    if (v === null) next.delete(k)
    else next.set(k, v)
    setSp(next, { replace: true })
  }

  // Open in the inspector and focus on the map, one click. An entity row is a step
  // of the walk: its card opens and the explorer's position moves with it.
  const openAndFocus = (id: string) => {
    const next = new URLSearchParams(sp)
    selectNodeParams(next, id)
    next.set('focus', id)
    setSp(next)
  }

  // A view row overlays the view and opens it in the inspector.
  const overlayView = (id: string) => {
    const next = new URLSearchParams(sp)
    next.set('node', id)
    next.set('view', id)
    next.delete('focus')
    setSp(next)
  }

  const windowed = <T,>(items: T[], render: (t: T) => React.ReactNode) => (
    <>
      {items.slice(0, WINDOW).map(render)}
      {items.length > WINDOW && (
        <p className="muted" style={{ padding: '2px 12px', fontSize: 11 }}>
          showing {WINDOW} of {items.length}, refine the filter
        </p>
      )}
    </>
  )

  const g = graph.data

  return (
    <>
      <div className="wb-side-pad" style={{ paddingBottom: 4 }}>
        <input
          type="search"
          placeholder="filter"
          value={sp.get('q') ?? ''}
          onChange={(e) => setParam('q', e.target.value || null)}
        />
      </div>
      <div className="wb-side-tabs">
        {LISTS.map((t) => (
          <a
            key={t}
            href={`#${t}`}
            className={t === list ? 'active' : ''}
            onClick={(e) => {
              e.preventDefault()
              setParam('list', t)
            }}
          >
            {t}
          </a>
        ))}
      </div>
      {graph.error && <p className="error-inline wb-side-pad">{graph.error.message}</p>}
      {!g && !graph.error && <p className="muted wb-side-pad">loading…</p>}

      {g && list === 'entities' && (
        <>
          {tree.error && <p className="error-inline wb-side-pad">{tree.error.message}</p>}
          {!tree.data && !tree.error && <p className="muted wb-side-pad">building the tree…</p>}
          {tree.data && (
            <ContainmentTree
              tree={tree.data}
              graph={g}
              q={q}
              node={node}
              view={view}
              revIdx={revIdx}
              openAndFocus={openAndFocus}
              overlayView={overlayView}
            />
          )}
        </>
      )}

      {g && list === 'requirements' &&
        windowed(
          Object.entries(g.requirements)
            .filter(([id, r]) => !q || `${id} ${r.statement}`.toLowerCase().includes(q))
            .sort(([a], [b]) => a.localeCompare(b)),
          ([id, r]) => (
            <a
              key={id}
              className={`wb-list-row${node === id ? ' active' : ''}`}
              title={r.statement}
              {...pressable(() => openAndFocus(id))}
            >
              <span className={`mono ${verifyClass(rows[id]?.status)}`}>●</span>{' '}
              <span className="mono">{id}</span> <span className="sub">{r.statement}</span>
            </a>
          ),
        )}

      {list === 'views' && (
        <>
          {views.error && <p className="error-inline wb-side-pad">{views.error.message}</p>}
          {!views.data && !views.error && <p className="muted wb-side-pad">loading…</p>}
          {views.data && views.data.views.length === 0 && <p className="muted wb-side-pad">no views yet</p>}
          {(views.data?.views ?? []).length > 0 &&
            // Grouped by kind, default views marked, member count and limits state.
            [...new Set((views.data?.views ?? []).map((v) => v.kind))].sort().map((kind) => (
              <div key={kind}>
                <div className="wb-explorer-label" style={{ paddingTop: 6 }}>{kind}</div>
                {(views.data?.views ?? [])
                  .filter((v) => v.kind === kind)
                  .filter((v) => !q || `${v.id} ${v.title}`.toLowerCase().includes(q))
                  .map((v) => {
                    const over = v.limits.some((l) => l.over)
                    return (
                      <a
                        key={v.id}
                        className={`wb-list-row${node === v.id ? ' active' : ''}`}
                        title={`overlay ${v.id}`}
                        {...pressable(() => overlayView(v.id))}
                      >
                        {v.title}
                        {v.default && <span className="chip sev-none">default</span>}{' '}
                        <span className={`sub ${over ? 'v-stale' : ''}`}>
                          {v.members} members · {v.edges} edges{over ? ' · over limit' : ''}
                        </span>
                      </a>
                    )
                  })}
              </div>
            ))}
        </>
      )}

      {g && list === 'diagnostics' &&
        windowed(
          Object.entries(g.diagnostics)
            .filter(([, d]) => d.triage !== 'suppressed')
            .filter(([id, d]) => !q || `${id} ${d.rule} ${d.message}`.toLowerCase().includes(q))
            .sort(([a], [b]) => a.localeCompare(b)),
          ([id, d]) => (
            <a
              key={id}
              className={`wb-list-row${node === id ? ' active' : ''}`}
              title={d.message}
              {...pressable(() => openAndFocus(id))}
            >
              <span className={`mono sev-${d.severity}`}>{d.severity}</span>{' '}
              <span className="mono">{d.rule}</span> <span className="sub">{d.message}</span>
            </a>
          ),
        )}

      {list === 'coverage' && (
        <>
          {coverage.error && <p className="error-inline wb-side-pad">{coverage.error.message}</p>}
          {!coverage.data && !coverage.error && <p className="muted wb-side-pad">loading…</p>}
          {coverage.data && Object.keys(coverage.data).length === 0 && (
            <p className="muted wb-side-pad">no document has reconciled yet</p>
          )}
          {coverage.data &&
            Object.entries(coverage.data)
              .sort(([a], [b]) => a.localeCompare(b))
              .map(([doc, rec]) => {
                const secs = Object.keys(rec.sections)
                const covered = secs.filter(
                  (sid) => (rec.coverage ?? {})[sid]?.state === 'covered',
                ).length
                return (
                  <a
                    key={doc}
                    className="wb-list-row mono"
                    title="inspect the document's ties and focus it on the map"
                    {...pressable(() => {
                      const next = new URLSearchParams(sp)
                      next.set('node', `doc:${doc}`)
                      next.set('focus', `doc:${doc}`)
                      setSp(next)
                    })}
                  >
                    {doc}{' '}
                    <span className="sub">
                      {covered}/{secs.length} covered
                    </span>
                  </a>
                )
              })}
        </>
      )}
    </>
  )
}
