// Build: the compiler console. Job list plus the live (or replayed) trace,
// grouped into collapsible turns.
import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { get, post, type Job, type TraceEvent } from '../lib/api'
import { useApp } from '../lib/store'
import NodeLink, { linkifyIds } from '../components/NodeLink'
import { verifyClass } from '../components/Chip'
import './routes.css'

function resultLine(job: Job): string | null {
  const r = job.result
  if (!r) return null
  if ('verdict' in r) {
    return `${r.verdict} · ${r.dirtyDocs} dirty docs · ${r.turns} turns · ${r.applied} applied · ${r.parked} parked · ${r.errors} err ${r.warnings} warn · ${r.coveragePct}% coverage`
  }
  // gen / verify / audit summaries: render scalar fields generically.
  return Object.entries(r)
    .filter(([, v]) => typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean')
    .map(([k, v]) => `${k} ${v}`)
    .join(' · ')
}

function ts(t: string | null): string {
  return t ? new Date(t).toLocaleTimeString() : ''
}

interface Turn {
  label: string
  header?: TraceEvent
  rows: TraceEvent[]
}

// Sequential grouping: a turnStart opens a new turn; a label change also splits.
function groupTurns(events: TraceEvent[]): Turn[] {
  const out: Turn[] = []
  let cur: Turn | null = null
  for (const ev of events) {
    const label = ev.label ?? ''
    if (ev.kind === 'turnStart' || !cur || (label && cur.label && label !== cur.label)) {
      cur = { label, header: ev.kind === 'turnStart' ? ev : undefined, rows: [] }
      out.push(cur)
      if (ev.kind === 'turnStart') continue
    }
    cur.rows.push(ev)
  }
  return out
}

function TraceRow({ ev }: { ev: TraceEvent }) {
  const s = (k: string) => String(ev[k] ?? '')
  switch (ev.kind) {
    case 'toolCall':
      return <div className="trace-row">→ {s('name')} {linkifyIds(s('summary'))}</div>
    case 'toolResult':
      return <div className="trace-row t-muted">← {linkifyIds(s('summary'))}</div>
    case 'toolError':
      return <div className="trace-row t-err">✗ {s('rule')}: {s('message')}</div>
    case 'modelText':
      return <div className="trace-row t-model">· {linkifyIds(s('text'))}</div>
    case 'turnDone':
      return (
        <div className="trace-row t-ok">
          ✓ staged {s('staged') || '0'} · {s('rounds') || '0'} rounds
          {ev.mode ? ` · ${s('mode')}` : ''}
          {ev.summary ? <> · {linkifyIds(s('summary'))}</> : null}
        </div>
      )
    case 'turnFailed':
      return <div className="trace-row t-err">✗ attempt {s('attempt')}: {s('error')}</div>
    case 'note':
      return <div className="trace-row t-muted">{linkifyIds(s('text'))}</div>
    case 'genEntityStart':
      return (
        <div className="trace-row">
          ▶ gen <NodeLink id={s('entity')} />{ev.stage ? ` · ${s('stage')}` : ''}
        </div>
      )
    case 'genEntitySkipped':
      return (
        <div className="trace-row t-muted">
          − skip <NodeLink id={s('entity')} />{ev.reason ? ` (${s('reason')})` : ''}
        </div>
      )
    case 'genEntityDone':
      return (
        <div className="trace-row t-ok">
          ✓ <NodeLink id={s('entity')} />
          {Array.isArray(ev.files) ? ` · ${(ev.files as string[]).join(', ')}` : ''}
        </div>
      )
    case 'genEntityFailed':
      return (
        <div className="trace-row t-err">
          ✗ <NodeLink id={s('entity')} /> {s('error')}
        </div>
      )
    case 'verifyRowStart':
      return (
        <div className="trace-row">
          ▶ verify <NodeLink id={s('requirement')} />{ev.run ? <span className="muted mono"> {s('run')}</span> : null}
        </div>
      )
    case 'verifyRowDone':
      return (
        <div className={`trace-row ${verifyClass(s('status') || s('verdict'))}`}>
          ✓ <NodeLink id={s('requirement')} /> {s('verdict') || s('status')}
          {ev.evidence ? <span className="muted"> · {s('evidence')}</span> : null}
        </div>
      )
    case 'verifyRowStale':
      return (
        <div className="trace-row t-warn">
          ~ <NodeLink id={s('requirement')} /> stale{ev.reason ? `: ${s('reason')}` : ''}
        </div>
      )
    case 'verifyRowError':
      return (
        <div className="trace-row t-err">
          ✗ <NodeLink id={s('requirement')} /> {s('message') || s('reason')}
        </div>
      )
    default:
      return <div className="trace-row t-muted">{ev.kind}{ev.label ? ` ${String(ev.label)}` : ''}</div>
  }
}

export default function Build() {
  const jobs = useApp((a) => a.jobs)
  const trace = useApp((a) => a.trace)
  const [picked, setPicked] = useState<number | null>(null)

  const list = Object.values(jobs).sort((a, b) => b.id - a.id)
  const running = list.find((j) => j.state === 'running')
  const sel = picked ?? running?.id ?? list[0]?.id ?? null
  const job = sel !== null ? jobs[sel] : undefined

  const liveRows = useMemo(
    () => trace.filter((r) => r.jobId === sel).map((r) => r.event),
    [trace, sel],
  )
  // Older jobs have no live rows; replay the buffered ring from the server.
  const replay = useQuery({
    queryKey: ['jobs', sel],
    queryFn: () => get<Job>(`/api/jobs/${sel}`),
    enabled: sel !== null && liveRows.length === 0,
  })
  const events = liveRows.length > 0 ? liveRows : (replay.data?.events ?? []).map((e) => e.event)
  const turns = useMemo(() => groupTurns(events), [events])
  const isRunning = job?.state === 'running'

  // Auto-scroll while running, paused when the user scrolls up.
  const boxRef = useRef<HTMLDivElement>(null)
  const stick = useRef(true)
  useEffect(() => {
    const el = boxRef.current
    if (el && isRunning && stick.current) el.scrollTop = el.scrollHeight
  }, [events.length, isRunning])

  return (
    <div>
      <h1>Build</h1>
      {list.length === 0 && <p className="muted">no jobs yet, press compile in the status bar</p>}
      <div className="joblist">
        {list.map((j) => (
          <div
            key={j.id}
            className={`card ${j.id === sel ? 'job-sel' : ''}`}
            onClick={() => setPicked(j.id)}
          >
            <div className="row">
              <span className="mono">#{j.id}</span>
              <b>{j.kind.kind}</b>
              <span className={j.state === 'failed' ? 'v-bad' : j.state === 'running' ? 'v-stale' : 'muted'}>
                {j.state}
              </span>
              <span className="muted mono">
                {ts(j.queuedAt)}
                {j.startedAt ? ` → ${ts(j.startedAt)}` : ''}
                {j.finishedAt ? ` → ${ts(j.finishedAt)}` : ''}
              </span>
              {(j.state === 'queued' || j.state === 'running') && (
                <button
                  onClick={(e) => {
                    e.stopPropagation()
                    post(`/api/jobs/${j.id}/cancel`)
                  }}
                >
                  cancel
                </button>
              )}
            </div>
            {resultLine(j) && <div className="muted mono oneline">{resultLine(j)}</div>}
          </div>
        ))}
      </div>

      {job && (
        <>
          <h2>trace #{job.id}</h2>
          {replay.error && liveRows.length === 0 && (
            <p className="error-inline">
              {replay.error.message}{' '}
              <a href="#retry" onClick={(e) => { e.preventDefault(); replay.refetch() }}>retry</a>
            </p>
          )}
          {events.length === 0 && !replay.error && <p className="muted">no trace for this job</p>}
          <div
            className="trace"
            ref={boxRef}
            onScroll={(e) => {
              const el = e.currentTarget
              stick.current = el.scrollTop + el.clientHeight >= el.scrollHeight - 40
            }}
          >
            {turns.map((t, i) => (
              <details key={i} className="card" open={i === turns.length - 1}>
                <summary className="mono">
                  {t.label || `turn ${i + 1}`}
                  {t.header ? (
                    <span className="muted">
                      {' '}· dirty {String(t.header.dirty ?? 0)} · stale {String(t.header.stale ?? 0)}
                    </span>
                  ) : null}
                </summary>
                {t.rows.map((ev, k) => (
                  <TraceRow key={k} ev={ev} />
                ))}
              </details>
            ))}
          </div>
        </>
      )}
    </div>
  )
}
