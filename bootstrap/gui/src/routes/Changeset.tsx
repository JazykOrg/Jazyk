// One committed changeset: the work item, every mutation with its reasoning,
// and navigation to neighbors and the diff.
import { Link, useParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { get, type JournalEntry } from '../lib/api'
import NodeLink from '../components/NodeLink'
import SectionLink from '../components/SectionLink'
import './routes.css'

function opClass(op: string): string {
  if (op.startsWith('create') || op.startsWith('report')) return 'v-ok'
  if (op.startsWith('update') || op.startsWith('triage')) return 'sev-info'
  if (op.startsWith('delete') || op.startsWith('resolve')) return 'v-bad'
  if (op.startsWith('merge')) return 'v-stale'
  return 'sev-none' // set_coverage, gc
}

export default function Changeset() {
  const { gen: genParam } = useParams()
  const gen = Number(genParam)
  const q = useQuery({
    queryKey: ['journal', 'one', gen],
    queryFn: () =>
      get<{ entries: JournalEntry[]; generation: number }>(`/api/journal?from=${gen}&to=${gen}`),
    enabled: Number.isFinite(gen) && gen > 0,
  })

  if (!Number.isFinite(gen) || gen <= 0) return <p className="muted">not a generation number</p>
  if (q.error)
    return (
      <p className="error-inline">
        {q.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); q.refetch() }}>retry</a>
      </p>
    )
  if (!q.data) return <p className="muted">loading…</p>

  const entry = q.data.entries.find((e) => e.generation === gen) ?? q.data.entries[0]
  const nav = (
    <p className="row mono">
      {gen > 1 && <Link to={`/journal/${gen - 1}`}>← g{gen - 1}</Link>}
      <Link to="/journal">journal</Link>
      <Link to={`/journal/${gen + 1}`}>g{gen + 1} →</Link>
      <Link to={`/journal/diff?from=${Math.max(1, gen - 1)}&to=${gen}`}>diff this build</Link>
    </p>
  )
  if (!entry)
    return (
      <div>
        {nav}
        <p className="muted">no changeset at generation {gen}</p>
      </div>
    )

  return (
    <div>
      <h1 className="mono">g{gen}</h1>
      {nav}
      <div className="card">
        <p style={{ margin: '2px 0' }}>
          <b>{entry.workItem.task}</b> · <span className="mono">{entry.workItem.target}</span> ·{' '}
          <span className="muted">
            {entry.rounds} rounds · {entry.tokens} tok
          </span>
        </p>
        {(entry.workItem.dirtySections ?? []).length > 0 && (
          <p style={{ margin: '2px 0' }}>
            {(entry.workItem.dirtySections ?? []).map((sec) => (
              <span key={sec} style={{ marginRight: 8 }}>
                <SectionLink doc={entry.workItem.target} section={sec} />
              </span>
            ))}
          </p>
        )}
      </div>

      {entry.mutations.length === 0 && <p className="muted">no mutations in this changeset</p>}
      {entry.mutations.map((m, i) => {
        const op = String(m.op ?? '')
        const id = typeof m.id === 'string' ? m.id : typeof m.keep === 'string' ? m.keep : null
        const reasoning = typeof m.reasoning === 'string' ? m.reasoning : null
        return (
          <div key={i} className="card">
            <p style={{ margin: '2px 0' }}>
              <span className={`chip ${opClass(op)}`}>{op}</span>
              {id && <NodeLink id={id} />}
            </p>
            {reasoning && <p className="muted" style={{ margin: '2px 0' }}>{reasoning}</p>}
            <details>
              <summary>body</summary>
              <pre className="pack">{JSON.stringify(m, null, 2)}</pre>
            </details>
          </div>
        )
      })}
    </div>
  )
}
