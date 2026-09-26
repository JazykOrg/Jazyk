// The action button and its sheet: one button whose label is the state, a sheet
// that says what will run, the mode switches, and Run
// (docs/frontends/gui.md#the-action-button). In on-request mode Run records the
// release and starts the job, or wakes the attached agent.
import { useEffect, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { post, put } from '../lib/api'
import { needsPageModel, useCapabilities, whyNot } from '../lib/capabilities'
import { useInbox } from '../lib/product'
import { useDocs, useGenPending, useStatus, useWorkers } from '../lib/queries'
import { useApp } from '../lib/store'
import ModelSourcePanel from '../components/ModelSourcePanel'
import { useModelSource } from '../lib/model-source'
import { DEFAULT_MODEL } from '../lib/webllm'
import { useWords } from '../lib/words'

function useRunning() {
  const jobs = useApp((a) => a.jobs)
  const running = Object.values(jobs).find((j) => j.state === 'running')
  const queued = Object.values(jobs).filter((j) => j.state === 'queued').length
  return { running, queued }
}

export default function ActionButton() {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const w = useWords()
  const { running, queued } = useRunning()
  const turns = useApp((a) => a.turns)
  const inbox = useInbox()
  const pending = useGenPending()
  const { data: docs } = useDocs()

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open])

  const runningTurn = Object.values(turns).find((t) => t.state === 'running')
  const progressN = runningTurn && runningTurn.sections.length > 0 ? `${runningTurn.touched.length} of ${runningTurn.sections.length}` : ''
  const changed = inbox.data?.pending.docs.length ?? (docs?.docs ?? []).filter((d) => d.stale).length
  const toBuild = pending.data?.pending.length ?? 0

  let label: string
  let cls = 'primary'
  if (running) {
    const verb = running.kind.kind === 'compile' ? w.syncing : running.kind.kind === 'gen' ? w.building : running.kind.kind === 'verify' || running.kind.kind === 'audit' ? w.checking : `${running.kind.kind}…`
    label = `${verb}${progressN ? ` ${progressN}` : ''}${queued > 0 ? ` (+${queued})` : ''}`
    cls += ' running'
  } else if (changed > 0) {
    label = `${w.sync} (${changed} document${changed === 1 ? '' : 's'} changed)`
  } else if (toBuild > 0) {
    label = `${w.build} (${toBuild} ${toBuild === 1 ? 'entity' : 'entities'})`
  } else {
    label = w.upToDate
    cls += ' idle'
  }

  return (
    <div className="ws-action" ref={ref}>
      <button className={cls} onClick={() => setOpen((v) => !v)} title="what will run, the modes, and Run">
        {running ? '●' : '▸'} {label}
      </button>
      {open && (
        <>
          <div className="ws-backdrop" onClick={() => setOpen(false)} />
          <ActionSheet close={() => setOpen(false)} />
        </>
      )}
    </div>
  )
}

