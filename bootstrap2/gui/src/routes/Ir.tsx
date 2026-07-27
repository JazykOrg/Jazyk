// IR: the graph browser. The viewer's cards served live, one text filter plus
// per-tab facets, all state in the URL.
import { useMemo } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { post, type Diagnostic, type Entity, type Graph, type Relationship, type Requirement, type VerifyRow } from '../lib/api'
import { useCoverage, useGraph, useMatrix } from '../lib/queries'
import NodeLink from '../components/NodeLink'
import SectionLink from '../components/SectionLink'
import { SevChip, VerifyChip } from '../components/Chip'
import './routes.css'

const TABS = ['entities', 'requirements', 'relationships', 'diagnostics', 'coverage'] as const
type Tab = (typeof TABS)[number]
const WINDOW = 200

// Reverse index: entity id -> requirement ids referencing it.
export function reverseIndex(graph: Graph): Map<string, string[]> {
  const m = new Map<string, string[]>()
  for (const [rid, r] of Object.entries(graph.requirements))
    for (const eid of r.entities ?? []) {
      const list = m.get(eid)
      if (list) list.push(rid)
      else m.set(eid, [rid])
    }
  return m
}

// Aggregate verification class for an entity from its requirements' statuses.
export function aggClass(reqIds: string[], rows: Record<string, VerifyRow>): string {
  let ok = 0
  let stale = false
  for (const id of reqIds) {
    const st = rows[id]?.status
    if (st === 'failing') return 'agg-bad'
    if (st?.startsWith('stale')) stale = true
    if (st === 'verified') ok++
  }
  if (stale) return 'agg-stale'
  if (ok > 0 && ok === reqIds.length) return 'agg-ok'
  return 'agg-none'
}

export function EntityCard({
  id,
  e,
  reqIds,
  rows,
}: {
  id: string
  e: Entity
  reqIds: string[]
  rows: Record<string, VerifyRow>
}) {
  return (
    <div className={`card ${aggClass(reqIds, rows)}`}>
      <h3>
        <NodeLink id={id} /> {e.name}
        {e.scope && e.scope !== 'public' && <span className="chip sev-none">{e.scope}</span>}
      </h3>
      {e.definition && <p style={{ margin: '4px 0' }}>{e.definition}</p>}
      {e.aliases && e.aliases.length > 0 && (
        <p className="muted" style={{ margin: '2px 0' }}>aka {e.aliases.join(', ')}</p>
      )}
      {(e.mentions ?? []).map((m, i) => (
        <p key={i} style={{ margin: '2px 0' }}>
          <SectionLink doc={m.doc} section={m.section} quote={m.quote} />{' '}
          <span className="muted">“{m.quote}”</span>
        </p>
      ))}
      {reqIds.length > 0 && (
        <p style={{ margin: '4px 0 0' }}>
          {reqIds.map((rid) => (
            <span key={rid} style={{ marginRight: 8 }}>
              <NodeLink id={rid} />
            </span>
          ))}
        </p>
      )}
    </div>
  )
}

export function RequirementCard({ id, r, row }: { id: string; r: Requirement; row?: VerifyRow }) {
  return (
    <div className="card">
      <h3>
        <NodeLink id={id} /> <VerifyChip status={row?.status ?? 'unverified'} />
        {row?.test?.kind && <span className="chip sev-none">{row.test.kind}</span>}
      </h3>
      <p style={{ margin: '4px 0' }}>{r.ears}</p>
      {row?.evidence && <p className="muted oneline" style={{ margin: '2px 0' }}>{row.evidence}</p>}
      {(r.entities ?? []).length > 0 && (
        <p style={{ margin: '2px 0' }}>
          {(r.entities ?? []).map((eid) => (
            <span key={eid} style={{ marginRight: 8 }}>
              <NodeLink id={eid} />
            </span>
          ))}
        </p>
      )}
      <p style={{ margin: '2px 0' }}>
        <SectionLink doc={r.source.doc} section={r.source.section} quote={r.source.quote} />{' '}
        <span className="muted">“{r.source.quote}”</span>
      </p>
      {(r.edges ?? []).map((ed, i) => (
        <p key={i} className="mono" style={{ margin: '2px 0' }}>
          <NodeLink id={ed.a} /> →{ed.type ?? 'related'}→ <NodeLink id={ed.b} />
        </p>
      ))}
    </div>
  )
}

export function RelationshipCard({ id, r }: { id: string; r: Relationship }) {
  return (
    <div className="card">
      <h3>
        <NodeLink id={id} /> <span className="chip sev-info">{r.type}</span>
      </h3>
      <p style={{ margin: '2px 0' }}>
        {r.members.map((m) => (
          <span key={m} style={{ marginRight: 8 }}>
            <NodeLink id={m} />
          </span>
        ))}
      </p>
      <p className="muted" style={{ margin: '2px 0' }}>
        from{' '}
        {r.requirements.map((q) => (
          <span key={q} style={{ marginRight: 8 }}>
            <NodeLink id={q} />
          </span>
        ))}
      </p>
    </div>
  )
}

