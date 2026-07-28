// The node cards: the viewer's card vocabulary served live, shared by the
// inspector and the graph sidebar.
import { useQueryClient } from '@tanstack/react-query'
import { post, type Diagnostic, type Entity, type Graph, type Relationship, type Requirement, type VerifyRow } from '../lib/api'
import NodeLink from './NodeLink'
import SectionLink from './SectionLink'
import { SevChip, VerifyChip } from './Chip'

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
