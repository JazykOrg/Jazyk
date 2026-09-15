// The node cards: the viewer's card vocabulary served live, shared by the
// inspector and the graph sidebar.
import { useQueryClient } from '@tanstack/react-query'
import {
  post,
  type Diagnostic,
  type Entity,
  type Graph,
  type Provenance,
  type Relationship,
  type Requirement,
  type VerifyRow,
} from '../lib/api'
import NodeLink from './NodeLink'
import SectionLink from './SectionLink'
import { SevChip, VerifyChip } from './Chip'
import FactField from './FactField'

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

// One line naming a provenance, whichever kind it carries; a quote opens the editor
// at the quote, a derivation walks to its upstream nodes.
export function ProvenanceLine({ p }: { p?: Provenance }) {
  if (!p) return null
  if (p.quote)
    return (
      <p style={{ margin: '2px 0' }}>
        <SectionLink doc={p.quote.doc} section={p.quote.section} quote={p.quote.quote} />{' '}
        <span className="muted">“{p.quote.quote}”</span>
      </p>
    )
  if (p.derived)
    return (
      <p style={{ margin: '2px 0' }}>
        <span className="chip sev-info">derived</span>{' '}
        {p.derived.from.map((f) => (
          <span key={f} style={{ marginRight: 6 }}>
            <NodeLink id={f} />
          </span>
        ))}
        <span className="muted"> {p.derived.reasoning}</span>
      </p>
    )
  if (p.decree)
    return (
      <p style={{ margin: '2px 0' }}>
        <span className="chip sev-warning">decree</span>{' '}
        <span className="muted">
          by {p.decree.author} at {p.decree.at}
          {p.decree.note ? `: ${p.decree.note}` : ''}
        </span>
      </p>
    )
  return null
}

export function EntityCard({
  id,
  e,
  reqIds,
  rows,
  editable,
}: {
  id: string
  e: Entity
  reqIds: string[]
  rows: Record<string, VerifyRow>
  editable?: boolean
}) {
  return (
    <div className={`card ${aggClass(reqIds, rows)}`}>
      <h3>
        <NodeLink id={id} /> {e.name}
        {e.stereotype && <span className="chip sev-info">«{e.stereotype}»</span>}
        {e.scope && e.scope !== 'public' && <span className="chip sev-none">{e.scope}</span>}
      </h3>
      <p style={{ margin: '4px 0' }}>
        {editable ? (
          <FactField id={id} field="definition" value={e.definition ?? ''} multiline label="definition" />
        ) : (
          e.definition
        )}
      </p>
      {e.parent && (
        <p style={{ margin: '2px 0' }}>
          <span className="muted">part of</span> <NodeLink id={e.parent} />
        </p>
      )}
      {(e.attributes ?? []).length > 0 && (
        <div style={{ margin: '2px 0' }}>
          {(e.attributes ?? []).map((a) => (
            <p key={a.name} className="mono" style={{ margin: '1px 0' }}>
              {a.name}
              {a.type ? `: ${a.type}` : ''}
              {a.value !== undefined && (
                <>
                  {' = '}
                  {editable ? (
                    <FactField id={id} field={`attributes.${a.name}.value`} value={a.value ?? ''} label={a.name} />
                  ) : (
                    a.value
                  )}
                </>
              )}
            </p>
          ))}
        </div>
      )}
      {e.aliases && e.aliases.length > 0 && (
        <p className="muted" style={{ margin: '2px 0' }}>aka {e.aliases.join(', ')}</p>
      )}
      {e.provenance && <ProvenanceLine p={e.provenance} />}
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

export function RequirementCard({
  id,
  r,
  row,
  editable,
}: {
  id: string
  r: Requirement
  row?: VerifyRow
  editable?: boolean
}) {
  return (
    <div className="card">
      <h3>
        <NodeLink id={id} /> <VerifyChip status={row?.status ?? 'unverified'} />
        {row?.test?.kind && <span className="chip sev-none">{row.test.kind}</span>}
      </h3>
      <p style={{ margin: '4px 0' }}>
        {editable ? (
          <FactField id={id} field="statement" value={r.statement} multiline label="statement" />
        ) : (
          r.statement
        )}
      </p>
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
      {r.source && (
        <p style={{ margin: '2px 0' }}>
          <SectionLink doc={r.source.doc} section={r.source.section} quote={r.source.quote} />{' '}
          <span className="muted">“{r.source.quote}”</span>
        </p>
      )}
      {!r.source && <ProvenanceLine p={r.provenance} />}
      {(r.edges ?? []).map((ed, i) => (
        <p key={i} className="mono" style={{ margin: '2px 0' }}>
          <NodeLink id={ed.a} /> →{ed.type ?? 'related'}→ <NodeLink id={ed.b} />
          {ed.cardinality && <span className="muted"> [{ed.cardinality}]</span>}
        </p>
      ))}
      {r.transition && (
        <p className="mono" style={{ margin: '2px 0' }}>
          <NodeLink id={r.transition.subject} />: {r.transition.from} → {r.transition.to}
          {r.transition.trigger && <span className="muted"> on {r.transition.trigger}</span>}
          {r.transition.guard && <span className="muted"> [{r.transition.guard}]</span>}
        </p>
      )}
      {(r.facets ?? []).length > 0 && (
        <p style={{ margin: '2px 0' }}>
          {(r.facets ?? []).map((f, i) => (
            <span key={i} className="chip sev-none" title={f.reasoning} style={{ marginRight: 4 }}>
              {f.facet}
              {f.measure ? ` (${f.measure})` : ''}
            </span>
          ))}
        </p>
      )}
    </div>
  )
}

// The relationship card: the members and the contribution groups, each direction
// and type with the requirements behind it: the justification walk's first step.
export function RelationshipCard({ id, r }: { id: string; r: Relationship }) {
  return (
    <div className="card">
      <h3>
        <NodeLink id={id} />
      </h3>
      <p style={{ margin: '2px 0' }}>
        {r.members.map((m) => (
          <span key={m} style={{ marginRight: 8 }}>
            <NodeLink id={m} />
          </span>
        ))}
      </p>
      {(r.contributions ?? []).map((c, i) => (
        <p key={i} style={{ margin: '2px 0' }}>
          <span className="mono">
            <NodeLink id={c.a} /> →{c.type}→ <NodeLink id={c.b} />
            {c.cardinality && <span className="muted"> [{c.cardinality}]</span>}
          </span>{' '}
          <span className="muted">
            from{' '}
            {c.requirements.map((q) => (
              <span key={q} style={{ marginRight: 6 }}>
                <NodeLink id={q} />
              </span>
            ))}
          </span>
        </p>
      ))}
    </div>
  )
}

export function DiagnosticCard({ id, d }: { id: string; d: Diagnostic }) {
  const qc = useQueryClient()
  const triage = (t: string | null) =>
    post(`/api/diagnostics/${encodeURIComponent(id)}/triage`, { triage: t }).then(() => {
      qc.invalidateQueries({ queryKey: ['graph'] })
      qc.invalidateQueries({ queryKey: ['status'] })
      qc.invalidateQueries({ queryKey: ['board'] })
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
      {d.prompt && <p className="muted" style={{ margin: '2px 0' }}>Q: {d.prompt.question}</p>}
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
