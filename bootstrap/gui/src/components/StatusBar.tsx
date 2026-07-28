// The persistent status bar: read-only project state, every segment links into
// the app. The run controls live in the activity panel's control line.
import { Link } from 'react-router'
import { useProject, useStatus } from '../lib/queries'
import { useApp } from '../lib/store'

export default function StatusBar() {
  const { data: s } = useStatus()
  const { data: proj } = useProject()
  const connected = useApp((a) => a.connected)
  const setActivityOpen = useApp((a) => a.setActivityOpen)

  const diag = s?.diagnostics ?? {}
  const cov = s?.coverage
  return (
    <footer className="statusbar mono">
      {s && (
        <>
          <a
            href="#activity"
            title="generation counter, opens the activity panel"
            onClick={(e) => {
              e.preventDefault()
              setActivityOpen(true)
            }}
          >
            g{s.generation}
          </a>
          <a
            href="#activity"
            className={s.verdict === 'incomplete' ? 'v-stale' : ''}
            title="last verdict"
            onClick={(e) => {
              e.preventDefault()
              setActivityOpen(true)
            }}
          >
            {s.verdict || 'no build'}
          </a>
          {s.parked.length > 0 && (
            <a
              href="#activity"
              className="v-stale"
              onClick={(e) => {
                e.preventDefault()
                setActivityOpen(true)
              }}
            >
              {s.parked.length} parked
            </a>
          )}
          {cov && (
            <Link to="/graph?list=coverage" title="covered sections">
              coverage {cov.covered}/{cov.total}
            </Link>
          )}
          <Link to="/graph?list=diagnostics" title="open diagnostics by severity">
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
        {proj && (
          <span className="muted" title="project root">
            {proj.root}
          </span>
        )}
      </span>
    </footer>
  )
}
