// The action button (docs/frontends/gui.md#the-action-button): one split button in
// the top bar. The main part runs the chosen action at once and its label is that
// action with its state; while a job runs it shows the progress and a click opens
// the job's build thread. With no model that builds in a tab it opens the arrow's
// popover instead. The arrow's popover holds the model chooser in a tab and the
// actions the backend runs on a server, with no buttons of its own.
import * as Popover from '@radix-ui/react-popover'
import { useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { post, put } from '../lib/api'
import { needsBuildModel, useCapabilities, whyNot, type JobKind } from '../lib/capabilities'
import { useInbox } from '../lib/product'
import { useDocs, useGenPending, useMatrix, useStatus, useWorkers } from '../lib/queries'
import { useApp, type ActionChoice } from '../lib/store'
import ModelSourcePanel from '../components/ModelSourcePanel'
import { useWords, type Words } from '../lib/words'

const CHOICES: ActionChoice[] = ['compile', 'gen', 'verify']
const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`

function useRunning() {
  const jobs = useApp((a) => a.jobs)
  const running = Object.values(jobs).find((j) => j.state === 'running')
  const queued = Object.values(jobs).filter((j) => j.state === 'queued').length
  return { running, queued }
}

// The counts every line reads: documents changed, work left, entities to generate,
// tests to run.
function useCounts() {
  const inbox = useInbox()
  const pending = useGenPending()
  const { data: docs } = useDocs()
  const matrix = useMatrix()
  const { data: s } = useStatus()
  const changedDocs = inbox.data?.pending.docs ?? (docs?.docs ?? []).filter((d) => d.stale).map((d) => d.path)
  return {
    changedDocs,
    openGoals: s?.board?.open ?? 0,
    toBuild: pending.data?.pending.length ?? 0,
    tests: matrix.data ? Object.values(matrix.data.rows).filter((r) => r.test).length : 0,
    status: s,
  }
}

type Counts = ReturnType<typeof useCounts>

// One action's state, in product words.
function stateLine(kind: ActionChoice, c: Counts): string {
  switch (kind) {
    case 'compile':
      return c.changedDocs.length > 0 ? `${plural(c.changedDocs.length, 'document')} changed` : c.openGoals > 0 ? `${plural(c.openGoals, 'item')} left` : 'Up to date'
    case 'gen':
      return c.toBuild > 0 ? plural(c.toBuild, 'entity', 'entities') : 'Nothing to generate'
    case 'verify':
      return c.tests > 0 ? plural(c.tests, 'test') : 'No tests yet'
  }
}

function actionWord(kind: ActionChoice, w: Words): string {
  return kind === 'compile' ? w.build : kind === 'gen' ? w.generate : w.review
}

function runningWord(kind: string, w: Words): string {
  return kind === 'compile' ? w.building : kind === 'gen' ? w.generating : kind === 'verify' || kind === 'audit' ? w.reviewing : `${kind}…`
}

// Which actions the backend offers: what it runs, plus Build in a tab whose model is
// not ready or of the reader tier, since the main part then opens the chooser.
function useOffered() {
  const caps = useCapabilities()
  return (kind: JobKind) => !caps.data || caps.data.jobs.includes(kind) || (kind === 'compile' && needsBuildModel(caps.data))
}

// The chosen action: the remembered choice when the backend offers it, else the
// default for the state (Build while documents changed, Generate when entities wait).
function useChosen(c: Counts): ActionChoice {
  const choice = useApp((a) => a.action)
  const offered = useOffered()
  if (choice && offered(choice)) return choice
  if (c.changedDocs.length === 0 && c.openGoals === 0 && c.toBuild > 0 && offered('gen')) return 'gen'
  return 'compile'
}

// The running job's progress: the sections the session reached, else the batch of
// the plan it is on.
function useProgress(): string {
  const turns = useApp((a) => a.turns)
  const all = Object.values(turns)
  const running = all.find((t) => t.state === 'running')
  if (running && running.sections.length > 0) return `${running.touched.length} of ${running.sections.length}`
  if (all.length > 0) {
    const done = all.filter((t) => t.state === 'done' || t.state === 'failed').length
    return `${Math.min(done + 1, all.length)} of ${all.length}`
  }
  return ''
}

export default function ActionButton() {
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const w = useWords()
  const dev = useApp((a) => a.dev)
  const caps = useCapabilities()
  const { running, queued } = useRunning()
  const counts = useCounts()
  const chosen = useChosen(counts)
  const progress = useProgress()
  const setAction = useApp((a) => a.setAction)
  const setChatOpen = useApp((a) => a.setChatOpen)
  const selectChat = useApp((a) => a.selectChat)
  const watchMode = useApp((a) => a.watchMode)
  const genMode = useApp((a) => a.genMode)
  const offered = useOffered()
  const { data: workers } = useWorkers()

  const server = caps.data?.backend === 'server'
  const manual = watchMode !== 'watch'
  const agents = (workers?.workers ?? []).filter((x) => x.kind === 'agent')
  const prefersAgent = workers?.workflow.worker === 'agent'
  // Nothing could run in a tab without a model that builds: the click shows the
  // chooser.
  const needsModel = needsBuildModel(caps.data)
  const nothing = (chosen === 'gen' && counts.toBuild === 0) || (chosen === 'verify' && counts.tests === 0)
  const costs = counts.status?.costs
  const estimate = costs && (costs.sessions ?? 0) > 0 ? `About ${plural(costs.sessions ?? 0, 'session')} and ${Math.round((costs.tokens ?? 0) / 1000)}k tokens, as last time.` : undefined

  // A job that starts takes over the main part; the popover gives way to it.
  const lastRunning = useRef<number | null>(null)
  useEffect(() => {
    const id = running?.id ?? null
    if (id !== null && lastRunning.current === null) setOpen(false)
    lastRunning.current = id
  }, [running?.id])

  // Records the release in on-request mode on a server, then starts the job, unless
  // the release wakes an attached agent the dispatch preference names
  // (docs/compiler/control-plane.md#dispatch). The job's build thread opens itself
  // when it starts; a start that fails says why in the popover.
  const run = async (kind: ActionChoice) => {
    const newest = () => Math.max(0, ...Object.values(useApp.getState().jobs).map((j) => j.id))
    const before = newest()
    setBusy(true)
    setError(null)
    try {
      const release = !server ? null : kind === 'compile' && manual ? 'compile' : kind === 'gen' && genMode === 'manual' ? 'generate' : null
      if (release) await post('/api/release', { stage: release })
      if (!(release && prefersAgent && agents.length > 0)) {
        if (kind === 'gen') await post('/api/jobs', { kind: 'gen', entities: [] })
        else if (kind === 'verify') await post('/api/jobs', { kind: 'verify', targets: [] })
        else await post('/api/jobs', { kind })
      }
    } catch (e) {
      // A job that started and failed tells its thread why; only a start that never
      // happened speaks here.
      if (newest() === before) {
        setError(e instanceof Error ? e.message : String(e))
        setOpen(true)
      }
    } finally {
      setBusy(false)
    }
  }

  const onMain = () => {
    if (running) {
      setChatOpen(true)
      selectChat(`follow-${running.id}`)
    } else if (needsModel) setOpen(true)
    else if (!busy) void run(chosen)
  }

  let label: string
  let title: string | undefined = estimate
  let cls = 'primary'
  if (running) {
    label = `${runningWord(running.kind.kind, w)}${progress ? ` ${progress}` : ''}${queued > 0 ? ` (+${queued})` : ''}`
    title = 'the build thread'
    cls += ' running'
  } else if (busy) {
    label = 'Starting…'
  } else if (needsModel) {
    label = w.build
    title = 'choose a model'
  } else if (chosen === 'compile' && counts.changedDocs.length === 0 && counts.openGoals === 0) {
    label = w.upToDate
    cls += ' idle'
  } else {
    label = `${actionWord(chosen, w)} · ${stateLine(chosen, counts)}`
    if (nothing) cls += ' idle'
  }

  const unclaimed = workers?.unclaimed ?? 0
  const entry = (kind: ActionChoice) => {
    const reason = offered(kind) ? null : whyNot(caps.data, kind)
    if (reason && !dev) return null
    const choose = () => {
      if (reason) return
      setAction(kind)
      setOpen(false)
    }
    return (
      <div
        key={kind}
        role="option"
        aria-selected={chosen === kind}
        aria-disabled={reason ? true : undefined}
        tabIndex={reason ? -1 : 0}
        className="ws-menu-item ws-action-item"
        onClick={choose}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            choose()
          }
        }}
      >
        <b>{actionWord(kind, w)}</b>
        <span className="state">{reason ?? stateLine(kind, counts)}</span>
      </div>
    )
  }
  const documentReason = whyNot(caps.data, 'decompile')

  return (
    <Popover.Root
      open={open}
      onOpenChange={(v) => {
        setOpen(v)
        if (!v) setError(null)
      }}
    >
      <Popover.Anchor asChild>
        <div className="ws-action">
          <button className={`${cls} main`} onClick={onMain} title={title} disabled={!running && !needsModel && nothing}>
            {running ? '●' : '▸'} {label}
          </button>
          <Popover.Trigger asChild>
            <button className={`${cls} arrow`} aria-label={server ? 'choose an action' : 'choose a model'} title={server ? 'choose an action' : 'choose a model'}>
              ▾
            </button>
          </Popover.Trigger>
        </div>
      </Popover.Anchor>
      <Popover.Portal>
        <Popover.Content className="ws-menu ws-pop" align="end" sideOffset={6} collisionPadding={8}>
          {error && <p className="ws-menu-note failed">{error}</p>}
          {!server && <ModelSourcePanel loadOnChoose />}
          {(server || dev) && (
            <div role="listbox" aria-label="actions" className="ws-pop-list">
              {CHOICES.map(entry)}
            </div>
          )}
          {dev && (
            <>
              {documentReason || unclaimed === 0 ? (
                <div className="ws-menu-item ws-action-item" aria-disabled title="documents are written from the product for an area of the not-described list">
                  <b>{w.document}</b>
                  <span className="state">{documentReason ?? 'Every product file is described'}</span>
                </div>
              ) : (
                <Link to="/product/not-described" className="ws-menu-item ws-action-item" onClick={() => setOpen(false)}>
                  <b>{w.document}</b>
                  <span className="state">{plural(unclaimed, 'file')} not described · choose the area</span>
                </Link>
              )}
              <Link to="/dev/board?preview=next" className="ws-menu-item ws-action-item" onClick={() => setOpen(false)} title="the next session's prompt, exactly as the model receives it">
                <b>Preview prompt</b>
                <span className="state">the next batch</span>
              </Link>
              <div className="ws-pop-modes">
                <label title="On save: every saved change builds (spends model budget). On request: changes wait in the Inbox until you run a build. Shared with attached agents.">
                  <span>Build</span>
                  <select value={manual ? 'queue' : 'watch'} onChange={(e) => put('/api/watch', { mode: e.target.value })}>
                    <option value="queue">{w.buildOnRequest}</option>
                    <option value="watch">{w.buildOnSave}</option>
                  </select>
                </label>
                <label title="After build: a finished build with entities to generate queues a generate run. On request: generating is a click.">
                  <span>Generate</span>
                  <select value={genMode} onChange={(e) => put('/api/watch', { gen: e.target.value })}>
                    <option value="manual">{w.generateOnRequest}</option>
                    <option value="auto">{w.generateAfterBuild}</option>
                  </select>
                </label>
              </div>
            </>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}
