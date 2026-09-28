// The action button (docs/frontends/gui.md#the-action-button): one split button in
// the top bar. Its main part runs one action and its label is that action with the
// state; its arrow opens the list of actions to choose from, one line each with its
// state, and the choice becomes the main part. Clicking the main part opens the
// action sheet for the chosen action: what will run, who runs it, the estimate, Run,
// and Cancel with the progress while a job runs.
import * as DropdownMenu from '@radix-ui/react-dropdown-menu'
import { useEffect, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { post, put } from '../lib/api'
import { belowBuildTier, needsBuildModel, smallBuildModel, useCapabilities, whyNot, type JobKind } from '../lib/capabilities'
import { useInbox } from '../lib/product'
import { useDocs, useGenPending, useMatrix, useStatus, useWorkers } from '../lib/queries'
import { useApp, type ActionChoice } from '../lib/store'
import ModelSourcePanel from '../components/ModelSourcePanel'
import { sourceTier, useModelSource, webllmModelOf, type ModelSource } from '../lib/model-source'
import { TIER_WORDS, modelName } from '../lib/webllm'
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
// not ready or of the reader tier, since the sheet then shows the model chooser.
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
  const w = useWords()
  const dev = useApp((a) => a.dev)
  const caps = useCapabilities()
  const { running, queued } = useRunning()
  const counts = useCounts()
  const chosen = useChosen(counts)
  const progress = useProgress()
  const setAction = useApp((a) => a.setAction)
  const offered = useOffered()
  const navigate = useNavigate()
  const { data: workers } = useWorkers()

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open])

  // The sheet gives way to the run: the main part shows the progress and the build
  // thread opens in the chat column.
  const lastRunning = useRef<number | null>(null)
  useEffect(() => {
    const id = running?.id ?? null
    if (id !== null && lastRunning.current === null) setOpen(false)
    lastRunning.current = id
  }, [running?.id])

  let label: string
  let cls = 'primary'
  if (running) {
    label = `${runningWord(running.kind.kind, w)}${progress ? ` ${progress}` : ''}${queued > 0 ? ` (+${queued})` : ''}`
    cls += ' running'
  } else if (chosen === 'compile' && needsBuildModel(caps.data)) {
    // No model that builds: the click shows the chooser
    // (docs/frontends/gui.md#the-action-button).
    label = w.build
  } else if (chosen === 'compile' && counts.changedDocs.length === 0 && counts.openGoals === 0) {
    label = w.upToDate
    cls += ' idle'
  } else {
    label = `${actionWord(chosen, w)} · ${stateLine(chosen, counts)}`
  }

  const unclaimed = workers?.unclaimed ?? 0
  const entry = (kind: ActionChoice) => {
    const reason = offered(kind) ? null : whyNot(caps.data, kind)
    if (reason && !dev) return null
    return (
      <DropdownMenu.Item key={kind} className="ws-menu-item ws-action-item" disabled={!!reason} onSelect={() => setAction(kind)}>
        <b>{actionWord(kind, w)}</b>
        <span className="state">{reason ?? stateLine(kind, counts)}</span>
      </DropdownMenu.Item>
    )
  }

  return (
    <div className="ws-action">
      <button className={`${cls} main`} onClick={() => setOpen((v) => !v)} title="what will run, and Run">
        {running ? '●' : '▸'} {label}
      </button>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <button className={`${cls} arrow`} aria-label="choose an action" title="choose an action">
            ▾
          </button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content className="ws-menu ws-action-menu" align="end" sideOffset={6} collisionPadding={8}>
            {CHOICES.map(entry)}
            {dev && (
              <DropdownMenu.Item
                className="ws-menu-item ws-action-item"
                disabled={!offered('decompile') || unclaimed === 0}
                onSelect={() => navigate('/product/not-described')}
                title="documents are written from the product for an area of the not-described list"
              >
                <b>{w.document}</b>
                <span className="state">
                  {whyNot(caps.data, 'decompile') ?? (unclaimed > 0 ? `${plural(unclaimed, 'file')} not described · choose the area` : 'Every product file is described')}
                </span>
              </DropdownMenu.Item>
            )}
            {dev && (
              <DropdownMenu.Item className="ws-menu-item ws-action-item" onSelect={() => navigate('/dev/board?preview=next')} title="the next session's prompt, exactly as the model receives it">
                <b>Preview prompt</b>
                <span className="state">the next batch</span>
              </DropdownMenu.Item>
            )}
            <DropdownMenu.Separator className="ws-menu-sep" />
            <DropdownMenu.Item className="ws-menu-item" onSelect={() => navigate('/settings')}>
              Settings…
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      {open && (
        <>
          <div className="ws-backdrop" onClick={() => setOpen(false)} />
          <ActionSheet kind={chosen} counts={counts} close={() => setOpen(false)} />
        </>
      )}
    </div>
  )
}