function ActionSheet({ close }: { close: () => void }) {
  const w = useWords()
  const dev = useApp((a) => a.dev)
  const navigate = useNavigate()
  const caps = useCapabilities()
  const { running, queued } = useRunning()
  const inbox = useInbox()
  const pending = useGenPending()
  const { data: s } = useStatus()
  const { data: workers } = useWorkers()
  const watchMode = useApp((a) => a.watchMode)
  const genMode = useApp((a) => a.genMode)
  const [busy, setBusy] = useState<string | null>(null)

  const manual = watchMode !== 'watch'
  const changedDocs = inbox.data?.pending.docs ?? []
  const toBuild = pending.data?.pending.length ?? 0
  const agents = (workers?.workers ?? []).filter((x) => x.kind === 'agent')
  const prefersAgent = workers?.workflow.worker === 'agent'
  const agentMissing = prefersAgent && agents.length === 0
  const costs = s?.costs
  const estimate = costs && (costs.sessions ?? 0) > 0 ? `${costs.sessions} sessions and ${Math.round((costs.tokens ?? 0) / 1000)}k tokens last time` : null
  const openGoals = s?.board?.open ?? 0

  const run = async (kind: 'compile' | 'gen' | 'verify' | 'decompile', release?: 'compile' | 'generate') => {
    setBusy(kind)
    try {
      if (release) await post('/api/release', { stage: release })
      // A release with an attached, preferred agent wakes the agent; the internal
      // run is only queued when nobody else will act (dispatch).
      if (!(release && prefersAgent && agents.length > 0)) {
        if (kind === 'gen') await post('/api/jobs', { kind: 'gen', entities: [] })
        else if (kind === 'verify') await post('/api/jobs', { kind: 'verify', targets: [] })
        else await post('/api/jobs', { kind })
      }
      close()
    } finally {
      setBusy(null)
    }
  }

  const cannot = (need: 'compile' | 'gen' | 'verify' | 'decompile') => whyNot(caps.data, need)

  return (
    <div className="ws-sheet">
      {running && (
        <>
          <h3>Running</h3>
          <div className="row">
            <span className="grow">
              {running.kind.kind}
              {queued > 0 ? `, ${queued} queued` : ''}
            </span>
            <button onClick={() => post(`/api/jobs/${running.id}/cancel`)} title="stops at the next boundary; a model call in flight finishes first">
              Cancel
            </button>
          </div>
        </>
      )}

      <h3>{w.sync}</h3>
      {needsPageModel(caps.data) ? (
        <ModelSourcePanel />
      ) : cannot('compile') ? (
        <p className="why">{cannot('compile')}</p>
      ) : (
        <>
          <p className="why" style={{ margin: '2px 0' }}>
            {changedDocs.length > 0
              ? `${changedDocs.length} document${changedDocs.length === 1 ? '' : 's'} changed since the last sync: ${changedDocs.slice(0, 3).join(', ')}${changedDocs.length > 3 ? ', …' : ''}.`
              : openGoals > 0
                ? `${openGoals} item${openGoals === 1 ? '' : 's'} of work left from the last sync.`
                : 'Nothing has changed since the last sync.'}
            {estimate ? ` ${estimate}.` : ''}
          </p>
          {caps.data?.backend !== 'server' && <ModelLine />}
          <p className="why" style={{ margin: '2px 0' }}>
            {agentMissing
              ? 'Your settings prefer an attached agent, but none is connected. The built-in runner will run instead.'
              : agents.length > 0 && prefersAgent
                ? `${agents[0].client || 'an agent'} is attached and will run it.`
                : 'The built-in runner will run it.'}
          </p>
          <div className="row">
            <button className="run" disabled={!!running || busy !== null} onClick={() => void run('compile', manual ? 'compile' : undefined)}>
              {busy === 'compile' ? 'Starting…' : `Run ${w.sync.toLowerCase().split(' ')[0]}`}
            </button>
            {dev && (
              <button
                onClick={() => {
                  close()
                  navigate('/dev/board?preview=next')
                }}
                title="the next session's prompt, exactly as the model receives it"
              >
                Preview prompt
              </button>
            )}
          </div>
        </>
      )}

      <h3>{w.build}</h3>
      {cannot('gen') ? (
        <p className="why">{cannot('gen')}</p>
      ) : (
        <div className="row">
          <span className="grow why">
            {toBuild > 0 ? `${toBuild} ${toBuild === 1 ? 'entity' : 'entities'} to build from the documents.` : 'Nothing to build: every entity is built or has no requirements yet.'}
          </span>
          <button disabled={!!running || toBuild === 0 || busy !== null} onClick={() => void run('gen', genMode === 'manual' ? 'generate' : undefined)}>
            {busy === 'gen' ? 'Starting…' : 'Run'}
          </button>
        </div>
      )}

      <h3>Modes</h3>
      <div className="modes">
        <label title="On save: every saved change syncs (spends model budget). On request: changes wait in the Inbox until you run a sync. Shared with attached agents.">
          <span>Sync</span>
          <select value={manual ? 'queue' : 'watch'} onChange={(e) => put('/api/watch', { mode: e.target.value })}>
            <option value="queue">{w.syncOnRequest}</option>
            <option value="watch">{w.syncOnSave}</option>
          </select>
        </label>
        <label title="After sync: a finished sync with entities to build queues a build. On request: building is a click.">
          <span>Build</span>
          <select value={genMode} onChange={(e) => put('/api/watch', { gen: e.target.value })}>
            <option value="manual">{w.buildOnRequest}</option>
            <option value="auto">{w.buildAfterSync}</option>
          </select>
        </label>
      </div>

      <h3>More</h3>
      <div className="entry">
        <b>{w.check}</b>
        <span className="grow">{cannot('verify') ?? 'Run the tests and refresh every trace row.'}</span>
        <button disabled={!!running || busy !== null || !!cannot('verify')} onClick={() => void run('verify')}>
          Run
        </button>
      </div>
      <div className="entry">
        <b>{w.document}</b>
        <span className="grow">
          {cannot('decompile') ??
            ((workers?.unclaimed ?? 0) > 0
              ? `${workers?.unclaimed} product file${workers?.unclaimed === 1 ? '' : 's'} no document describes. Write documents for them.`
              : 'Every product file is described by a document.')}
        </span>
        <button
          disabled={!!running || busy !== null || !!cannot('decompile') || (workers?.unclaimed ?? 0) === 0}
          onClick={() => {
            close()
            navigate('/product/not-described')
          }}
        >
          Choose area
        </button>
      </div>
    </div>
  )
}

// The browser's model, once ready, and where to change it.
function ModelLine() {
  const src = useModelSource()
  const name = src.kind === 'endpoint' ? src.model : src.kind === 'chrome' ? "the browser's built-in model" : DEFAULT_MODEL
  return (
    <p className="why" style={{ margin: '2px 0' }}>
      Model: {name}. <Link to="/settings">Change</Link>
    </p>
  )
}
