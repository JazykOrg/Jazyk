// The activity panel: what were the Build and Journal tabs, merged. A run is one
// job plus what it committed: the trace turns and the journal entries in the
// run's generation span. Collapsed, the panel is the control line: compile,
// generate, verify, the watch and generation modes, the running job
// (docs/frontends/gui.md#activity).
import { useMemo, useState } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { entryLabel, get, post, put, verdictText, type Job, type JournalEntry, type TraceEvent } from '../lib/api'
import { useDocs, useGenPending, useJournal, useStatus, useWorkers } from '../lib/queries'
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

// Every event is numbered per run: the number is how the full payload is fetched
// when a row is elided (docs/frontends/gui.md#jobs).
interface Numbered {
  n: number
  event: TraceEvent
}

interface Transcript {
  meta: TraceMeta | null
  outcome: TraceOutcome | null
  events: Numbered[]
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
    // The BuildReport: the verdict with its counts (docs/frontends/cli.md#jazyk-compile).
    return `${result.verdict} · ${result.goals} goals · ${result.sessions} sessions · ${result.applied} applied · ${result.parked} parked ${result.failed} failed ${result.blocked} blocked · ${result.errors} err ${result.warnings} warn · ${result.coveragePct}% coverage`
  }
  return Object.entries(result)
    .filter(([, v]) => typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean')
    .map(([k, v]) => `${k} ${v}`)
    .join(' · ')
}

const ts = (t: string | null | undefined) => (t ? new Date(t).toLocaleTimeString() : '')

// The transcript is the baseline; live rows extend it past its fetch point. Both
// sides carry the per-run event number, so the seam is arithmetic.
function mergeLive(base: Numbered[], live: Numbered[]): Numbered[] {
  if (live.length === 0) return base
  if (base.length === 0) return live
  const last = base[base.length - 1].n
  return [...base, ...live.filter((r) => r.n > last)]
}

// One model call and everything the harness did with its answer.
interface Round {
  step: string
  request?: Numbered
  response?: Numbered
  rows: Numbered[]
}

interface Turn {
  key: string
  label: string
  start?: TraceEvent
  // Rows before the first model call of the turn (notes, worker events).
  preRows: Numbered[]
  rounds: Round[]
  done?: TraceEvent
  failed?: TraceEvent
}

// The work an event belongs to. Turn events carry the label; the workers name
// their entity or requirement, which is the same key their model calls use.
function labelOf(ev: TraceEvent): string | null {
  if (typeof ev.label === 'string' && ev.label !== '') return ev.label
  if (typeof ev.entity === 'string') return `gen ${ev.entity}`
  if (typeof ev.requirement === 'string') return `verify ${ev.requirement}`
  return null
}

