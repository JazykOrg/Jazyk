// Build: the compiler console. Job history (live + transcripts on disk) newest
// first, and the selected job's transcript as turn groups, newest turn first,
// with the running turn pinned and highlighted at the top.
import { useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { get, post, type Job, type TraceEvent } from '../lib/api'
import { useApp } from '../lib/store'
import NodeLink, { linkifyIds } from '../components/NodeLink'
import './routes.css'

type LiveJob = Job & { stem?: string }

interface TraceMeta {
  id?: number
  kind?: { kind?: string }
  queuedAt?: string | null
  startedAt?: string | null
}

interface TraceOutcome {
  state: string
  result: Record<string, unknown> | null
  finishedAt?: string
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
      {active && (
        <p className="v-stale" style={{ margin: '2px 0' }}>
          working on {g.label || 'build'} · {g.rows.length} rows
        </p>
      )}
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

export default function Build() {
  const jobs = useApp((a) => a.jobs)
  const liveTrace = useApp((a) => a.trace)
  const [picked, setPicked] = useState<string | null>(null)

  const traces = useQuery({
    queryKey: ['jobs', 'history'],
    queryFn: () => get<{ traces: TraceListing[] }>('/api/trace'),
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
        }),
      )
    const rest = [...finished, ...history].sort((a, b) =>
      (b.startedAt ?? '').localeCompare(a.startedAt ?? ''),
    )
    return [...active, ...rest]
  }, [jobs, traces.data])

  // Pinned selection: a click sticks until that job disappears from the list.
  const sel = rows.find((r) => r.key === picked) ?? rows.find((r) => r.state === 'running') ?? rows[0]

  const stem = sel?.stem ?? ''
  const isRunning = sel?.state === 'running'
  const transcript = useQuery({
    queryKey: ['jobs', 'trace', stem],
    queryFn: () => get<Transcript>(`/api/trace/${stem}`),
    enabled: stem.length > 0,
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

  return (
    <div>
      <h1>Build</h1>
      {traces.error && (
        <p className="error-inline">
          {traces.error.message}{' '}
          <a href="#retry" onClick={(e) => { e.preventDefault(); traces.refetch() }}>retry</a>
        </p>
      )}
      {rows.length === 0 && !traces.error && (
        <p className="muted">no jobs yet, press compile in the status bar</p>
      )}
      <div className="joblist">
        {rows.map((r) => (
          <div
            key={r.key}
            className={`card ${sel && r.key === sel.key ? 'job-sel' : ''}`}
            onClick={() => setPicked(r.key)}
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

      {sel && (
        <>
          <h2>
            trace <span className="mono">{sel.stem || `#${sel.id}`}</span>
          </h2>
          {sel.state === 'queued' && <p className="muted">queued, waiting its turn</p>}
          {transcript.error && liveEvents.length === 0 && sel.state !== 'queued' && (
            <p className="error-inline">
              {transcript.error.message}{' '}
              <a href="#retry" onClick={(e) => { e.preventDefault(); transcript.refetch() }}>retry</a>
            </p>
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
  )
}
