// The Developer lens's pages (docs/frontends/gui.md#developer-toggle): Runs (the
// run console), Prompt (the preview pane), Journal (raw entries), Workers (the
// control-plane snapshot). Board, Map, Benchmarks, and Feedback mount their own
// routes.
import { useEffect } from 'react'
import { Link } from 'react-router'
import { post } from '../lib/api'
import { useJournal, useWorkers } from '../lib/queries'
import { useApp } from '../lib/store'
import Activity from '../workbench/Activity'
import PreviewPane from '../components/PreviewPane'
import './routes.css'

export function Runs() {
  const setOpen = useApp((a) => a.setActivityOpen)
  useEffect(() => {
    setOpen(true)
  }, [setOpen])
  return (
    <div className="dev-runs">
      <Activity />
    </div>
  )
}

export function Prompt() {
  return (
    <div className="ws-scroll">
      <h1>Prompt</h1>
      <p className="muted">The next session's prompt exactly as the model receives it. Read-only: prompts are assembled, never authored.</p>
      <PreviewPane target="" onClose={() => {}} />
    </div>
  )
}

export function Journal() {
  const journal = useJournal(200)
  return (
    <div className="ws-scroll">
      <h1>Journal</h1>
      {journal.error && <p className="error-inline">{journal.error.message}</p>}
      {(journal.data?.entries ?? []).map((e) => (
        <div key={e.generation} className="card">
          <p className="row" style={{ margin: 0 }}>
            <Link to={`/history/${e.generation}`} className="mono">
              g{e.generation}
            </Link>
            <span className="mono muted">{e.kind ?? 'session'}</span>
            <span className="mono muted">{e.build}</span>
            <span className="muted">
              {e.mutations.length} mutations · {(e.resolved_goals ?? []).length} resolved · {(e.opened_goals ?? []).length} opened · {e.rounds} rounds · {e.tokens} tok
            </span>
          </p>
          {(e.batch ?? []).length > 0 && <p className="mono muted" style={{ margin: '2px 0' }}>{(e.batch ?? []).join(', ')}</p>}
        </div>
      ))}
    </div>
  )
}

export function Workers() {
  const w = useWorkers()
  const d = w.data
  return (
    <div className="ws-scroll">
      <h1>Workers</h1>
      {w.error && <p className="error-inline">{w.error.message}</p>}
      {d && (
        <>
          <p className="mono muted">
            compile: {d.workflow.compile} · generate: {d.workflow.generate ?? d.workflow.gen} · worker: {d.workflow.worker}
          </p>
          <h2>registered</h2>
          {d.workers.length === 0 && <p className="muted">no worker registered</p>}
          {d.workers.map((x) => (
            <p key={x.id} className="mono">
              {x.kind} {x.client} pid {x.pid} · heartbeat {new Date(x.heartbeat_at * 1000).toLocaleTimeString()} · {x.task || 'idle'}
            </p>
          ))}
          <h2>leases</h2>
          {d.leases.length === 0 && <p className="muted">no lease held</p>}
          {d.leases.map((l) => (
            <p key={l.task} className="mono">
              {l.task} by {l.worker} until {new Date(l.expires_at * 1000).toLocaleTimeString()}
            </p>
          ))}
          <h2>gated</h2>
          <p className="row">
            <span className="mono">compile {d.gated.compile}</span>
            {d.gated.compile > 0 && <button onClick={() => post('/api/release', { stage: 'compile' })}>release compile</button>}
            <span className="mono">generate {d.gated.generate}</span>
            {d.gated.generate > 0 && <button onClick={() => post('/api/release', { stage: 'generate' })}>release generate</button>}
          </p>
          <h2>actionable</h2>
          <p className="mono muted">
            compile {d.actionable.compile} · bind {d.actionable.bind ?? 0} · generate {d.actionable.generate} · verify {d.actionable.verify} · unclaimed {d.unclaimed ?? 0}
          </p>
        </>
      )}
    </div>
  )
}