export function DiagnosticCard({ id, d }: { id: string; d: Diagnostic }) {
  const qc = useQueryClient()
  const triage = (t: string | null) =>
    post(`/api/diagnostics/${encodeURIComponent(id)}/triage`, { triage: t }).then(() => {
      qc.invalidateQueries({ queryKey: ['graph'] })
      qc.invalidateQueries({ queryKey: ['status'] })
    })
  return (
    <div className="card">
      <h3>
        <NodeLink id={id} /> <SevChip severity={d.severity} />
        <span className="chip sev-none">{d.rule}</span>
        {d.lifecycle && d.lifecycle !== 'open' && <span className="chip v-ok">{d.lifecycle}</span>}
        {d.triage && <span className="chip sev-none">{d.triage}</span>}
      </h3>
      {(d.subjects ?? []).length > 0 && (
        <p style={{ margin: '2px 0' }}>
          {(d.subjects ?? []).map((sub) => (
            <span key={sub} style={{ marginRight: 8 }}>
              <NodeLink id={sub} />
            </span>
          ))}
        </p>
      )}
      <p style={{ margin: '4px 0' }}>{d.message}</p>
      {d.reasoning && (
        <details>
          <summary>reasoning</summary>
          <p className="muted">{d.reasoning}</p>
        </details>
      )}
      <p className="row" style={{ margin: '6px 0 0' }}>
        <button onClick={() => triage('acknowledged')}>acknowledge</button>
        <button onClick={() => triage('wontfix')}>wontfix</button>
        <button onClick={() => triage('suppressed')}>suppress</button>
        {d.triage && <button onClick={() => triage(null)}>clear</button>}
      </p>
    </div>
  )
}

function Facet({
  value,
  param,
  label,
}: {
  value: string
  param: string
  label?: string
}) {
  const [sp, setSp] = useSearchParams()
  const active = sp.get(param) === value
  return (
    <span
      className={`chip facet ${active ? '' : 'off'}`}
      onClick={() => {
        const next = new URLSearchParams(sp)
        if (active) next.delete(param)
        else next.set(param, value)
        setSp(next, { replace: true })
      }}
    >
      {label ?? value}
    </span>
  )
}

