// The activity panel: what were the Build and Journal tabs, merged. A run is one
// job plus what it committed: the trace turns and the journal entries in the
// run's generation span. Collapsed, the panel is the control line: compile,
// generate, verify, the watch and generation modes, the running job
// (docs/frontends/gui.md#activity).
import { useMemo, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { get, post, put, type Job, type JournalEntry, type TraceEvent } from '../lib/api'
import { useDocs, useGenPending, useJournal, useStatus } from '../lib/queries'
import { useApp } from '../lib/store'
import NodeLink, { linkifyIds } from '../components/NodeLink'
import '../routes/routes.css'

type LiveJob = Job & { stem?: string }

interface TraceMeta {
  id?: number
  kind?: { kind?: string }
  queuedAt?: string | null
  startedAt?: string | null
  source?: string
  generation?: number
}

interface TraceOutcome {
  state: string
  result: Record<string, unknown> | null
  finishedAt?: string
  generation?: number
}

interface TraceListing {
  stem: string
  meta: TraceMeta | null
  outcome: TraceOutcome | null
  events: number
}

interface Transcript {
  meta: TraceMeta | null
  outcome: TraceOutcome | null
  events: { n: number; event: TraceEvent }[]
}

interface Row {
  key: string
  stem: string
  id: number | null
  kind: string
  state: string
  startedAt: string | null
  result: Record<string, unknown> | null
  live: LiveJob | null
  fromGen: number | null
  toGen: number | null
}

function resultLine(result: Record<string, unknown> | null, state: string): string | null {
  if (!result) return state === 'died' ? 'died mid-run (no outcome recorded)' : null
  if ('verdict' in result) {
    return `${result.verdict} · ${result.dirtyDocs} dirty docs · ${result.turns} turns · ${result.applied} applied · ${result.parked} parked · ${result.errors} err ${result.warnings} warn · ${result.coveragePct}% coverage`
  }
  return Object.entries(result)
    .filter(([, v]) => typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean')
    .map(([k, v]) => `${k} ${v}`)
    .join(' · ')
}

const ts = (t: string | null | undefined) => (t ? new Date(t).toLocaleTimeString() : '')

// The transcript is the baseline; live zustand rows extend it past its fetch
// point. Live rows carry no per-job n, so the seam is found by matching the
// baseline's last event in the live tail.
function mergeLive(base: TraceEvent[], live: TraceEvent[]): TraceEvent[] {
  if (live.length === 0) return base
  if (base.length === 0) return live
  const lastKey = JSON.stringify(base[base.length - 1])
  for (let i = live.length - 1; i >= 0; i--) {
    if (JSON.stringify(live[i]) === lastKey) return [...base, ...live.slice(i + 1)]
  }
  // No overlap: one side is strictly ahead of the other; show the longer view.
  return live.length > base.length ? live : base
}

interface Turn {
  label: string
  start?: TraceEvent
  rows: TraceEvent[]
  done?: TraceEvent
  failed?: TraceEvent
}

// Chronological events into turn groups. A turnStart opens a group; unlabeled
// events outside any turn (notes, gen*, verifyRow*) pool under "build".
function groupTurns(events: TraceEvent[]): Turn[] {
  const out: Turn[] = []
  let cur: Turn | null = null
  const synthetic = (ev: TraceEvent) => {
    const last = out[out.length - 1]
    if (last && !last.start && last.label === 'build') last.rows.push(ev)
    else out.push({ label: 'build', rows: [ev] })
  }
  for (const ev of events) {
    if (ev.kind === 'turnStart') {
      cur = { label: ev.label ?? '', start: ev, rows: [] }
      out.push(cur)
    } else if (ev.kind === 'turnDone' || ev.kind === 'turnFailed') {
      if (cur) {
        if (ev.kind === 'turnDone') cur.done = ev
        else cur.failed = ev
        cur = null
      } else synthetic(ev)
    } else if (cur && (!ev.label || ev.label === cur.label)) {
      cur.rows.push(ev)
    } else if (ev.label) {
      cur = { label: ev.label, rows: [ev] }
      out.push(cur)
    } else {
      synthetic(ev)
    }
  }
  return out
}

function pretty(raw: string): string {
  try {
    return JSON.stringify(JSON.parse(raw), null, 2)
  } catch {
    return raw
  }
}

const CONDENSE = 200

// One trace row, always visible; ▸ expands the full payload when there is one.
function TraceRow({ ev }: { ev: TraceEvent }) {
  const [open, setOpen] = useState(false)
  const s = (k: string) => String(ev[k] ?? '')
  let cls = ''
  let line: React.ReactNode
  let full: string | null = typeof ev.full === 'string' ? ev.full : null

  switch (ev.kind) {
    case 'toolCall':
      line = <>→ {s('name')} {linkifyIds(s('summary'))}</>
      break
    case 'toolResult':
      cls = 't-muted'
      line = <>← {linkifyIds(s('summary'))}</>
      break
    case 'toolError':
      cls = 't-err'
      line = <>✗ {s('rule')}: {s('message')}</>
      break
    case 'modelText': {
      cls = 't-model'
      const text = s('text')
      if (text.length > CONDENSE) {
        full = text
        line = <>· {linkifyIds(text.slice(0, CONDENSE).replace(/\s+/g, ' '))}…</>
      } else {
        line = <>· {linkifyIds(text)}</>
      }
      break
    }
    case 'note':
      cls = 't-muted'
      line = <>{linkifyIds(s('text'))}</>
      break
    case 'genEntityStart':
      line = <>▶ gen <NodeLink id={s('entity')} />{ev.stage ? ` · ${s('stage')}` : ''}</>
      break
    case 'genEntitySkipped':
      cls = 't-muted'
      line = <>− skip <NodeLink id={s('entity')} />{ev.reason ? ` (${s('reason')})` : ''}</>
      break
    case 'genEntityDone':
      cls = 't-ok'
      line = (
        <>
          ✓ <NodeLink id={s('entity')} />
          {Array.isArray(ev.files) ? ` · ${(ev.files as string[]).join(', ')}` : ''}
        </>
      )
      break
    case 'genEntityFailed':
      cls = 't-err'
      line = <>✗ <NodeLink id={s('entity')} /> {s('error')}</>
      break
    case 'verifyRowStart':
      line = (
        <>
          ▶ verify <NodeLink id={s('requirement')} />
          {ev.run ? <span className="muted mono"> {s('run')}</span> : null}
        </>
      )
      break
    case 'verifyRowDone':
      cls = 't-ok'
      line = (
        <>
          ✓ <NodeLink id={s('requirement')} /> {s('verdict') || s('status')}
          {ev.evidence ? <span className="muted"> · {s('evidence')}</span> : null}
        </>
      )
      break
    case 'verifyRowStale':
      cls = 't-warn'
      line = <>~ <NodeLink id={s('requirement')} /> stale{ev.reason ? `: ${s('reason')}` : ''}</>
      break
    case 'verifyRowError':
      cls = 't-err'
      line = <>✗ <NodeLink id={s('requirement')} /> {s('message') || s('reason')}</>
      break
    default:
      cls = 't-muted'
      line = <>{ev.kind}</>
  }

  return (
    <>
      <div className={`trace-row ${cls}`}>
        {full !== null && (
          <button className="expand" onClick={() => setOpen(!open)}>
            {open ? '▾' : '▸'}
          </button>
        )}
        {line}
      </div>
      {open && full !== null && <pre className="pack trace-full">{pretty(full)}</pre>}
    </>
  )
}

function TurnCard({ g, active }: { g: Turn; active: boolean }) {
  const sp = g.label.indexOf(' ')
  const task = sp > 0 ? g.label.slice(0, sp) : g.label || 'build'
  const target = sp > 0 ? g.label.slice(sp + 1) : ''
  const count = g.rows.length + (g.start ? 1 : 0) + (g.done || g.failed ? 1 : 0)
  return (
    <div className={`card ${active ? 'turn-active' : ''}`}>
      <div className="row">
        <b>{task}</b>
        {target && <span className="mono">{target}</span>}
        {active && <span className="chip v-stale">running</span>}
        {g.done && (
          <span className="chip v-ok">
            staged {String(g.done.staged ?? 0)} · {String(g.done.rounds ?? 0)} rounds
          </span>
        )}
        {g.failed && <span className="chip v-bad">failed</span>}
        {!active && !g.done && !g.failed && <span className="chip sev-none">unfinished</span>}
        <span className="muted">{count} events</span>
      </div>
      {g.start && (
        <div className="trace-row t-muted">
          dirty {String(g.start.dirty ?? 0)} · stale {String(g.start.stale ?? 0)}
        </div>
      )}
      {g.rows.map((ev, i) => (
        <TraceRow key={i} ev={ev} />
      ))}
      {g.done?.summary ? (
        <div className="trace-row t-ok">✓ {linkifyIds(String(g.done.summary))}</div>
      ) : null}
      {g.failed && (
        <div className="trace-row t-err">
          ✗ attempt {String(g.failed.attempt ?? '')}: {String(g.failed.error ?? '')}
        </div>
      )}
    </div>
  )
}

// +created ~updated/merged -deleted, by op prefix.
function opCounts(mutations: Record<string, unknown>[]): [number, number, number] {
  let a = 0
  let u = 0
  let d = 0
  for (const m of mutations) {
    const op = String(m.op ?? '')
    if (op.startsWith('create')) a++
    else if (op.startsWith('update') || op.startsWith('merge')) u++
    else if (op.startsWith('delete')) d++
  }
  return [a, u, d]
}

// The changesets the run committed: journal entries inside its generation span.
function Changesets({ from, to }: { from: number; to: number }) {
  const journal = useJournal(500)
  if (to <= from) return null
  const entries = (journal.data?.entries ?? []).filter(
    (e) => e.generation > from && e.generation <= to,
  )
  if (entries.length === 0) return null
  return (
    <div className="wb-changesets">
      <div className="row">
        <b>committed</b>
        <span className="muted">
          g{from + 1}..g{to}
        </span>
        {to > from + 1 && (
          <Link to={`/journal/diff?from=${from + 1}&to=${to}`}>release diff</Link>
        )}
      </div>
      {entries.map((e: JournalEntry) => {
        const [a, u, d] = opCounts(e.mutations)
        return (
          <details key={e.generation} className="wb-changeset-row">
            <summary>
              <Link to={`/journal/${e.generation}`}>g{e.generation}</Link> · {e.workItem.task} ·{' '}
              {e.workItem.target} · <span className="v-ok">+{a}</span>{' '}
              <span className="sev-info">~{u}</span> <span className="v-bad">-{d}</span> ·{' '}
              {e.tokens} tok
            </summary>
            {e.mutations.map((m, i) => {
              const id = typeof m.id === 'string' ? m.id : null
              const reasoning = typeof m.reasoning === 'string' ? m.reasoning : null
              return (
                <p key={i} style={{ margin: '1px 0 1px 16px' }}>
                  <span className="chip sev-none">{String(m.op ?? '')}</span>
                  {id && <NodeLink id={id} />}
                  {reasoning && <span className="muted"> {reasoning}</span>}
                </p>
              )
            })}
          </details>
        )
      })}
    </div>
  )
}

// The control line: run actions and the automatic modes, visible even collapsed.
function ControlBar({ open, setOpen }: { open: boolean; setOpen: (v: boolean) => void }) {
  const { data: s } = useStatus()
  const { data: docs } = useDocs()
  const pending = useGenPending()
  const jobs = useApp((a) => a.jobs)
  const watchMode = useApp((a) => a.watchMode)
  const genMode = useApp((a) => a.genMode)
  const running = Object.values(jobs).find((j) => j.state === 'running')
  const queued = Object.values(jobs).filter((j) => j.state === 'queued').length
  const changedDocs = (docs?.docs ?? []).filter((d) => d.stale).length
  const genPending = pending.data?.pending.length ?? 0

  return (
    <div className="wb-activity-bar">
      <button className="toggle" onClick={() => setOpen(!open)} title="run history">
        {open ? '▾' : '▸'} activity
      </button>
      {running ? (
        <span className="v-stale">
          ▶ {running.kind.kind} running{queued > 0 ? ` (+${queued} queued)` : ''}
          <button
            style={{ marginLeft: 8 }}
            onClick={() => post(`/api/jobs/${running.id}/cancel`)}
          >
            cancel
          </button>
        </span>
      ) : (
        <span className="muted">{s?.verdict || 'no build yet'}</span>
      )}
      <span className="bar-right">
        <label className="muted" title="off: changes only update badges · queue: changes queue, compiling is a click · watch: compile on change (spends LLM budget)">
          compile
          <select value={watchMode} onChange={(e) => put('/api/watch', { mode: e.target.value })}>
            <option value="off">off</option>
            <option value="queue">manual</option>
            <option value="watch">auto</option>
          </select>
        </label>
        <label className="muted" title="manual: generation runs on click · auto: a finished compile with pending entities queues a gen job (spends LLM budget)">
          gen
          <select value={genMode} onChange={(e) => put('/api/watch', { gen: e.target.value })}>
            <option value="manual">manual</option>
            <option value="auto">auto</option>
          </select>
        </label>
        <button disabled={!!running} onClick={() => post('/api/jobs', { kind: 'compile' })}>
          compile{changedDocs > 0 ? ` ${changedDocs}` : ''} ▸
        </button>
        <button
          disabled={!!running || genPending === 0}
          onClick={() => post('/api/jobs', { kind: 'gen', entities: [] })}
          title="generate the pending entities"
        >
          generate{genPending > 0 ? ` ${genPending}` : ''} ▸
        </button>
        <button
          disabled={!!running}
          onClick={() => post('/api/jobs', { kind: 'verify', targets: [] })}
        >
          verify ▸
        </button>
      </span>
    </div>
  )
}

export default function Activity() {
  const open = useApp((a) => a.activityOpen)
  const setOpen = useApp((a) => a.setActivityOpen)
  const jobs = useApp((a) => a.jobs)
  const liveTrace = useApp((a) => a.trace)
  const [sp, setSp] = useSearchParams()
  const picked = sp.get('run')

  const traces = useQuery({
    queryKey: ['jobs', 'history'],
    queryFn: () => get<{ traces: TraceListing[] }>('/api/trace'),
    enabled: open,
  })

  const rows = useMemo<Row[]>(() => {
    const live = Object.values(jobs) as LiveJob[]
    const liveRow = (j: LiveJob): Row => ({
      key: j.stem || `job-${j.id}`,
      stem: j.stem ?? '',
      id: j.id,
      kind: j.kind.kind,
      state: j.state,
      startedAt: j.startedAt,
      result: j.result,
      live: j,
      fromGen: null,
      toGen: null,
    })
    const active = live
      .filter((j) => j.state === 'running' || j.state === 'queued')
      .sort((a, b) => b.id - a.id)
      .map(liveRow)
    const liveStems = new Set(live.map((j) => j.stem).filter(Boolean))
    const finished = live
      .filter((j) => j.state !== 'running' && j.state !== 'queued')
      .map(liveRow)
    const history = (traces.data?.traces ?? [])
      .filter((t) => !liveStems.has(t.stem))
      .map(
        (t): Row => ({
          key: t.stem,
          stem: t.stem,
          id: t.meta?.id ?? null,
          kind: t.meta?.kind?.kind ?? '?',
          state: t.outcome?.state ?? 'died',
          startedAt: t.meta?.startedAt ?? null,
          result: t.outcome?.result ?? null,
          live: null,
          fromGen: t.meta?.generation ?? null,
          toGen: t.outcome?.generation ?? null,
        }),
      )
    const rest = [...finished, ...history].sort((a, b) =>
      (b.startedAt ?? '').localeCompare(a.startedAt ?? ''),
    )
    return [...active, ...rest]
  }, [jobs, traces.data])

  // Pinned selection: a click sticks until that run disappears from the list.
  const sel = rows.find((r) => r.key === picked) ?? rows.find((r) => r.state === 'running') ?? rows[0]

  const stem = sel?.stem ?? ''
  const isRunning = sel?.state === 'running'
  const transcript = useQuery({
    queryKey: ['jobs', 'trace', stem],
    queryFn: () => get<Transcript>(`/api/trace/${stem}`),
    enabled: open && stem.length > 0,
    // Slow safety refetch while running; live SSE rows fill the gap in between.
    refetchInterval: isRunning ? 5000 : false,
  })

  const selId = sel?.id ?? null
  const selLive = sel?.live != null
  const liveEvents = useMemo(
    () => (selLive ? liveTrace.filter((r) => r.jobId === selId).map((r) => r.event) : []),
    [liveTrace, selLive, selId],
  )
  const events = useMemo(
    () => mergeLive((transcript.data?.events ?? []).map((e) => e.event), liveEvents),
    [transcript.data, liveEvents],
  )
  const turns = useMemo(() => groupTurns(events), [events])
  const lastTurn = turns.length > 0 ? turns[turns.length - 1] : null
  const activeTurn = isRunning && lastTurn && !lastTurn.done && !lastTurn.failed ? lastTurn : null

  const status = useStatus()
  const fromGen = sel?.fromGen ?? transcript.data?.meta?.generation ?? null
  const toGen =
    sel?.toGen ??
    transcript.data?.outcome?.generation ??
    (isRunning ? (status.data?.generation ?? null) : null)

  const pick = (key: string) => {
    const next = new URLSearchParams(sp)
    next.set('run', key)
    setSp(next, { replace: true })
  }

  return (
    <div className={`wb-activity${open ? ' open' : ''}`}>
      <ControlBar open={open} setOpen={setOpen} />
      {open && (
        <div className="wb-activity-body">
          <div className="wb-runlist">
            {traces.error && <p className="error-inline">{traces.error.message}</p>}
            {rows.length === 0 && !traces.error && (
              <p className="muted">no runs yet, press compile</p>
            )}
            {rows.map((r) => (
              <div
                key={r.key}
                className={`card ${sel && r.key === sel.key ? 'job-sel' : ''}`}
                onClick={() => pick(r.key)}
              >
                <div className="row">
                  <b>{r.kind}</b>
                  <span
                    className={
                      r.state === 'failed' || r.state === 'died'
                        ? 'v-bad'
                        : r.state === 'running'
                          ? 'v-stale'
                          : 'muted'
                    }
                  >
                    {r.state}
                  </span>
                  <span className="muted mono">{ts(r.startedAt)}</span>
                  {r.live && (r.state === 'queued' || r.state === 'running') && (
                    <button
                      onClick={(e) => {
                        e.stopPropagation()
                        post(`/api/jobs/${r.id}/cancel`)
                      }}
                    >
                      cancel
                    </button>
                  )}
                </div>
                {resultLine(r.result, r.state) && (
                  <div className="muted mono oneline">{resultLine(r.result, r.state)}</div>
                )}
              </div>
            ))}
          </div>

          <div className="wb-rundetail">
            {!sel && <p className="muted">select a run</p>}
            {sel && (
              <>
                <div className="row">
                  <b>{sel.kind}</b>
                  <span className="mono muted">{sel.stem || `#${sel.id}`}</span>
                  {sel.state === 'queued' && <span className="muted">queued, waiting its turn</span>}
                </div>
                {fromGen !== null && toGen !== null && (
                  <Changesets from={fromGen} to={toGen} />
                )}
                {transcript.error && liveEvents.length === 0 && sel.state !== 'queued' && (
                  <p className="error-inline">{transcript.error.message}</p>
                )}
                {stem.length > 0 && !transcript.data && !transcript.error && liveEvents.length === 0 && (
                  <p className="muted">loading…</p>
                )}
                {events.length === 0 && transcript.data && sel.state !== 'queued' && (
                  <p className="muted">no events in this transcript</p>
                )}
                {[...turns].reverse().map((g, i) => (
                  <TurnCard key={turns.length - 1 - i} g={g} active={g === activeTurn} />
                ))}
              </>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
