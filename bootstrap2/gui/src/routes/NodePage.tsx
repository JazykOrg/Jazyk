// Universal node resolver: /n/:id renders the full card for whatever the id is,
// plus context, backlinks, and verification for entities.
import { Link, useParams } from 'react-router'
import { useMemo } from 'react'
import type { VerifyRow } from '../lib/api'
import { useContextPack, useGraph, useJournal, useMatrix } from '../lib/queries'
import NodeLink, { useResolveId } from '../components/NodeLink'
import SectionLink from '../components/SectionLink'
import { VerifyChip } from '../components/Chip'
import {
  DiagnosticCard,
  EntityCard,
  RelationshipCard,
  RequirementCard,
  aggClass,
  reverseIndex,
} from './Ir'
import './routes.css'

function VerifyLine({ id, row }: { id: string; row?: VerifyRow }) {
  return (
    <p style={{ margin: '2px 0' }}>
      <NodeLink id={id} /> <VerifyChip status={row?.status ?? 'unverified'} />
      {row?.test?.kind && <span className="chip sev-none">{row.test.kind}</span>}
      {row?.test?.run && <span className="mono muted"> {row.test.run}</span>}
      {row?.reason && <span className="muted"> · {row.reason}</span>}
      {row?.lastRun && <span className="muted"> · {row.lastRun}</span>}
      {row?.evidence && <span className="muted oneline"> · {row.evidence}</span>}
    </p>
  )
}

export default function NodePage() {
  const { id: raw } = useParams()
  const id = useResolveId(decodeURIComponent(raw ?? ''))
  const graph = useGraph()
  const matrix = useMatrix()
  const journal = useJournal(200)
  const isEntity = id.startsWith('ent:')
  const pack = useContextPack(id)

  const revIdx = useMemo(
    () => (graph.data ? reverseIndex(graph.data) : new Map<string, string[]>()),
    [graph.data],
  )

  if (graph.error)
    return (
      <p className="error-inline">
        {graph.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); graph.refetch() }}>retry</a>
      </p>
    )
  if (!graph.data) return <p className="muted">loading…</p>
  const g = graph.data
  const rows = matrix.data?.rows ?? {}

  const journalHits = (journal.data?.entries ?? []).filter((e) =>
    e.mutations.some((m) => JSON.stringify(m).includes(id)),
  )

  const entity = g.entities[id]
  if (entity) {
    const reqIds = revIdx.get(id) ?? []
    const rels = Object.entries(g.relationships).filter(([, r]) => r.members.includes(id))
    const diags = Object.entries(g.diagnostics).filter(
      ([, d]) => (d.subjects ?? []).includes(id) && d.triage !== 'suppressed',
    )
    return (
      <div>
        <h1 className="mono">{id}</h1>
        <p>
          <Link to={`/map?focus=${encodeURIComponent(id)}`}>open in map</Link>
        </p>
        <EntityCard id={id} e={entity} reqIds={reqIds} rows={rows} />

        <h2>context pack</h2>
        {pack.error && <p className="error-inline">{pack.error.message}</p>}
        {pack.data ? <pre className="pack">{pack.data.pack}</pre> : <p className="muted">loading…</p>}

        <h2>verification</h2>
        {reqIds.length === 0 && <p className="muted">no requirements reference this entity</p>}
        {reqIds.length > 0 && (
          <div className={`card ${aggClass(reqIds, rows)}`}>
            {reqIds.map((rid) => (
              <VerifyLine key={rid} id={rid} row={rows[rid]} />
            ))}
          </div>
        )}

        {rels.length > 0 && (
          <>
            <h2>relationships</h2>
            {rels.map(([rid, r]) => (
              <RelationshipCard key={rid} id={rid} r={r} />
            ))}
          </>
        )}

        {diags.length > 0 && (
          <>
            <h2>diagnostics</h2>
            {diags.map(([did, d]) => (
              <DiagnosticCard key={did} id={did} d={d} />
            ))}
          </>
        )}

        <h2>journal</h2>
        {journalHits.length === 0 && <p className="muted">no recent changesets touch this node</p>}
        {journalHits.map((e) => (
          <p key={e.generation} className="oneline mono">
            <Link to={`/journal/${e.generation}`}>g{e.generation}</Link> · {e.workItem.task} ·{' '}
            {e.workItem.target}
          </p>
        ))}
      </div>
    )
  }

  const req = g.requirements[id]
  if (req) {
    const row = rows[id]
    return (
      <div>
        <h1 className="mono">{id}</h1>
        <RequirementCard id={id} r={req} row={row} />
        <h2>verification</h2>
        <VerifyLine id={id} row={row} />
        {row?.entity && (
          <p className="muted">
            deliverable entity <NodeLink id={row.entity} />
          </p>
        )}
        <h2>source</h2>
        <p>
          <SectionLink doc={req.source.doc} section={req.source.section} quote={req.source.quote} />{' '}
          <span className="muted">“{req.source.quote}”</span>
        </p>
        {journalHits.length > 0 && (
          <>
            <h2>journal</h2>
            {journalHits.map((e) => (
              <p key={e.generation} className="oneline mono">
                <Link to={`/journal/${e.generation}`}>g{e.generation}</Link> · {e.workItem.task} ·{' '}
                {e.workItem.target}
              </p>
            ))}
          </>
        )}
      </div>
    )
  }

  const rel = g.relationships[id]
  if (rel)
    return (
      <div>
        <h1 className="mono">{id}</h1>
        <RelationshipCard id={id} r={rel} />
      </div>
    )

  const diag = g.diagnostics[id]
  if (diag) {
    if (diag.triage === 'suppressed')
      return <p className="muted">this diagnostic is suppressed</p>
    return (
      <div>
        <h1 className="mono">{id}</h1>
        <DiagnosticCard id={id} d={diag} />
      </div>
    )
  }

  return (
    <p className="muted">
      no node with id <span className="mono">{id}</span> in the graph
    </p>
  )
}
