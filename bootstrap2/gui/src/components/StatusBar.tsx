// The persistent status bar: every segment links into the app, and the compile
// button becomes the live job indicator while one runs.
import { useState } from 'react'
import { Link } from 'react-router'
import { post, put } from '../lib/api'
import { useDocs, useStatus } from '../lib/queries'
import { useApp } from '../lib/store'

export default function StatusBar() {
  const { data: s } = useStatus()
  const { data: docs } = useDocs()
  const [queueOpen, setQueueOpen] = useState(false)
  const jobs = useApp((a) => a.jobs)
  const connected = useApp((a) => a.connected)
  const watchMode = useApp((a) => a.watchMode)
  const running = Object.values(jobs).find((j) => j.state === 'running')
  const queued = Object.values(jobs).filter((j) => j.state === 'queued').length
  // The change queue is derived: documents whose on-disk hash drifted from the graph.
  const changedDocs = (docs?.docs ?? []).filter((d) => d.stale)

  const diag = s?.diagnostics ?? {}
  const cov = s?.coverage
  return (
    <footer className="statusbar mono">
      {s && (
        <>
          <Link to="/journal" title="generation counter">
            g{s.generation}
          </Link>
          <Link to="/build" className={s.verdict === 'incomplete' ? 'v-stale' : ''} title="last verdict">
            {s.verdict || 'no build'}
          </Link>
          {s.parked.length > 0 && (
            <Link to="/build" className="v-stale">
              {s.parked.length} parked
            </Link>
          )}
          {cov && (
            <Link to="/ir/coverage" title="covered sections">
              coverage {cov.covered}/{cov.total}
            </Link>
          )}
          <Link to="/ir/diagnostics" title="open diagnostics by severity">
            {(['error', 'warning', 'info'] as const).map((sev) =>
              diag[sev] ? (
                <span key={sev} className={`sev-${sev}`}>
                  {' '}
                  {diag[sev]} {sev}
                </span>
              ) : null,
            )}
            {!diag.error && !diag.warning && !diag.info && <span className="muted"> no diagnostics</span>}
          </Link>
          <span className="muted" title="tokens spent">
            {s.spent.turns} turns · {Math.round(s.spent.tokens / 1000)}k tok
          </span>
        </>
      )}
      <span className="statusbar-right">
        {!connected && <span className="v-stale">live updates disconnected, polling</span>}
        <select
          value={watchMode}
          onChange={(e) => put('/api/watch', { mode: e.target.value })}
          title="off: changes only update badges · queue: changes queue, compiling is a click · watch: compile on change (spends LLM budget)"
        >
          <option value="off">off</option>
          <option value="queue">queue</option>
          <option value="watch">watch</option>
        </select>
        {watchMode !== 'off' && changedDocs.length > 0 && !running && (
          <a
            href="#queue"
            className="v-stale"
            onClick={(e) => {
              e.preventDefault()
              setQueueOpen((v) => !v)
            }}
            title="documents changed since the last reconcile"
          >
            {changedDocs.length} changed
          </a>
        )}
        {running ? (
          <Link to="/build" className="v-stale">
            ▶ {running.kind.kind} running{queued > 0 ? ` (+${queued} queued)` : ''}
          </Link>
        ) : (
          <button onClick={() => post('/api/jobs', { kind: 'compile' })}>
            compile{changedDocs.length > 0 ? ` ${changedDocs.length}` : ''} ▸
          </button>
        )}
      </span>
      {queueOpen && changedDocs.length > 0 && (
        <div className="queue-pop card">
          <p className="muted">changed since the last reconcile:</p>
          {changedDocs.map((d) => (
            <p key={d.path}>
              <Link className="id mono" to={`/docs/${d.path}`} onClick={() => setQueueOpen(false)}>
                {d.path}
              </Link>
            </p>
          ))}
          <button
            onClick={() => {
              post('/api/jobs', { kind: 'compile' })
              setQueueOpen(false)
            }}
          >
            compile ▸
          </button>
        </div>
      )}
    </footer>
  )
}
