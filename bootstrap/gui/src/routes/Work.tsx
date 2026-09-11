// Work: the generation worklist, with live per-entity progress while a gen job runs.
import { Link } from 'react-router'
import { post } from '../lib/api'
import { useGenPending, useWorkers } from '../lib/queries'
import { useApp } from '../lib/store'
import NodeLink, { linkifyIds } from '../components/NodeLink'
import './routes.css'

// The unclaimed report beside the decompile action: deliverable files no binding
// names, and the click that drafts docs for them. Decompilation stays outside the
// goal board (docs/frontends/gui.md#work).
function Unclaimed({ busy }: { busy: boolean }) {
  const { data: w } = useWorkers()
  if (!w) return null
  const n = w.unclaimed ?? 0
  return (
    <div className="card">
      <p className="row" style={{ margin: '2px 0' }}>
        <b>unclaimed</b>
        <span className={n > 0 ? 'v-stale' : 'muted'}>
          {n === 0 ? 'every deliverable file is bound to a requirement' : `${n} deliverable file${n === 1 ? '' : 's'} no binding names`}
        </span>
        <button
          disabled={busy || n === 0}
          title="draft documents describing what the unclaimed files do; records a decompile release and runs like compile and generate"
          onClick={() => post('/api/jobs', { kind: 'decompile' })}
        >
          decompile ▸
        </button>
      </p>
      {(w.decompileReleased ?? []).length > 0 && (
        <p className="muted mono" style={{ margin: '2px 0' }}>
          released for: {(w.decompileReleased ?? []).join(', ')}
        </p>
      )}
    </div>
  )
}

export default function Work() {
  const pending = useGenPending()
  const jobs = useApp((a) => a.jobs)
  const trace = useApp((a) => a.trace)
  const genJob = Object.values(jobs).find((j) => j.state === 'running' && j.kind.kind === 'gen')
  const busy = Object.values(jobs).some((j) => j.state === 'running' || j.state === 'queued')

  if (pending.error)
    return (
      <p className="error-inline">
        {pending.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); pending.refetch() }}>retry</a>
      </p>
    )
  if (!pending.data) return <p className="muted">loading…</p>
  const rows = pending.data.pending

  // Live gen progress for one entity, from the running job's trace.
  const liveFor = (entity: string) =>
    genJob
      ? trace.filter(
          (r) =>
            r.jobId === genJob.id &&
            r.event.kind.startsWith('genEntity') &&
            r.event.entity === entity,
        )
      : []

  return (
    <div>
      <h1>Work</h1>
      <div className="actionrow">
        <button
          disabled={busy || rows.length === 0}
          onClick={() => post('/api/jobs', { kind: 'gen', entities: rows.map((r) => r.entity) })}
        >
          generate all pending
        </button>
        <span className="muted">(leaf entities first)</span>
        <Link to="/work/verify">verification matrix</Link>
      </div>

      <Unclaimed busy={busy} />

      <h2>generation packages</h2>
      {rows.length === 0 && <p className="empty">no pending generation work</p>}
      {rows.map((r) => (
        <div key={r.entity} className="card">
          <p className="row" style={{ margin: '2px 0' }}>
            <NodeLink id={r.entity} />
            <span className={`chip ${r.reason === 'new' ? 'v-ok' : 'v-stale'}`}>{r.reason}</span>
            <Link to={`/work/gen/${encodeURIComponent(r.entity)}`}>task package</Link>
            <button
              disabled={busy}
              onClick={() => post('/api/jobs', { kind: 'gen', entities: [r.entity] })}
            >
              generate
            </button>
          </p>
          {r.changed.length > 0 && (
            <p className="muted" style={{ margin: '2px 0' }}>
              changed: {linkifyIds(r.changed.join(' '))}
            </p>
          )}
          {liveFor(r.entity).map((row) => {
            const ev = row.event
            const cls =
              ev.kind === 'genEntityFailed'
                ? 't-err'
                : ev.kind === 'genEntityDone'
                  ? 't-ok'
                  : 't-muted'
            return (
              <p key={row.seq} className={`trace-row ${cls}`} style={{ margin: 0 }}>
                {ev.kind.replace('genEntity', '').toLowerCase()}
                {ev.stage ? ` · ${String(ev.stage)}` : ''}
                {ev.reason ? ` · ${String(ev.reason)}` : ''}
                {Array.isArray(ev.files) ? ` · ${(ev.files as string[]).join(', ')}` : ''}
                {ev.error ? ` · ${String(ev.error)}` : ''}
              </p>
            )
          })}
        </div>
      ))}
    </div>
  )
}
