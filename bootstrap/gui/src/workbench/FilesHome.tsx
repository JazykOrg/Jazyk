// The files center with nothing open: the project at a glance and the attention
// list. The run actions live in the activity control line below.
import { Link } from 'react-router'
import { useGenPending, useGraph, useJournal, useMatrix, useStatus } from '../lib/queries'
import NodeLink from '../components/NodeLink'
import { SevChip, verifyClass } from '../components/Chip'
import '../routes/routes.css'

const SEV_RANK: Record<string, number> = { error: 0, warning: 1, info: 2 }

export default function FilesHome() {
  const status = useStatus()
  const graph = useGraph()
  const matrix = useMatrix()
  const pending = useGenPending()
  const journal = useJournal(50)

  if (status.error)
    return (
      <p className="error-inline">
        {status.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); status.refetch() }}>retry</a>
      </p>
    )
  if (!status.data) return <p className="muted">loading…</p>
  const s = status.data

  const counts = matrix.data?.counts ?? {}
  const verifyPending = Object.entries(counts)
    .filter(([k]) => k === 'failing' || k === 'unverified' || k.startsWith('stale'))
    .reduce((a, [, n]) => a + n, 0)

  const openDiags = Object.entries(graph.data?.diagnostics ?? {})
    .filter(([, d]) => (d.lifecycle ?? 'open') === 'open' && d.triage !== 'suppressed')
    .sort((a, b) => (SEV_RANK[a[1].severity] ?? 3) - (SEV_RANK[b[1].severity] ?? 3))

  return (
    <div>
      <p className="muted">select a document or a deliverable file on the left</p>
      <div className="statrow">
        <span><b>{s.counts.entities}</b> entities</span>
        <span><b>{s.counts.requirements}</b> requirements</span>
        <span><b>{s.counts.relationships}</b> relationships</span>
        <Link to="/graph?list=diagnostics" style={{ textDecoration: 'none' }}>
          {Object.entries(s.diagnostics)
            .filter(([, n]) => n > 0)
            .map(([sev, n]) => (
              <span key={sev} className={`chip sev-${sev}`}>
                {n} {sev}
              </span>
            ))}
          {!Object.values(s.diagnostics).some((n) => n > 0) && (
            <span className="muted">no diagnostics</span>
          )}
        </Link>
        <span>
          coverage <b>{s.coverage.covered}</b>/{s.coverage.total}
        </span>
        <span>
          {Object.entries(counts).map(([st, n]) => (
            <span key={st} className={verifyClass(st)} style={{ marginRight: 8 }}>
              {n} {st}
            </span>
          ))}
        </span>
      </div>

      <div className="grid2">
        <div className="card">
          <h2 style={{ marginTop: 0 }}>Attention</h2>
          <p>
            <Link to="/work">{pending.data ? pending.data.pending.length : '…'} pending generation</Link>
            {' · '}
            <Link to="/work/verify">{matrix.data ? verifyPending : '…'} pending verification</Link>
          </p>
          {graph.data && openDiags.length === 0 && <p className="muted">no open diagnostics</p>}
          {openDiags.slice(0, 5).map(([id, d]) => (
            <p key={id} className="oneline" title={d.message}>
              <SevChip severity={d.severity} /> {d.message} <NodeLink id={id} />
            </p>
          ))}
        </div>

        <div className="card">
          <h2 style={{ marginTop: 0 }}>Recent changes</h2>
          {journal.data && journal.data.entries.length === 0 && (
            <p className="muted">no changesets yet</p>
          )}
          {(journal.data?.entries ?? []).slice(0, 8).map((e) => (
            <p key={e.generation} className="oneline mono">
              <Link to={`/journal/${e.generation}`}>g{e.generation}</Link> · {e.workItem.task} ·{' '}
              {e.workItem.target} · {e.mutations.length} mut · {e.tokens} tok
            </p>
          ))}
        </div>
      </div>
    </div>
  )
}
