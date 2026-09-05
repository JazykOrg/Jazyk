// The persistent status bar: read-only project state, every segment links into
// the app. The run controls live in the activity panel's control line.
import { useEffect } from 'react'
import { Link } from 'react-router'
import { verdictText } from '../lib/api'
import { useProject, useStatus } from '../lib/queries'
import { useApp } from '../lib/store'

export default function StatusBar() {
  const { data: s } = useStatus()
  const { data: proj } = useProject()
  const connected = useApp((a) => a.connected)
  const setActivityOpen = useApp((a) => a.setActivityOpen)

  // The tab names the project, so two servers side by side are told apart
  // (docs/frontends/gui.md#serving).
  useEffect(() => {
    if (!proj) return
    const dir = proj.root.replace(/\/+$/, '').split('/').pop() || proj.root
    document.title = `${dir} · jazyk`
  }, [proj])

  const diag = s?.diagnostics ?? {}
  const cov = s?.coverage
  const board = s?.board
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
            className={s.verdict?.state === 'incomplete' ? 'v-stale' : ''}
            title="last verdict"
            onClick={(e) => {
              e.preventDefault()
              setActivityOpen(true)
            }}
          >
            {verdictText(s.verdict)}
          </a>
          {board && (board.open > 0 || board.blocked > 0) && (
            <Link to="/board" title="the goal board">
              {board.open} open goal{board.open === 1 ? '' : 's'}
              {board.blocked > 0 ? `, ${board.blocked} blocked` : ''}
            </Link>
          )}
          {s.parked.length > 0 && (
            <Link to="/board?state=parked" className="v-stale">
              {s.parked.length} parked
            </Link>
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
          <span className="muted" title="sessions and tokens spent">
            {s.spent.sessions} sessions · {Math.round(s.spent.tokens / 1000)}k tok
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