// Chronological events into one group per label (the batch id), each group into
// rounds. Parallel work interleaves on the wire; the label puts it back together.
// A second sessionStart for a label opens a new group: that is a retry, not more
// of the same session. Events with no label (build notes) pool under "build".
function groupTurns(events: Numbered[]): Turn[] {
  const out: Turn[] = []
  const open = new Map<string, Turn>()
  const groupFor = (label: string): Turn => {
    const g = open.get(label)
    if (g) return g
    const fresh: Turn = { key: `${label}#${out.length}`, label, preRows: [], rounds: [] }
    open.set(label, fresh)
    out.push(fresh)
    return fresh
  }
  for (const row of events) {
    const ev = row.event
    const label = labelOf(ev) ?? 'build'
    if (ev.kind === 'sessionStart') {
      const fresh: Turn = { key: `${label}#${out.length}`, label, start: ev, preRows: [], rounds: [] }
      open.set(label, fresh)
      out.push(fresh)
      continue
    }
    const g = groupFor(label)
    switch (ev.kind) {
      case 'sessionDone':
        g.done = ev
        open.delete(label)
        break
      case 'sessionFailed':
        g.failed = ev
        open.delete(label)
        break
      case 'llmRequest':
        g.rounds.push({ step: String(ev.step ?? ''), request: row, rows: [] })
        break
      case 'llmResponse': {
        const r = g.rounds[g.rounds.length - 1]
        if (r) r.response = row
        else g.rounds.push({ step: String(ev.step ?? ''), response: row, rows: [] })
        break
      }
      default: {
        const r = g.rounds[g.rounds.length - 1]
        if (r) r.rows.push(row)
        else g.preRows.push(row)
      }
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

// The full event behind an elided one. Nothing is fetched until a row is opened,
// and the answer is cached by run and event number.
function useFullEvent(stem: string, n: number | null, enabled: boolean) {
  return useQuery({
    queryKey: ['trace', 'event', stem, n],
    queryFn: () => get<{ n: number; event: TraceEvent }>(`/api/trace/${stem}/${n}`),
    enabled: enabled && stem.length > 0 && n !== null,
    staleTime: Infinity,
  })
}

// A payload panel: the elided copy renders immediately, the full one replaces it
// when it arrives.
function Payload({ text, loading }: { text: string; loading: boolean }) {
  return (
    <pre className="pack trace-full">
      {loading ? `${text}\n\n(loading the full payload…)` : text}
    </pre>
  )
}

// One message of a request, collapsed to its role and opening words.
function MessageRow({ m }: { m: Record<string, unknown> }) {
  const role = String(m.role ?? '?')
  // A tool-calling reply carries a null content beside its calls; show the calls,
  // not the word "null".
  const content =
    typeof m.content === 'string'
      ? m.content
      : m.content == null
        ? ''
        : JSON.stringify(m.content, null, 2)
  const calls = m.tool_calls ? JSON.stringify(m.tool_calls, null, 2) : null
  const reasoning = typeof m.reasoning_content === 'string' ? m.reasoning_content : null
  const head = content.replace(/\s+/g, ' ').slice(0, 90)
  return (
    <details className="msg">
      <summary>
        <span className={`chip msg-${role}`}>{role}</span>
        <span className="muted mono"> {content.length} chars</span>
        <span className="muted"> {head}</span>
      </summary>
      {reasoning && <pre className="pack msg-body t-model">{reasoning}</pre>}
      {content && <pre className="pack msg-body">{content}</pre>}
      {calls && <pre className="pack msg-body">{calls}</pre>}
    </details>
  )
}

// The request behind a round: what the model was actually asked, message by
// message (docs/frontends/gui.md#activity).
function RequestView({ stem, row }: { stem: string; row: Numbered }) {
  const elided = row.event.elided === true
  const full = useFullEvent(stem, row.n, elided)
  const ev = (full.data?.event ?? row.event) as TraceEvent
  const messages = Array.isArray(ev.messages) ? (ev.messages as Record<string, unknown>[]) : []
  const tools = Array.isArray(ev.tools) ? (ev.tools as string[]) : []
  return (
    <div className="round-body">
      <div className="trace-row t-muted">
        model {String(ev.model ?? '')} · {messages.length} messages
        {tools.length > 0 ? ` · tools: ${tools.join(', ')}` : ' · no tools offered'}
        {elided && full.isLoading ? ' · loading full payload…' : ''}
        {full.error ? ` · could not load the full payload: ${full.error.message}` : ''}
      </div>
      {messages.map((m, i) => (
        <MessageRow key={i} m={m} />
      ))}
    </div>
  )
}

// The reply as it arrived, reasoning field and tool calls included.
function ResponseView({ stem, row }: { stem: string; row: Numbered }) {
  const elided = row.event.elided === true
  const full = useFullEvent(stem, row.n, elided)
  const ev = (full.data?.event ?? row.event) as TraceEvent
  const m = (ev.message ?? {}) as Record<string, unknown>
  return (
    <div className="round-body">
      <div className="trace-row t-muted">
        answer · {String(ev.ms ?? 0)} ms · {String(ev.tokens ?? 0)} tokens
        {elided && full.isLoading ? ' · loading full payload…' : ''}
      </div>
      <MessageRow m={m} />
    </div>
  )
}

// One round: the header is its arithmetic, always visible; ▸ opens the prompt and
// the answer. The tool rows below it stay visible either way, so the reading order
// is what happened, with the detail one click away.
function RoundCard({ stem, r, index }: { stem: string; r: Round; index: number }) {
  const [open, setOpen] = useState(false)
  const req = r.request?.event
  const res = r.response?.event
  const messages = Array.isArray(req?.messages) ? (req.messages as unknown[]).length : 0
  const chars = typeof req?.messages === 'object' ? JSON.stringify(req?.messages ?? '').length : 0
  const calls = r.rows.filter((x) => x.event.kind === 'toolCall').length
  const errors = r.rows.filter((x) => x.event.kind === 'toolError').length
  return (
    <div className="round">
      <div className="round-head">
        <button className="expand" onClick={() => setOpen(!open)} title="prompt and answer">
          {open ? '▾' : '▸'}
        </button>
        <b className="mono">{r.step || `#${index + 1}`}</b>
        {messages > 0 && (
          <span className="muted mono">
            {messages} msg · {Math.round(chars / 100) / 10}k chars
          </span>
        )}
        {res ? (
          <span className="muted mono">
            {String(res.ms ?? 0)} ms · {String(res.tokens ?? 0)} tok
          </span>
        ) : (
          <span className="chip v-stale">waiting</span>
        )}
        {calls > 0 && <span className="muted mono">{calls} calls</span>}
        {errors > 0 && <span className="v-bad mono">{errors} rejected</span>}
      </div>
      {open && r.request && <RequestView stem={stem} row={r.request} />}
      {open && r.response && <ResponseView stem={stem} row={r.response} />}
      {r.rows.map((row) => (
        <TraceRow key={row.n} ev={row.event} stem={stem} n={row.n} />
      ))}
    </div>
  )
}

// One trace row, always visible; ▸ expands the full payload when there is one.
function TraceRow({ ev, stem, n }: { ev: TraceEvent; stem: string; n: number }) {
  const [open, setOpen] = useState(false)
  const s = (k: string) => String(ev[k] ?? '')
  const elided = ev.elided === true
  const fullEv = useFullEvent(stem, n, open && elided)
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
    case 'section':
      cls = 't-muted'
      line = <>§ {s('section')} <span className="muted">({s('tool')})</span></>
      break
    case 'llmRetry':
      cls = 't-warn'
      line = (
        <>
          ↻ attempt {s('attempt')} failed, retrying in {Number(ev.waitMs ?? 0) / 1000}s: {s('error')}
        </>
      )
      break
    case 'board': {
      cls = 't-muted'
      const kinds = Array.isArray(ev.kinds) ? (ev.kinds as [string, number][]) : []
      line = (
        <>
          ▷ compile: {String(ev.goals ?? 0)} goals
          {kinds.length > 0 ? ` (${kinds.map(([k, n]) => `${n} ${k}`).join(', ')})` : ''}
          {Number(ev.blocked ?? 0) > 0 ? `, ${String(ev.blocked)} blocked` : ''}
        </>
      )
      break
    }
    case 'batchStart': {
      cls = 't-muted'
      const goals = Array.isArray(ev.goals) ? (ev.goals as { id?: string; kind?: string; target?: string }[]) : []
      line = (
        <>
          ▷ batch {s('label')}: {s('class')}
          {ev.tier != null ? ` tier ${String(ev.tier)}` : ''} · {goals.length} goal(s)
          {typeof ev.executor === 'string' ? ` · ${String(ev.executor)}` : ''}
        </>
      )
      full = goals.map((g) => `${g.kind} ${g.target}`).join('\n') || null
      break
    }
    case 'gcBurst':
      cls = 't-warn'
      line = (
        <>
          ⟳ gc burst: {s('goalKind')} {s('target')} ({String(ev.count ?? '')} &gt; {String(ev.limit ?? '')})
        </>
      )
      break
    case 'goal': {
      const event = s('event')
      cls = event === 'resolved' ? 't-ok' : event === 'failed' ? 't-err' : 't-muted'
      const tail = s('justification') || s('reason')
      line = (
        <>
          {event === 'resolved' ? '✓' : event === 'failed' ? '✗' : '◈'} {event} {linkifyIds(s('goal'))}
          {tail ? <span className="muted"> · {linkifyIds(tail)}</span> : null}
        </>
      )
      break
    }
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

  // An elided row expands to the payload as it was recorded, not to the preview.
  const fullText = (() => {
    if (!open) return null
    const src = fullEv.data?.event
    if (src) {
      if (typeof src.full === 'string') return src.full
      if (typeof src.text === 'string') return src.text
      return JSON.stringify(src, null, 2)
    }
    return full
  })()

  return (
    <>
      <div className={`trace-row ${cls}`}>
        {(full !== null || elided) && (
          <button className="expand" onClick={() => setOpen(!open)}>
            {open ? '▾' : '▸'}
          </button>
        )}
        {line}
      </div>
      {open && fullText !== null && (
        <Payload text={pretty(fullText)} loading={elided && fullEv.isLoading} />
      )}
    </>
  )
}

function TurnCard({ g, active, stem }: { g: Turn; active: boolean; stem: string }) {
  // The header names the batch: its goals and their targets. The label is the
  // batch id; the start event carries the claimed task and target.
  const task = String(g.start?.task ?? '') || g.label || 'build'
  const target = String(g.start?.target ?? '')
  const goals = Array.isArray(g.start?.goals) ? (g.start.goals as string[]) : []
  const sections = Array.isArray(g.start?.sections) ? (g.start.sections as string[]) : []
  // Where the session got to, from its own section events: the same path the files
  // tree and the editor draw (docs/compiler/sessions.md#trace-events).
  const path = g.rounds
    .flatMap((r) => r.rows)
    .filter((row) => row.event.kind === 'section')
    .map((row) => String(row.event.section ?? ''))
  const reached = new Set(path)
  const at = path[path.length - 1]
  return (
    <div className={`card ${active ? 'turn-active' : ''}`}>
      <div className="row">
        <b>{g.label}</b>
        <span className="mono">{task}{target ? ` ${target}` : ''}</span>
        {goals.length > 1 && <span className="muted mono">{goals.length} goals</span>}
        {active && <span className="chip v-stale">running</span>}
        {g.done && (
          <span className="chip v-ok">
            staged {String(g.done.staged ?? 0)} · {String(g.done.rounds ?? 0)} rounds
          </span>
        )}
        {g.failed && <span className="chip v-bad">failed</span>}
        {!active && !g.done && !g.failed && <span className="chip sev-none">unfinished</span>}
        {sections.length > 0 && (
          <span className="muted mono" title={sections.join('\n')}>
            {reached.size}/{sections.length} sections
          </span>
        )}
        {active && at && <span className="mono">at {at}</span>}
        <span className="muted">{g.rounds.length} rounds</span>
      </div>
      {g.start && (
        <div className="trace-row t-muted">
          {goals.length > 0 ? `${goals.join(' · ')} · ` : ''}
          dirty {String(g.start.dirty ?? 0)} · stale {String(g.start.stale ?? 0)}
          {sections.length > 0 ? ` · ${sections.join(' ')}` : ''}
        </div>
      )}
      {g.preRows.map((row) => (
        <TraceRow key={row.n} ev={row.event} stem={stem} n={row.n} />
      ))}
      {g.rounds.map((r, i) => (
        <RoundCard key={r.request?.n ?? r.response?.n ?? i} stem={stem} r={r} index={i} />
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
              <Link to={`/journal/${e.generation}`}>g{e.generation}</Link> · {e.kind || 'changeset'} ·{' '}
              {entryLabel(e)} · <span className="v-ok">+{a}</span>{' '}
              <span className="sev-info">~{u}</span> <span className="v-bad">-{d}</span> ·{' '}
              {e.tokens} tok
            </summary>
            {(e.resolved_goals ?? []).map((r) => (
              <p key={r.goal} style={{ margin: '1px 0 1px 16px' }}>
                <span className="v-ok">✓</span> <NodeLink id={r.goal} />{' '}
                <span className="muted">{r.justification}</span>
              </p>
            ))}
            {(e.opened_goals ?? []).map((o) => (
              <p key={o.goal} style={{ margin: '1px 0 1px 16px' }}>
                <span className="sev-info">◈</span> <NodeLink id={o.goal} />{' '}
                <span className="muted mono">
                  g{o.cause.generation} #{o.cause.mutation}
                  {o.cause.via ? ` via ${o.cause.via}` : ''}
                </span>
              </p>
            ))}
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

// The workers strip: who is attached, what they hold, and the release buttons when
// gated work exists. Mirrors docs/frontends/gui.md#workers.
function WorkersStrip() {
  const { data: w } = useWorkers()
  if (!w) return null
  const agents = w.workers.filter((x) => x.kind === 'agent')
  const gatedC = w.gated?.compile ?? 0
  const gatedG = w.gated?.generate ?? 0
  const unclaimed = w.unclaimed ?? 0
  const held = w.leases.filter((l) => l.task !== 'build')
  if (agents.length === 0 && gatedC === 0 && gatedG === 0 && held.length === 0 && unclaimed === 0)
    return null
  return (
    <span className="muted" style={{ display: 'inline-flex', gap: 8, alignItems: 'center' }}>
      {agents.map((a) => (
        <span key={a.id} title={`agent worker ${a.id} (pid ${a.pid})`}>
          ⚡ {a.client || 'agent'}
          {a.task ? ` · ${a.task}` : ' · idle'}
        </span>
      ))}
      {gatedC > 0 && (
        <button
          title="approve the pending document changes; the attached worker compiles them"
          onClick={() => post('/api/release', { stage: 'compile' })}
        >
          release compile {gatedC}
        </button>
      )}
      {gatedG > 0 && (
        <button
          title="approve the pending graph changes for binding and generation"
          onClick={() => post('/api/release', { stage: 'generate' })}
        >
          release gen {gatedG}
        </button>
      )}
      {unclaimed > 0 && (
        <button
          title={`${unclaimed} deliverable file(s) no binding names; draft docs describing what they do`}
          onClick={() => post('/api/jobs', { kind: 'decompile' })}
        >
          decompile {unclaimed} unclaimed
        </button>
      )}
    </span>
  )
}

// The control line: run actions and the automatic modes, visible even collapsed.
// In compile: manual the compile click opens the preview pane on the board; its
// release button records the release (docs/frontends/gui.md#workflow-modes).
function ControlBar({ open, setOpen }: { open: boolean; setOpen: (v: boolean) => void }) {
  const { data: s } = useStatus()
  const { data: docs } = useDocs()
  const pending = useGenPending()
  const navigate = useNavigate()
  const jobs = useApp((a) => a.jobs)
  const watchMode = useApp((a) => a.watchMode)
  const genMode = useApp((a) => a.genMode)
  const running = Object.values(jobs).find((j) => j.state === 'running')
  const queued = Object.values(jobs).filter((j) => j.state === 'queued').length
  const changedDocs = (docs?.docs ?? []).filter((d) => d.stale).length
  const genPending = pending.data?.pending.length ?? 0
  const boardCounts = s?.board
  const compileClick = () => {
    if (watchMode !== 'watch') {
      // Manual: the click opens the preview pane before any release.
      navigate('/board?preview=next')
      return
    }
    void post('/api/jobs', { kind: 'compile' })
  }

  return (
    <div className="wb-activity-bar">
      <button className="toggle" onClick={() => setOpen(!open)} title="run history">
        {open ? '▾' : '▸'} activity
      </button>
      <WorkersStrip />
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
        <span className="muted">
          {verdictText(s?.verdict)}
          {boardCounts && boardCounts.open > 0 ? ` · ${boardCounts.open} open goals` : ''}
          {boardCounts && boardCounts.blocked > 0 ? `, ${boardCounts.blocked} blocked` : ''}
        </span>
      )}
      <span className="bar-right">
        <label className="muted" title="manual: changes queue and await a release, compiling is a click · auto: compile on change (spends LLM budget). Shared with agents via control.yaml.">
          compile
          <select value={watchMode === 'watch' ? 'watch' : 'queue'} onChange={(e) => put('/api/watch', { mode: e.target.value })}>
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
        <button disabled={!!running} onClick={compileClick} title={watchMode !== 'watch' ? 'opens the preview pane; its release button runs the build' : 'compile now'}>
          compile{changedDocs > 0 ? ` ${changedDocs}` : ''}
          {boardCounts && boardCounts.gated > 0 ? ` (${boardCounts.gated} gated)` : ''} ▸
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
    () =>
      selLive
        ? liveTrace.filter((r) => r.jobId === selId).map((r) => ({ n: r.seq, event: r.event }))
        : [],
    [liveTrace, selLive, selId],
  )
  const events = useMemo(
    () => mergeLive(transcript.data?.events ?? [], liveEvents),
    [transcript.data, liveEvents],
  )
  const turns = useMemo(() => groupTurns(events), [events])
  // Turns run in parallel: every group still without an outcome is live, not just
  // the last one on the wire.
  const activeTurns = useMemo(
    () => new Set(isRunning ? turns.filter((g) => g.start && !g.done && !g.failed) : []),
    [turns, isRunning],
  )

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
                {[...turns].reverse().map((g) => (
                  <TurnCard key={g.key} g={g} active={activeTurns.has(g)} stem={stem} />
                ))}
              </>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
