// The inspector: the detail pane for one selection, opened from anywhere,
// replacing nothing (docs/frontends/gui.md#inspector). Driven by ?node=; with no
// selection it shows the open center item's ties. Every rendered element walks to
// the sentence behind it: an arrow to its relationship, the relationship to its
// requirements, each requirement to its quote.
import { Link, useLocation, useSearchParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { get, type VerifyRow } from '../lib/api'
import { useContextPack, useExplain, useGraph, useJournal, useMatrix, useView } from '../lib/queries'
import { useDocDelivLinks } from '../lib/links'
import { delivHref } from '../lib/nav'
import { linkifyIds, useResolveId } from '../components/NodeLink'
import NodeLink from '../components/NodeLink'
import DiagramSvg from '../components/DiagramSvg'
import SectionLink from '../components/SectionLink'
import { VerifyChip } from '../components/Chip'
import { DiagnosticCard, ProvenanceLine, RelationshipCard } from '../components/Cards'
import { CriterionView, EntityView, RelationView, RequirementView } from '../components/NodeViews'
import { NameLink, Part, ProblemItem, StatusDot } from '../components/Present'
import { useNodeName } from '../lib/present'
import { useApp } from '../lib/store'
import { entryLabel } from '../lib/api'
import type { FileResp } from './DelivFile'
import { useEffect, useMemo, useState } from 'react'

// The journal entries that touched this node: from a fact, to the entry that
// landed it, to the goals that entry resolved and opened.
function JournalHits({ id }: { id: string }) {
  const journal = useJournal(200)
  const hits = (journal.data?.entries ?? []).filter((e) =>
    e.mutations.some((m) => JSON.stringify(m).includes(id)),
  )
  if (hits.length === 0) return null
  return (
    <>
      <h2>journal</h2>
      {hits.slice(0, 8).map((e) => (
        <div key={e.generation} style={{ margin: '2px 0' }}>
          <p className="oneline mono" style={{ margin: 0 }}>
            <Link to={`/history/${e.generation}`}>g{e.generation}</Link> · {entryLabel(e)}
          </p>
          {(e.resolved_goals ?? []).slice(0, 2).map((r) => (
            <p key={r.goal} className="muted oneline" style={{ margin: '0 0 0 12px' }}>
              ✓ {r.goal}: {r.justification}
            </p>
          ))}
        </div>
      ))}
    </>
  )
}

// The ripple from a target, fetched on demand; `?ripple=1` (a board card's
// ripple action) opens it at once.
function RipplePane({ target }: { target: string }) {
  const [sp] = useSearchParams()
  const [open, setOpen] = useState(sp.get('ripple') === '1')
  const q = useQuery({
    queryKey: ['ripple', target],
    queryFn: () => get<{ text: string }>(`/api/ripple?target=${encodeURIComponent(target)}`),
    enabled: open && target !== '',
    staleTime: 5_000,
  })
  return (
    <>
      <p className="row" style={{ margin: '4px 0' }}>
        <button onClick={() => setOpen(!open)}>{open ? 'hide ripple' : 'ripple'}</button>
      </p>
      {open && q.isLoading && <p className="muted">walking the journal…</p>}
      {open && q.error && <p className="muted">no cascade touches this target</p>}
      {open && q.data && <pre className="pack">{q.data.text}</pre>}
    </>
  )
}

// The goal as GET /api/explain returns it beside its explanation text.
interface ExplainedGoal {
  kind?: string
  class?: string
  mandatory?: boolean
  target?: string
  unit?: string
  change?: unknown
  cause?: { generation?: number; mutation?: number; via?: string }
  state?: string | { blocked?: { on: string }; failed?: { reason: string } }
  hints?: string[]
  ready?: boolean
  gated?: boolean
  blockedBy?: string
  tier?: number | null
  batch?: string
}

function goalStateLine(s: ExplainedGoal['state']): string {
  if (!s) return ''
  if (typeof s === 'string') return s
  if (s.blocked) return `blocked on ${s.blocked.on}`
  if (s.failed) return `failed: ${s.failed.reason}`
  return ''
}

// The whole-build report: the ripple DAG rooted at a run's first generation, the
// cost beside it, and the parked and failed goals with their reasons
// (docs/frontends/gui.md#activity). Addressed as `?node=g<N>`.
interface RippleTree {
  sessions?: number
  tokens?: number
  recomputes?: number
  by_kind?: Record<string, { sessions?: number; tokens?: number } | number>
  verdict?: string
  parked?: { id: string; kind?: string; target?: string }[]
  failed?: { goal?: { id: string } | string; id?: string; reason?: string }[]
}

function BuildReport({ generation }: { generation: number }) {
  const q = useQuery({
    queryKey: ['ripple', 'generation', generation],
    queryFn: () => get<{ text: string; tree: RippleTree }>(`/api/ripple?generation=${generation}`),
    staleTime: 5_000,
  })
  if (q.isLoading) return <p className="muted">walking the journal…</p>
  if (q.error) return <p className="error-inline">{q.error.message}</p>
  if (!q.data) return null
  const t = q.data.tree
  const failed = (t.failed ?? []).map((f) => ({
    id: typeof f.goal === 'string' ? f.goal : (f.goal?.id ?? f.id ?? ''),
    reason: f.reason ?? '',
  }))
  return (
    <>
      <div className="card">
        <h3>
          build from <Link to={`/history/${generation}`}>g{generation}</Link>
        </h3>
        {t.verdict && <p style={{ margin: '2px 0' }}>{t.verdict}</p>}
        <p className="muted mono" style={{ margin: '2px 0' }}>
          {t.sessions ?? 0} sessions · {Math.round((t.tokens ?? 0) / 1000)}k tok · {t.recomputes ?? 0} recomputes
        </p>
        {Object.entries(t.by_kind ?? {}).map(([k, v]) => (
          <p key={k} className="muted mono" style={{ margin: 0, paddingLeft: 8 }}>
            {k}: {typeof v === 'number' ? v : `${v.sessions ?? 0} · ${Math.round((v.tokens ?? 0) / 1000)}k`}
          </p>
        ))}
      </div>
      {(t.parked ?? []).length > 0 && (
        <>
          <h2>parked ({(t.parked ?? []).length})</h2>
          {(t.parked ?? []).map((p) => (
            <p key={p.id} style={{ margin: '2px 0' }}>
              <NodeLink id={p.id} />
            </p>
          ))}
        </>
      )}
      {failed.length > 0 && (
        <>
          <h2>failed ({failed.length})</h2>
          {failed.map((f, i) => (
            <p key={i} style={{ margin: '2px 0' }}>
              <NodeLink id={f.id} /> <span className="v-bad">{f.reason}</span>
            </p>
          ))}
        </>
      )}
      <h2>ripple</h2>
      <pre className="pack">{q.data.text}</pre>
    </>
  )
}

// A goal from a board card: kind, class, target, change, cause, state, hints, and
// its explanation (the change record, the readiness sentence, what blocks it).
// Mirrors docs/frontends/gui.md#inspector.
function GoalDetail({ id }: { id: string }) {
  const explain = useExplain(id)
  if (explain.isLoading) return <p className="muted">explaining…</p>
  if (explain.error) return <p className="error-inline">{explain.error.message}</p>
  if (!explain.data)
    return (
      <p className="muted">
        the board holds no goal <span className="mono">{id}</span>; it resolved, or its change was undone
      </p>
    )
  const g = (explain.data.goal ?? {}) as ExplainedGoal
  const change = g.change
  const changeText =
    change == null ? '' : typeof change === 'string' ? change : JSON.stringify(change)
  return (
    <>
      {g.kind && (
        <div className="card">
          <h3>
            {g.kind}
            {g.class && <span className="chip sev-none">{g.class}</span>}
            {g.mandatory !== undefined && (
              <span className={`chip ${g.mandatory ? 'sev-warning' : 'sev-none'}`}>
                {g.mandatory ? 'mandatory' : 'optional'}
              </span>
            )}
            {g.gated && <span className="chip sev-info">gated</span>}
          </h3>
          {g.target && (
            <p style={{ margin: '2px 0' }}>
              target <NodeLink id={g.target} />
              {g.unit && <span className="muted"> ({g.unit})</span>}
            </p>
          )}
          {g.state !== undefined && (
            <p style={{ margin: '2px 0' }}>
              state <span className="mono">{goalStateLine(g.state)}</span>
              {g.ready ? <span className="chip v-ok">ready</span> : null}
              {g.batch && <span className="chip v-stale">in session {g.batch}</span>}
            </p>
          )}
          {g.blockedBy && <p className="muted" style={{ margin: '2px 0' }}>{g.blockedBy}</p>}
          {changeText && <p className="muted mono" style={{ margin: '2px 0' }}>change: {changeText}</p>}
          {g.cause && (
            <p className="muted mono" style={{ margin: '2px 0' }}>
              cause:{' '}
              {g.cause.generation !== undefined ? (
                <Link to={`/history/${g.cause.generation}`}>g{g.cause.generation}</Link>
              ) : null}{' '}
              {g.cause.mutation !== undefined ? `#${g.cause.mutation}` : ''}
              {g.cause.via ? ` via ${g.cause.via}` : ''}
            </p>
          )}
          {(g.hints ?? []).length > 0 && (
            <ul className="muted" style={{ margin: '2px 0', paddingLeft: 18 }}>
              {(g.hints ?? []).map((h, i) => (
                <li key={i}>{linkifyIds(h)}</li>
              ))}
            </ul>
          )}
        </div>
      )}
      <h2>explanation</h2>
      <pre className="pack">{explain.data.text}</pre>
      {g.target && <RipplePane target={g.target} />}
    </>
  )
}

// A diagram: its members by name, a flow's steps in order; the query, the
// collapse set, the exclusions, the limits, and the arrows are Developer details
// (docs/frontends/gui.md#model).
function ViewDetailPane({ id }: { id: string }) {
  const view = useView(id)
  const dev = useApp((a) => a.dev)
  if (view.isLoading) return <p className="muted">loading the diagram…</p>
  if (view.error) return <p className="error-inline">{view.error.message}</p>
  const v = view.data
  if (!v) return null
  const entities = v.members.filter((m) => m.node === 'entity' && !m.hidden)
  const steps = v.members.filter((m) => m.node === 'requirement')
  return (
    <div className="node-view">
      {entities.length > 0 && (
        <Part title="What it shows">
          <div className="member-list">
            {entities.map((m) => (
              <NameLink key={m.id} id={m.id} />
            ))}
          </div>
        </Part>
      )}
      {steps.length > 0 && (
        <Part title="Steps">
          <ol className="step-list">
            {steps.map((m) => (
              <li key={m.id}>
                <NameLink id={m.id} />
              </li>
            ))}
          </ol>
        </Part>
      )}
      {dev && (
        <details className="dev-details">
          <summary>Developer details</summary>
          <p className="mono">
            {id} {v.kind}
            {v.default && ' default'}
          </p>
          {v.default && (
            <p className="muted" style={{ margin: '2px 0' }}>
              editing this view clears its default mark; the recompute leaves it alone from that commit on
            </p>
          )}
          <p style={{ margin: '4px 0' }}>
            <Link to={`/dev/map?view=${encodeURIComponent(id)}`}>overlay on the map →</Link>
          </p>
          {v.provenance && <ProvenanceLine p={v.provenance} />}
          {(v.limits ?? []).map((l) => (
            <p key={l.limit} className={`mono ${l.overHard ? 'v-bad' : l.over ? 'v-stale' : 'muted'}`} style={{ margin: '1px 0' }}>
              {l.limit}: {l.count} / {l.soft} soft, {l.hard} hard
            </p>
          ))}
          {v.members.some((m) => m.hidden) && (
            <>
              <h2>collapsed away</h2>
              {v.members.filter((m) => m.hidden).map((m) => (
                <p key={m.id} style={{ margin: '2px 0' }}>
                  <NodeLink id={m.id} />
                </p>
              ))}
            </>
          )}
          {(v.children ?? []).length > 0 && (
            <>
              <h2>levels below</h2>
              {v.children.map((c) => (
                <p key={c.member} style={{ margin: '2px 0' }}>
                  <NodeLink id={c.member} />{' '}
                  <Link className="mono" to={`/dev/map?view=${encodeURIComponent(c.view)}&node=${encodeURIComponent(c.view)}`}>
                    {c.view} ↓
                  </Link>
                </p>
              ))}
            </>
          )}
          {(v.excluded ?? []).length > 0 && (
            <>
              <h2>excluded</h2>
              {v.excluded.map((x) => (
                <p key={x.id} style={{ margin: '2px 0' }}>
                  <NodeLink id={x.id} /> <span className="muted">{x.note}</span>
                </p>
              ))}
            </>
          )}
          {v.query && <pre className="pack">{JSON.stringify(v.query, null, 2)}</pre>}
          {(v.arrows ?? []).map((a, i) => (
            <details key={i} style={{ margin: '2px 0' }}>
              <summary className="mono">
                <NodeLink id={a.a} /> →{a.type}→ <NodeLink id={a.b} />
                {a.lifted && <span className="chip sev-none">lifted: {a.count}</span>}
              </summary>
              {a.concrete.map((c, j) => (
                <p key={j} className="mono" style={{ margin: '1px 0 1px 12px' }}>
                  <NodeLink id={c.a} /> →{c.type}→ <NodeLink id={c.b} />{' '}
                  {c.requirements.map((q) => (
                    <span key={q} style={{ marginRight: 4 }}>
                      <NodeLink id={q} />
                    </span>
                  ))}
                </p>
              ))}
            </details>
          ))}
          {v.renderError && <p className="muted">renderer: {v.renderError}</p>}
        </details>
      )}
    </div>
  )
}

export function NodeDetail({ id, page }: { id: string; page?: boolean }) {
  const graph = useGraph()
  const matrix = useMatrix()
  const isEntity = id.startsWith('ent:')
  const dev = useApp((a) => a.dev)
  // The loaded context is a Developer detail; nothing fetches it otherwise.
  const pack = useContextPack(isEntity && dev ? id : '')
  if (graph.error) return <p className="error-inline">{graph.error.message}</p>
  if (!graph.data) return <p className="muted">loading…</p>
  const g = graph.data
  const rows = matrix.data?.rows ?? {}

  if (id.startsWith('g:')) return <GoalDetail id={id} />
  if (/^g[0-9]+$/.test(id)) return <BuildReport generation={Number(id.slice(1))} />
  if (id.startsWith('view:')) return <ViewDetailPane id={id} />

  const machine = g.stateMachines?.[id]
  if (machine) {
    // The machine's open checks: the diagnostics naming it or its subject.
    const checks = Object.entries(g.diagnostics).filter(
      ([, d]) =>
        (d.lifecycle ?? 'open') === 'open' &&
        d.triage !== 'suppressed' &&
        (d.subjects ?? []).some((s) => s === id || s === machine.subject),
    )
    return (
      <>
        <div className="card">
          <h3>
            <NodeLink id={id} /> <span className="chip sev-info">state machine</span>
          </h3>
          <p style={{ margin: '2px 0' }}>
            subject <NodeLink id={machine.subject} />
          </p>
          <p className="mono" style={{ margin: '2px 0' }}>
            states: {machine.states.join(', ')}
            {machine.initial && <span className="muted"> (initial: {machine.initial})</span>}
            {!machine.initial && <span className="muted"> (no initial state)</span>}
          </p>
          {machine.transitions.length === 0 && <p className="muted">no transitions</p>}
          {machine.transitions.map((t, i) => (
            <p key={i} className="mono" style={{ margin: '1px 0' }}>
              {t.from} → {t.to}
              {t.trigger && <span className="muted"> on {t.trigger}</span>}
              {t.guard && <span className="muted"> [{t.guard}]</span>}
              {t.requirements.map((r) => (
                <span key={r}>
                  {' '}
                  <NodeLink id={r} />
                </span>
              ))}
            </p>
          ))}
        </div>
        {checks.length > 0 && (
          <>
            <h2>checks</h2>
            {checks.map(([did, d]) => (
              <DiagnosticCard key={did} id={did} d={d} />
            ))}
          </>
        )}
      </>
    )
  }

  const entity = g.entities[id]
  if (entity) {
    const rels = Object.entries(g.relationships).filter(([, r]) => r.members.includes(id))
    const views = Object.entries(g.views ?? {}).filter(([, v]) => (v.members ?? []).includes(id))
    return (
      <EntityView
        id={id}
        graph={g}
        rows={rows}
        page={page}
        devExtra={
          <>
            {views.length > 0 && (
              <>
                <h2>views it is drawn in</h2>
                {views.map(([vid, v]) => (
                  <p key={vid} style={{ margin: '2px 0' }}>
                    <NodeLink id={vid} /> <Link to={`/dev/map?view=${encodeURIComponent(vid)}`}>overlay</Link>
                    {v.default && <span className="chip sev-none">default</span>}
                  </p>
                ))}
              </>
            )}
            {rels.length > 0 && (
              <>
                <h2>relationships</h2>
                {rels.map(([rid, r]) => (
                  <RelationshipCard key={rid} id={rid} r={r} />
                ))}
              </>
            )}
            {pack.data && (
              <>
                <h2>loaded</h2>
                <pre className="pack">{pack.data.pack}</pre>
              </>
            )}
            <JournalHits id={id} />
            <RipplePane target={id} />
          </>
        }
      />
    )
  }

  const req = g.requirements[id]
  if (req) {
    return (
      <RequirementView
        id={id}
        graph={g}
        row={rows[id]}
        page={page}
        devExtra={
          <>
            <JournalHits id={id} />
            <RipplePane target={id} />
          </>
        }
      />
    )
  }

  if (g.criteria?.[id]) return <CriterionView id={id} graph={g} />

  const rel = g.relationships[id]
  if (rel) return <RelationView id={id} r={rel} />

  const diag = g.diagnostics[id]
  if (diag) {
    // A suppressed diagnostic still opens (a link brought the reader here); the
    // card's triage actions can clear the suppression.
    return (
      <>
        {diag.triage === 'suppressed' && <p className="muted">This problem is hidden; it counts nowhere.</p>}
        {dev ? <DiagnosticCard id={id} d={diag} /> : <ProblemItem id={id} d={diag} />}
      </>
    )
  }

  return (
    <p className="muted">
      no node with id <span className="mono">{id}</span> in the graph; it may have been deleted, merged
      away, or never existed
    </p>
  )
}

// The ties of the open deliverable file: owners, sites, lost sites flagged.
function FileTies({ path }: { path: string }) {
  const fileQ = useQuery({
    queryKey: ['deliverable', 'file', path],
    queryFn: () => get<FileResp>(`/api/deliverable/file?path=${encodeURIComponent(path)}`),
    staleTime: 5_000,
  })
  const matrix = useMatrix()
  const links = useDocDelivLinks()
  const rows = matrix.data?.rows ?? {}
  const f = fileQ.data
  if (fileQ.error) return <p className="error-inline">{fileQ.error.message}</p>
  if (!f) return <p className="muted">loading…</p>
  const sites = f.sites ?? []
  const lost = sites.filter((s) => s.line === null)
  const docs = [...(links.fileToDocs.get(path) ?? [])]
  return (
    <>
      <h2>entities</h2>
      {f.owners.entities.length === 0 && <p className="muted">none</p>}
      {f.owners.entities.map((slug) => (
        <p key={slug} style={{ margin: '2px 0' }}>
          {/* The ledger keys entities by slug; the graph id carries the prefix. */}
          <NodeLink id={`ent:${slug}`} />
        </p>
      ))}
      <h2>requirements</h2>
      {f.owners.requirements.length === 0 && <p className="muted">none</p>}
      {f.owners.requirements.map((id) => (
        <p key={id} style={{ margin: '2px 0' }}>
          <NodeLink id={id} /> <VerifyChip status={rows[id]?.status ?? 'unverified'} />
        </p>
      ))}
      <h2>tests</h2>
      {f.owners.tests.length === 0 && <p className="muted">none</p>}
      {f.owners.tests.map((id) => (
        <p key={id} style={{ margin: '2px 0' }}>
          <NodeLink id={id} /> <VerifyChip status={rows[id]?.status ?? 'unverified'} />
        </p>
      ))}
      {lost.length > 0 && (
        <>
          <h2>lost sites</h2>
          {lost.map((s, i) => (
            <p key={i} style={{ margin: '2px 0' }}>
              <NodeLink id={s.requirement} /> <span className="v-bad">site lost</span>
              {!s.exists && <span className="v-bad"> (requirement gone)</span>}
            </p>
          ))}
        </>
      )}
      {docs.length > 0 && (
        <>
          <h2>from documents</h2>
          {docs.map((d) => (
            <p key={d} style={{ margin: '2px 0' }}>
              <SectionLink doc={d} />
            </p>
          ))}
        </>
      )}
    </>
  )
}

// The open document's ties: the deliverable files its requirements produce.
function DocTies({ path }: { path: string }) {
  const links = useDocDelivLinks()
  const graph = useGraph()
  const files = [...(links.docToFiles.get(path) ?? [])]
  const reqs = Object.entries(graph.data?.requirements ?? {}).filter(
    ([, r]) => r.source?.doc === path,
  )
  const matrix = useMatrix()
  const rows = matrix.data?.rows ?? {}
  return (
    <>
      <h2>Requirements in this document</h2>
      {reqs.length === 0 && <p className="muted">None found yet. A sync reads them out of the text.</p>}
      <div className="req-list">
        {reqs.slice(0, 60).map(([rid]) => (
          <div key={rid} className="req-row">
            <StatusDot status={rows[rid]?.status} />
            <NameLink id={rid} />
          </div>
        ))}
      </div>
      <h2>Built in</h2>
      {files.length === 0 && <p className="muted">Not built yet.</p>}
      {files.map((p) => (
        <p key={p} style={{ margin: '2px 0' }}>
          <Link className="mono" to={delivHref(p)}>
            {p}
          </Link>
        </p>
      ))}
    </>
  )
}

// The pane's title: what kind of thing is open, its id only in Developer mode.
function InspectorTitle({ node, fallback }: { node: string | null; fallback: string }) {
  const dev = useApp((a) => a.dev)
  const name = useNodeName()
  if (!node) return <span>{fallback.split('/').pop()}</span>
  const kind = node.startsWith('ent:')
    ? 'Entity'
    : node.startsWith('req:')
      ? 'Requirement'
      : node.startsWith('crit:')
        ? 'Acceptance criterion'
        : node.startsWith('view:')
          ? 'Diagram'
          : node.startsWith('diag:')
            ? 'Problem'
            : node.startsWith('rel:')
              ? name(node)
              : node
  return (
    <span>
      {kind}
      {dev && <span className="mono muted"> {node}</span>}
    </span>
  )
}

export default function Inspector({
  node: selected,
  openNode,
  close,
}: {
  node: string | null
  openNode: (id: string) => void
  close: () => void
}) {
  const loc = useLocation()
  const [sp] = useSearchParams()
  // The explorer's position opens the inspector on its card when nothing else is
  // selected, so a shared `?entity=` URL lands on the walk (docs/frontends/gui.md#explore).
  const node = selected ?? sp.get('entity')
  const resolved = useResolveId(node ?? '')
  // Escape closes the pane, the same as its ✕, unless a field has the focus.
  useEffect(() => {
    if (!node) return
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null
      if (e.key !== 'Escape') return
      if (t && ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName)) return
      // The command palette owns Escape while it is open.
      if (document.querySelector('.palette')) return
      close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [node, close])
  void openNode

  // Contextual fallback: the open center item's ties when nothing is selected.
  const docPath = loc.pathname.startsWith('/docs/')
    ? decodeURIComponent(loc.pathname.slice('/docs/'.length))
    : ''
  const delivPath = loc.pathname.startsWith('/product/files/')
    ? decodeURIComponent(loc.pathname.slice('/product/files/'.length))
    : ''

  if (!node && !docPath && !delivPath) return null

  // The map's document and file nodes inspect as their ties, with an open link.
  const pseudoDoc = node?.startsWith('doc:') ? node.slice(4) : ''
  const pseudoFile = node?.startsWith('file:') ? node.slice(5) : ''

  return (
    <aside className="wb-inspector">
      <div className="wb-inspector-head">
        <InspectorTitle node={node ? resolved : null} fallback={docPath || delivPath} />
        {node ? (
          <button onClick={close} title="close (Escape)" aria-label="close the inspector">
            ✕
          </button>
        ) : null}
      </div>
      {pseudoDoc ? (
        <>
          <p style={{ margin: '2px 0' }}>
            <SectionLink doc={pseudoDoc}>open in the editor →</SectionLink>
          </p>
          <DocTies path={pseudoDoc} />
        </>
      ) : pseudoFile ? (
        <>
          <p style={{ margin: '2px 0' }}>
            <Link className="mono" to={delivHref(pseudoFile)}>
              open the file →
            </Link>
          </p>
          <FileTies path={pseudoFile} />
        </>
      ) : node ? (
        <NodeDetail id={resolved} />
      ) : delivPath ? (
        <FileTies path={delivPath} />
      ) : (
        <DocTies path={docPath} />
      )}
    </aside>
  )
}
