// The status bar: the sync number, the state in product words, the waiting count,
// the problem counts, the connection state, and the Developer toggle
// (docs/frontends/gui.md#layout). The state never travels alone.
import { useEffect } from 'react'
import { Link } from 'react-router'
import { useProject, useStatus } from '../lib/queries'
import { useInbox } from '../lib/product'
import { useApp } from '../lib/store'
import { useWords, verdictWords } from '../lib/words'

export default function StatusBar() {
  const { data: s } = useStatus()
  const { data: proj } = useProject()
  const connected = useApp((a) => a.connected)
  const dev = useApp((a) => a.dev)
  const setDev = useApp((a) => a.setDev)
  const w = useWords()
  const inbox = useInbox()
  const waiting = inbox.data?.total ?? 0

  // The tab names the project, so two servers side by side are told apart.
  useEffect(() => {
    if (!proj) return
    const dir = proj.root.replace(/\/+$/, '').split('/').pop() || proj.root
    document.title = `${dir} · jazyk`
  }, [proj])

  // ⌘⇧D flips the toggle from anywhere.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === 'd') {
        e.preventDefault()
        setDev(!useApp.getState().dev)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [setDev])

  const diag = s?.diagnostics ?? {}
  const incomplete = s?.verdict?.state === 'incomplete'
  return (
    <footer className="ws-status">
      {s && (
        <>
          <Link to="/history" title="the last sync">
            {w.syncNumber(s.generation)}
          </Link>
          <Link to={incomplete ? '/inbox' : '/history'} className={incomplete ? 'v-stale' : ''} title="the state of the last sync">
            {verdictWords(s.verdict, dev)}
          </Link>
          <Link to="/inbox" className={waiting > 0 ? '' : 'muted'} title="questions, proposals, pending changes, and errors">
            {waiting > 0 ? `waiting on you (${waiting})` : 'nothing waiting on you'}
          </Link>
          <Link to="/inbox/problems" title="open problems by severity">
            {(['error', 'warning', 'info'] as const).map((sev) =>
              diag[sev] ? (
                <span key={sev} className={`sev-${sev}`}>
                  {' '}
                  {diag[sev]} {sev === 'info' ? 'note' : sev}
                  {diag[sev] === 1 ? '' : 's'}
                </span>
              ) : null,
            )}
            {!diag.error && !diag.warning && !diag.info && <span className="muted">no problems</span>}
          </Link>
          {dev && s.spent && (
            <span className="muted mono" title="sessions and tokens the last build spent">
              {s.spent.sessions} sessions · {Math.round(s.spent.tokens / 1000)}k tok
            </span>
          )}
          {dev && s.board && (
            <Link to="/dev/board" className="muted mono" title="the goal board">
              {s.board.open} open · {s.board.blocked} blocked · {s.board.parked} parked
            </Link>
          )}
        </>
      )}
      <span className="right">
        <span title={connected ? 'live updates connected' : 'live updates disconnected, polling'}>
          <span className={`ws-dot${connected ? '' : ' off'}`} /> {connected ? 'live' : 'polling'}
        </span>
        {proj && dev && (
          <span className="muted mono" title="project root">
            {proj.root}
          </span>
        )}
        <span
          className={`ws-dev${dev ? ' on' : ''}`}
          role="switch"
          aria-checked={dev}
          tabIndex={0}
          title="Developer: the compiler workbench and its words (⌘⇧D)"
          onClick={() => setDev(!dev)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault()
              setDev(!dev)
            }
          }}
        >
          <span className="knob" /> Developer
        </span>
      </span>
    </footer>
  )
}