export default function Ir() {
  const { tab: tabParam } = useParams()
  const tab: Tab = (TABS as readonly string[]).includes(tabParam ?? '') ? (tabParam as Tab) : 'entities'
  const [sp, setSp] = useSearchParams()
  const q = (sp.get('q') ?? '').toLowerCase()

  const graph = useGraph()
  const matrix = useMatrix()
  const coverage = useCoverage()

  const rows = matrix.data?.rows ?? {}
  const revIdx = useMemo(() => (graph.data ? reverseIndex(graph.data) : new Map<string, string[]>()), [graph.data])

  if (graph.error)
    return (
      <p className="error-inline">
        {graph.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); graph.refetch() }}>retry</a>
      </p>
    )
  if (!graph.data) return <p className="muted">loading…</p>
  const g = graph.data

  const tabs = (
    <p className="row mono">
      {TABS.map((t) => (
        <Link key={t} to={`/ir/${t}`} style={t === tab ? { fontWeight: 700 } : undefined}>
          {t}
        </Link>
      ))}
    </p>
  )

  const filterInput = (
    <input
      type="search"
      placeholder="filter"
      value={sp.get('q') ?? ''}
      onChange={(e) => {
        const next = new URLSearchParams(sp)
        if (e.target.value) next.set('q', e.target.value)
        else next.delete('q')
        setSp(next, { replace: true })
      }}
    />
  )

  const windowed = <T,>(items: T[], render: (t: T) => React.ReactNode) => (
    <>
      {items.slice(0, WINDOW).map(render)}
      {items.length > WINDOW && (
        <p className="muted">
          showing {WINDOW} of {items.length}, refine the filter
        </p>
      )}
    </>
  )

  if (tab === 'entities') {
    const scopes = [...new Set(Object.values(g.entities).map((e) => e.scope ?? 'public'))].sort()
    const items = Object.entries(g.entities)
      .filter(([id, e]) => {
        const scope = sp.get('scope')
        if (scope && (e.scope ?? 'public') !== scope) return false
        if (!q) return true
        return `${id} ${e.name} ${e.definition ?? ''} ${(e.aliases ?? []).join(' ')}`.toLowerCase().includes(q)
      })
      .sort(([a], [b]) => a.localeCompare(b))
    return (
      <div>
        {tabs}
        <div className="filterbar">
          {filterInput}
          {scopes.map((s) => (
            <Facet key={s} value={s} param="scope" />
          ))}
        </div>
        {items.length === 0 && <p className="empty">no entities match</p>}
        {windowed(items, ([id, e]) => (
          <EntityCard key={id} id={id} e={e} reqIds={revIdx.get(id) ?? []} rows={rows} />
        ))}
      </div>
    )
  }

  if (tab === 'requirements') {
    const statuses = [...new Set(Object.values(rows).map((r) => r.status))].sort()
    const items = Object.entries(g.requirements)
      .filter(([id, r]) => {
        const st = sp.get('status')
        if (st && (rows[id]?.status ?? 'unverified') !== st) return false
        if (!q) return true
        return `${id} ${r.ears} ${(r.entities ?? []).join(' ')}`.toLowerCase().includes(q)
      })
      .sort(([a], [b]) => a.localeCompare(b))
    return (
      <div>
        {tabs}
        <div className="filterbar">
          {filterInput}
          {statuses.map((s) => (
            <Facet key={s} value={s} param="status" />
          ))}
        </div>
        {items.length === 0 && <p className="empty">no requirements match</p>}
        {windowed(items, ([id, r]) => (
          <RequirementCard key={id} id={id} r={r} row={rows[id]} />
        ))}
      </div>
    )
  }

  if (tab === 'relationships') {
    const types = [...new Set(Object.values(g.relationships).map((r) => r.type))].sort()
    const items = Object.entries(g.relationships)
      .filter(([id, r]) => {
        const ty = sp.get('type')
        if (ty && r.type !== ty) return false
        if (!q) return true
        return `${id} ${r.type} ${r.members.join(' ')}`.toLowerCase().includes(q)
      })
      .sort(([a], [b]) => a.localeCompare(b))
    return (
      <div>
        {tabs}
        <div className="filterbar">
          {filterInput}
          {types.map((t) => (
            <Facet key={t} value={t} param="type" />
          ))}
        </div>
        {items.length === 0 && <p className="empty">no relationships match</p>}
        {windowed(items, ([id, r]) => (
          <RelationshipCard key={id} id={id} r={r} />
        ))}
      </div>
    )
  }

  if (tab === 'diagnostics') {
    const visible = Object.entries(g.diagnostics).filter(([, d]) => d.triage !== 'suppressed')
    const suppressed = Object.keys(g.diagnostics).length - visible.length
    const sevs = [...new Set(visible.map(([, d]) => d.severity))].sort()
    const rules = [...new Set(visible.map(([, d]) => d.rule))].sort()
    const items = visible
      .filter(([id, d]) => {
        const sev = sp.get('sev')
        if (sev && d.severity !== sev) return false
        const rule = sp.get('rule')
        if (rule && d.rule !== rule) return false
        if (!q) return true
        return `${id} ${d.rule} ${d.message} ${(d.subjects ?? []).join(' ')}`.toLowerCase().includes(q)
      })
      .sort(([a], [b]) => a.localeCompare(b))
    return (
      <div>
        {tabs}
        <div className="filterbar">
          {filterInput}
          {sevs.map((s) => (
            <Facet key={s} value={s} param="sev" />
          ))}
          {rules.map((r) => (
            <Facet key={r} value={r} param="rule" />
          ))}
        </div>
        {items.length === 0 && <p className="empty">no diagnostics match</p>}
        {windowed(items, ([id, d]) => (
          <DiagnosticCard key={id} id={id} d={d} />
        ))}
        {suppressed > 0 && <p className="muted">{suppressed} suppressed hidden</p>}
      </div>
    )
  }

  // coverage
  if (coverage.error)
    return (
      <div>
        {tabs}
        <p className="error-inline">
          {coverage.error.message}{' '}
          <a href="#retry" onClick={(e) => { e.preventDefault(); coverage.refetch() }}>retry</a>
        </p>
      </div>
    )
  const cov = coverage.data
  return (
    <div>
      {tabs}
      {!cov && <p className="muted">loading…</p>}
      {cov && Object.keys(cov).length === 0 && <p className="empty">no documents reconciled yet</p>}
      {cov && (
        <table>
          <thead>
            <tr>
              <th>document</th>
              <th>covered</th>
              <th>non-normative</th>
              <th>unprocessed</th>
            </tr>
          </thead>
          <tbody>
            {Object.entries(cov)
              .sort(([a], [b]) => a.localeCompare(b))
              .map(([doc, rec]) => {
                const secs = Object.entries(rec.sections).sort((a, b) => a[1].order - b[1].order)
                const state = (sec: string) => rec.coverage[sec]?.state ?? 'unprocessed'
                const count = (want: string) => secs.filter(([sid]) => state(sid) === want).length
                return (
                  <tr key={doc}>
                    <td>
                      <details>
                        <summary className="mono">{doc}</summary>
                        {secs.map(([sid, sec]) => (
                          <p key={sid} style={{ margin: '2px 0' }}>
                            <span
                              className={`chip ${
                                state(sid) === 'covered'
                                  ? 'v-ok'
                                  : state(sid) === 'non-normative'
                                    ? 'sev-none'
                                    : 'v-stale'
                              }`}
                            >
                              {state(sid)}
                            </span>
                            <SectionLink doc={doc} section={sid}>
                              {sec.title || sid}
                            </SectionLink>
                          </p>
                        ))}
                      </details>
                    </td>
                    <td className="v-ok">{count('covered')}</td>
                    <td className="muted">{count('non-normative')}</td>
                    <td className="v-stale">{count('unprocessed')}</td>
                  </tr>
                )
              })}
          </tbody>
        </table>
      )}
    </div>
  )
}