function ActionSheet({ kind, counts, close }: { kind: ActionChoice; counts: Counts; close: () => void }) {
  const w = useWords()
  const dev = useApp((a) => a.dev)
  const caps = useCapabilities()
  const { running, queued } = useRunning()
  const progress = useProgress()
  const { data: workers } = useWorkers()
  const src = useModelSource()
  const watchMode = useApp((a) => a.watchMode)
  const genMode = useApp((a) => a.genMode)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // A chat-tier model builds; the chooser stays folded until asked for.
  const [chooser, setChooser] = useState(false)

  const manual = watchMode !== 'watch'
  const agents = (workers?.workers ?? []).filter((x) => x.kind === 'agent')
  const prefersAgent = workers?.workflow.worker === 'agent'
  const agentMissing = prefersAgent && agents.length === 0
  const costs = counts.status?.costs
  const estimate = costs && (costs.sessions ?? 0) > 0 ? `About ${plural(costs.sessions ?? 0, 'session')} and ${Math.round((costs.tokens ?? 0) / 1000)}k tokens, as last time.` : null
  const server = caps.data?.backend === 'server'

  const run = async () => {
    setBusy(true)
    setError(null)
    try {
      // A release is control-plane state; a tab has none to record.
      const release = !server ? null : kind === 'compile' && manual ? 'compile' : kind === 'gen' && genMode === 'manual' ? 'generate' : null
      if (release) await post('/api/release', { stage: release })
      // A release with an attached, preferred agent wakes the agent; the internal
      // run is only queued when nobody else will act (dispatch).
      if (!(release && prefersAgent && agents.length > 0)) {
        if (kind === 'gen') await post('/api/jobs', { kind: 'gen', entities: [] })
        else if (kind === 'verify') await post('/api/jobs', { kind: 'verify', targets: [] })
        else await post('/api/jobs', { kind })
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const what =
    kind === 'compile'
      ? counts.changedDocs.length > 0
        ? `${plural(counts.changedDocs.length, 'document')} changed since the last build: ${counts.changedDocs.slice(0, 3).join(', ')}${counts.changedDocs.length > 3 ? ', …' : ''}.`
        : counts.openGoals > 0
          ? `${plural(counts.openGoals, 'item')} of work left from the last build.`
          : 'Nothing has changed since the last build.'
      : kind === 'gen'
        ? counts.toBuild > 0
          ? `${plural(counts.toBuild, 'entity', 'entities')} to generate from the documents.`
          : 'Nothing to generate: every entity is generated or has no requirements yet.'
        : counts.tests > 0
          ? `${plural(counts.tests, 'test')} to run; every trace row refreshes.`
          : 'No tests yet: generating writes one test per criterion.'
  const nothing = (kind === 'gen' && counts.toBuild === 0) || (kind === 'verify' && counts.tests === 0)
  const who = agentMissing
    ? 'Your settings prefer an attached agent, but none is connected. The built-in runner will run instead.'
    : agents.length > 0 && prefersAgent
      ? `${agents[0].client || 'an agent'} is attached and will run it.`
      : 'The built-in runner will run it.'

  return (
    <div className="ws-sheet">
      <h3>{actionWord(kind, w)}</h3>
      {running ? (
        <>
          <p className="why" style={{ margin: '2px 0' }}>
            {runningWord(running.kind.kind, w)}
            {progress ? ` ${progress}` : ''}
            {queued > 0 ? `, ${queued} queued` : ''}. Its thread is open in the chat column.
          </p>
          <div className="row">
            <button onClick={() => void post(`/api/jobs/${running.id}/cancel`)} title="stops at the next boundary; a model call in flight finishes first">
              Cancel
            </button>
          </div>
        </>
      ) : kind === 'compile' && needsBuildModel(caps.data) ? (
        <>
          <p className="why" style={{ margin: '2px 0' }}>
            {what}
          </p>
          {belowBuildTier(caps.data) && (
            <p className="why ws-tier" style={{ margin: '2px 0' }}>
              A build needs a model of 3B or more, or an endpoint; {loadedName(src)} answers questions.
            </p>
          )}
          <ModelSourcePanel />
        </>
      ) : (
        <>
          <p className="why" style={{ margin: '2px 0' }}>
            {what}
            {estimate ? ` ${estimate}` : ''}
          </p>
          {!server && <ModelLine />}
          <p className="why" style={{ margin: '2px 0' }}>
            {who}
          </p>
          {kind === 'compile' && smallBuildModel(caps.data) && (
            <>
              <p className="why ws-small" style={{ margin: '2px 0' }}>
                {loadedName(src)} is a small model: a build runs, and its results are rough. A 7B or larger model, or an endpoint, builds better.
              </p>
              {!chooser && (
                <button type="button" className="ws-stronger" onClick={() => setChooser(true)}>
                  Choose a stronger model
                </button>
              )}
              {chooser && <ModelSourcePanel />}
            </>
          )}
          {error && (
            <p className="why v-bad" style={{ margin: '2px 0' }}>
              {error}
            </p>
          )}
          <div className="row">
            <button className="run" disabled={busy || nothing} onClick={() => void run()}>
              {busy ? 'Starting…' : 'Run'}
            </button>
            <button onClick={close}>Cancel</button>
          </div>
        </>
      )}

      {dev && (
        <>
          <h3>Modes</h3>
          <div className="modes">
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
    </div>
  )
}

// The name of the model a source gives, in product words.
function loadedName(src: ModelSource): string {
  return src.kind === 'endpoint' ? (src.model ?? 'the endpoint') : src.kind === 'chrome' ? "the browser's built-in model" : modelName(webllmModelOf(src))
}

// The browser's model, once ready, with its tier and where to change it.
function ModelLine() {
  const src = useModelSource()
  const tier = sourceTier(src)
  return (
    <p className="why" style={{ margin: '2px 0' }}>
      Model: {loadedName(src)}, {tier} tier ({TIER_WORDS[tier]}). <Link to="/settings">Change</Link>
    </p>
  )
}
