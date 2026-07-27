// The persistent status bar: every segment links into the app, and the compile
// button becomes the live job indicator while one runs.
import { Link } from 'react-router'
import { post, put } from '../lib/api'
import { useStatus } from '../lib/queries'
import { useApp } from '../lib/store'

export default function StatusBar() {
  const { data: s } = useStatus()
  const jobs = useApp((a) => a.jobs)
  const connected = useApp((a) => a.connected)
  const watch = useApp((a) => a.watch)
  const running = Object.values(jobs).find((j) => j.state === 'running')
  const queued = Object.values(jobs).filter((j) => j.state === 'queued').length

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
        <label className="muted" title="compile on document change (spends LLM budget)">
          <input
            type="checkbox"
            checked={watch}
            onChange={() => put('/api/watch', { enabled: !watch })}
          />{' '}
          watch
        </label>
        {running ? (
          <Link to="/build" className="v-stale">
            ▶ {running.kind.kind} running{queued > 0 ? ` (+${queued} queued)` : ''}
          </Link>
        ) : (
          <button onClick={() => post('/api/jobs', { kind: 'compile' })}>compile ▸</button>
        )}
      </span>
    </footer>
  )
}
