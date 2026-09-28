// The chat pane: the persistent conversation surface on the far right. Chat sessions
// talk to the configured ACP agent (in a tab, the loop in the worker); follow entries render the running build from the
// live trace the jobs already stream. Mirrors docs/frontends/gui.md#chat.
import { useEffect, useMemo, useRef, useState } from 'react'
import { useLocation, useNavigate, useSearchParams } from 'react-router'
import { get, post, verdictText } from '../lib/api'
import { useApp, type ChatUpdateRow, type TurnProgress } from '../lib/store'
import { needsPageModel, useCapabilities, type Capabilities } from '../lib/capabilities'
import ModelSourcePanel from '../components/ModelSourcePanel'
import { toolWords, workingLine } from '../lib/working'
import { useWords, verdictWords } from '../lib/words'

interface Command {
  name: string
  description: string
}

// ---- transcript row model: both session kinds normalize onto this ----

interface Row {
  key: string
  kind: 'user' | 'agent' | 'thought' | 'tool' | 'marker'
  text: string
  // Tool rows: pending | ok | failed.
  status?: string
  detail?: string
}

// Chat sessions: coalesce streamed chunks, one row per tool call.
// A tool payload one field per line (docs/compiler/tools.md): a string that already
// holds pretty JSON stays as it is, anything else is pretty-printed.
function pretty(v: unknown): string {
  if (typeof v === 'string') return v
  try {
    return JSON.stringify(v, null, 2)
  } catch {
    return String(v)
  }
}

function chatRows(updates: ChatUpdateRow[]): Row[] {
  const rows: Row[] = []
  const toolRow: Record<string, number> = {}
  const push = (kind: Row['kind'], text: string, key: string) => rows.push({ key, kind, text })
  const appendText = (kind: 'agent' | 'thought', text: string, key: string) => {
    const last = rows[rows.length - 1]
    if (last && last.kind === kind) last.text += text
    else push(kind, text, key)
  }
  for (const { n, update } of updates) {
    const u = update as Record<string, never> & Record<string, unknown>
    const k = String(u.sessionUpdate ?? '')
    const content = (u.content ?? {}) as { type?: string; text?: string }
    switch (k) {
      case 'user_message':
        push('user', String(u.text ?? ''), `u${n}`)
        break
      case 'agent_message':
        push('agent', String(u.text ?? ''), `a${n}`)
        break
      case 'agent_message_chunk':
        appendText('agent', content.text ?? '', `a${n}`)
        break
      case 'agent_thought_chunk':
        appendText('thought', content.text ?? '', `t${n}`)
        break
      case 'tool_call': {
        const id = String(u.toolCallId ?? n)
        toolRow[id] = rows.length
        rows.push({
          key: `c${n}`,
          kind: 'tool',
          text: String(u.title ?? 'tool'),
          status: 'pending',
          detail: u.rawInput !== undefined ? pretty(u.rawInput) : undefined,
        })
        break
      }
      case 'tool_call_update': {
        const id = String(u.toolCallId ?? '')
        const fields = (u.fields ?? u) as { status?: string; rawOutput?: unknown; title?: string }
        const at = toolRow[id]
        if (at !== undefined && rows[at]) {
          const status = fields.status === 'failed' ? 'failed' : fields.status === 'completed' ? 'ok' : 'pending'
          rows[at] = {
            ...rows[at],
            status,
            detail: fields.rawOutput !== undefined ? pretty(fields.rawOutput) : rows[at].detail,
          }
        }
        break
      }
      case 'plan': {
        const entries = (u.entries as { content?: string; status?: string }[]) ?? []
        push('marker', entries.map((e) => `[${e.status}] ${e.content}`).join('\n'), `p${n}`)
        break
      }
      case 'turn_end':
        push('marker', u.error ? `turn ended: ${u.error}` : `· ${String(u.stop ?? 'end')} ·`, `e${n}`)
        break
      default:
        break
    }
  }
  return rows
}

// A batch in plain words: what the session does and to what
// (docs/frontends/gui.md#chat). The Developer toggle keeps the goal ids beside it.
function batchWords(goals: { kind?: string; target?: string }[], dev: boolean): string {
  if (goals.length === 0) return 'working'
  const first = goals[0]
  const kind = first.kind ?? ''
  const target = first.target ?? ''
  const doc = target.includes('#') ? target.slice(0, target.indexOf('#')) : target
  const name = (t: string) => t.replace(/^(ent|req|view):/, '')
  let text: string
  switch (kind) {
    case 'reconcile-section':
      text = `Reading ${doc}${goals.length > 1 ? `, ${goals.length} sections` : ''}`
      break
    case 'place-anchors':
      text = `Re-anchoring ${doc}`
      break
    case 'rejudge-pair':
      text = `Comparing ${goals.length} pair${goals.length === 1 ? '' : 's'} of requirements`
      break
    case 'review-entity':
      text = `Reviewing ${name(target)}${goals.length > 1 ? ` and ${goals.length - 1} more` : ''}`
      break
    case 'retrace':
      text = `Re-tracing ${name(target)}`
      break
    case 'conform-instance':
      text = `Checking ${name(target)} against its type`
      break
    case 'bind':
      text = `Binding ${goals.length} requirement${goals.length === 1 ? '' : 's'} to the product`
      break
    case 'generate':
      text = `Building ${name(target)}${goals.length > 1 ? ` and ${goals.length - 1} more` : ''}`
      break
    case 'verify':
      text = `Checking ${goals.length} requirement${goals.length === 1 ? '' : 's'}`
      break
    case 'declare-edges':
    case 'dedupe-candidates':
    case 'curate-view':
    case 'split-view':
    case 'abstract-entity':
      text = `Tidying the model: ${kind.replace('-', ' ')} on ${name(target)}`
      break
    default:
      text = `${kind} ${target}`
  }
  return dev ? `${text} (${goals.map((g) => `${g.kind} ${g.target}`).join(' · ')})` : text
}

// Follow entries: the running build's trace rows, filtered by job.
function followRows(trace: { jobId: number; seq: number; event: Record<string, unknown> }[], jobId: number, dev: boolean): Row[] {
  const rows: Row[] = []
  for (const { jobId: j, seq, event } of trace) {
    if (j !== jobId) continue
    const kind = String(event.kind ?? '')
    const label = String(event.label ?? '')
    switch (kind) {
      case 'batchStart': {
        const goals = Array.isArray(event.goals)
          ? (event.goals as { kind?: string; target?: string }[])
          : []
        rows.push({
          key: `b${seq}`,
          kind: 'marker',
          text: `▷ ${batchWords(goals, dev)}${dev ? ` [${label}]` : ''}`,
        })
        break
      }
      case 'sessionStart':
        if (dev) rows.push({ key: `s${seq}`, kind: 'marker', text: `▶ ${label}` })
        break
      case 'goal': {
        const tail = String(event.justification ?? event.reason ?? '')
        rows.push({
          key: `g${seq}`,
          kind: 'marker',
          text: `${event.event === 'resolved' ? '✓' : '◈'} ${String(event.event ?? '')} ${String(event.goal ?? '')}${tail ? `: ${tail}` : ''}`,
        })
        break
      }
      case 'modelText':
        if (dev) rows.push({ key: `m${seq}`, kind: 'thought', text: String(event.text ?? '') })
        break
      case 'toolCall':
        rows.push({
          key: `c${seq}`,
          kind: 'tool',
          text: `${label} → ${String(event.name ?? '')}`,
          status: 'pending',
          detail: String(event.summary ?? ''),
        })
        break
      case 'toolResult': {
        // The result closes the last pending tool row of the same label.
        for (let i = rows.length - 1; i >= 0; i--) {
          if (rows[i].kind === 'tool' && rows[i].status === 'pending' && rows[i].text.startsWith(label)) {
            rows[i] = { ...rows[i], status: 'ok', detail: String(event.summary ?? '') }
            break
          }
        }
        break
      }
      case 'toolError':
        rows.push({
          key: `x${seq}`,
          kind: 'tool',
          text: `${label} ✗ ${String(event.rule ?? '')}`,
          status: 'failed',
          detail: String(event.message ?? ''),
        })
        break
      case 'genEntityStart':
        rows.push({ key: `ge${seq}`, kind: 'marker', text: `▷ Building ${String(event.entity ?? '').replace(/^ent:/, '')}${event.stage ? ` (${String(event.stage)})` : ''}` })
        break
      case 'genEntityDone':
        rows.push({
          key: `gd${seq}`,
          kind: 'marker',
          text: `✓ Built ${String(event.entity ?? '').replace(/^ent:/, '')}${Array.isArray(event.files) ? `: ${(event.files as string[]).join(', ')}` : ''}`,
        })
        break
      case 'genEntitySkipped':
        rows.push({ key: `gs${seq}`, kind: 'marker', text: `· Skipped ${String(event.entity ?? '').replace(/^ent:/, '')}${event.reason ? `: ${String(event.reason)}` : ''}` })
        break
      case 'genEntityFailed':
        rows.push({ key: `gf${seq}`, kind: 'marker', text: `✗ ${String(event.entity ?? '').replace(/^ent:/, '')}: ${String(event.error ?? event.reason ?? 'failed')}` })
        break
      case 'verifyRowStart':
        rows.push({ key: `vs${seq}`, kind: 'tool', text: `Checking ${String(event.requirement ?? '')}`, status: 'pending', detail: String(event.test ?? '') })
        break
      case 'verifyRowDone': {
        const rid = String(event.requirement ?? '')
        const pass = String(event.verdict ?? '') === 'pass'
        let closed = false
        for (let i = rows.length - 1; i >= 0; i--) {
          if (rows[i].kind === 'tool' && rows[i].status === 'pending' && rows[i].text === `Checking ${rid}`) {
            rows[i] = { ...rows[i], text: `${pass ? 'Verified' : 'Failing'} ${rid}`, status: pass ? 'ok' : 'failed', detail: String(event.evidence ?? event.run ?? '') }
            closed = true
            break
          }
        }
        if (!closed) rows.push({ key: `vd${seq}`, kind: 'tool', text: `${pass ? 'Verified' : 'Failing'} ${rid}`, status: pass ? 'ok' : 'failed', detail: String(event.evidence ?? '') })
        break
      }
      case 'verifyRowStale':
        rows.push({ key: `vt${seq}`, kind: 'tool', text: `Stale ${String(event.requirement ?? '')}`, status: 'pending', detail: `${String(event.status ?? '')}: ${String(event.reason ?? '')}` })
        break
      case 'verifyRowError':
        rows.push({ key: `ve${seq}`, kind: 'tool', text: `Could not check ${String(event.requirement ?? '')}`, status: 'failed', detail: String(event.error ?? event.reason ?? '') })
        break
      case 'sessionDone':
        rows.push({ key: `d${seq}`, kind: 'marker', text: dev ? `✓ ${label}` : `✓ done${event.summary ? `: ${String(event.summary)}` : ''}` })
        break
      case 'sessionFailed':
        rows.push({ key: `f${seq}`, kind: 'marker', text: `✗ ${dev ? `${label}: ` : ''}${String(event.error ?? 'failed')}` })
        break
      default:
        break
    }
  }
  return rows.slice(-500)
}

// A new conversation. A tab passes its model's tier: a reader-tier model gets the
// read tools alone and a short instruction block (docs/frontends/wasm.md#chat).
function openChat(caps: Capabilities | undefined) {
  return post<{ id: string }>(`/api/chat/sessions`, caps?.modelTier === 'reader' ? { tier: 'reader' } : undefined)
}

export default function ChatPane() {
  const open = useApp((a) => a.chatOpen)
  const setOpen = useApp((a) => a.setChatOpen)
  if (!open)
    return (
      <div className="wb-chat wb-chat-closed">
        <button className="chat-toggle" title="chat" onClick={() => setOpen(true)}>
          ✦
        </button>
      </div>
    )
  return <OpenPane close={() => setOpen(false)} />
}

// ---- standing questions: prompted diagnostics awaiting a human answer ----
// Mirrors docs/frontends/gui.md#questions.

interface Question {
  id: string
  rule: string
  severity: string
  message: string
  prompt: {
    question: string
    options?: { label: string; edit?: unknown; answer?: string }[]
    freeform?: boolean
  }
  answer?: { status: string; text: string } | null
}

function Questions() {
  const lastCommit = useApp((a) => a.lastCommit)
  const [qs, setQs] = useState<Question[]>([])
  const [text, setText] = useState<Record<string, string>>({})
  const [busy, setBusy] = useState<string | null>(null)
  const refresh = async () => {
    try {
      const r = await get<{ questions: Question[] }>(`/api/questions`)
      setQs(r.questions)
    } catch {
      /* the panel is best-effort; the next commit refreshes it */
    }
  }
  useEffect(() => {
    void refresh()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lastCommit?.generation])
  const respond = async (id: string, body: Record<string, unknown>) => {
    setBusy(id)
    try {
      await post(`/api/questions/${encodeURIComponent(id)}/answer`, body)
    } finally {
      setBusy(null)
      void refresh()
    }
  }
  const open = qs.filter((q) => !q.answer || q.answer.status === 'failed')
  const handling = qs.filter((q) => q.answer?.status === 'handling')
  if (open.length === 0 && handling.length === 0) return null
  return (
    <div className="chat-questions">
      <div className="chat-q-head">questions ({open.length})</div>
      {open.map((q) => (
        <div key={q.id} className="chat-q">
          <div className="chat-q-msg">
            {q.rule}: {q.message}
          </div>
          <div className="chat-q-q">{q.prompt.question}</div>
          <div className="chat-q-opts">
            {(q.prompt.options ?? []).map((o, i) => (
              <button
                key={i}
                disabled={busy === q.id}
                className={o.edit ? 'edit' : ''}
                title={o.edit ? 'suggested edit: applies to the document and resolves' : 'handled by the agent'}
                onClick={() => void respond(q.id, { option: i })}
              >
                {o.edit ? '✎ ' : ''}
                {o.label}
              </button>
            ))}
          </div>
          {q.prompt.freeform && (
            <input
              className="chat-q-free"
              disabled={busy === q.id}
              value={text[q.id] ?? ''}
              placeholder="answer in your own words, enter to send"
              onChange={(e) => setText({ ...text, [q.id]: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && (text[q.id] ?? '').trim()) void respond(q.id, { text: text[q.id].trim() })
              }}
            />
          )}
        </div>
      ))}
      {handling.map((q) => (
        <div key={q.id} className="chat-q handling">
          <span className="dot running" /> handling: {q.prompt.question}
        </div>
      ))}
    </div>
  )
}

function OpenPane({ close }: { close: () => void }) {
  const sessions = useApp((a) => a.chatSessions)
  const jobs = useApp((a) => a.jobs)
  const selected = useApp((a) => a.chatSelected)
  const select = useApp((a) => a.selectChat)
  const caps = useCapabilities()
  const w = useWords()
  const noAgent = caps.data?.agent === 'none'
  // A tab's agent runs the page's model; until it is ready the pane offers the
  // chooser instead of the prompt box (docs/frontends/gui.md#chat).
  const needsModel = !noAgent && needsPageModel(caps.data)

  // Build threads: one per job this page has seen, live ones first
  // (docs/frontends/gui.md#chat).
  const followIds = useMemo(() => {
    const live = (j: { state: string }) => j.state === 'running' || j.state === 'queued'
    return Object.values(jobs)
      .sort((a, b) => Number(live(b)) - Number(live(a)) || b.id - a.id)
      .slice(0, 8)
      .map((j) => `follow-${j.id}`)
  }, [jobs])

  const chatIds = Object.keys(sessions)
  const active =
    selected && (chatIds.includes(selected) || followIds.includes(selected))
      ? selected
      : followIds[0] ?? chatIds[chatIds.length - 1] ?? null
  const chatId = active && !active.startsWith('follow-') ? active : null
  const chatRunning = chatId ? sessions[chatId]?.state === 'running' : false

  const newChat = async () => {
    const r = await openChat(caps.data)
    select(r.id)
  }

  return (
    <div className="wb-chat">
      <div className="chat-head">
        <span className="chat-title">Chat</span>
        {!noAgent && (
          <button className="chat-new" onClick={() => void newChat()} title="new conversation with the agent">
            +
          </button>
        )}
        <button className="chat-toggle" onClick={close} title="collapse">
          ›
        </button>
      </div>
      <div className="chat-sessions">
        {followIds.map((id) => {
          const j = jobs[Number(id.slice(7))]
          const live = j && (j.state === 'running' || j.state === 'queued')
          const verb = j?.kind.kind === 'compile' ? w.sync : j?.kind.kind === 'gen' ? w.build : j?.kind.kind === 'verify' || j?.kind.kind === 'audit' ? w.review : (j?.kind.kind ?? 'build')
          return (
            <button key={id} className={`chat-sess${active === id ? ' active' : ''}${live ? ' working' : ''}`} onClick={() => select(id)} title={`build thread for job ${id.slice(7)}`}>
              <span className={`dot ${live ? 'running' : j?.state === 'failed' ? 'failed' : ''}`} /> {verb} #{id.slice(7)}
            </button>
          )
        })}
        {chatIds.map((id) => (
          <button key={id} className={`chat-sess${active === id ? ' active' : ''}${sessions[id].state === 'running' ? ' working' : ''}`} onClick={() => select(id)}>
            <span className={`dot ${sessions[id].state}`} /> {sessions[id].title}
            {sessions[id].pending.length > 0 && <span className="ask-badge">?</span>}
          </button>
        ))}
      </div>
      {active?.startsWith('follow-') ? (
        <FollowView jobId={Number(active.slice(7))} />
      ) : chatId ? (
        <ChatView id={chatId} />
      ) : (
        <div className="chat-empty muted">
          {noAgent ? (
            <p>This backend has no agent to talk to. Syncs still show here as build threads while they run.</p>
          ) : (
            <p>Ask about the project, or run a sync and follow it here.</p>
          )}
        </div>
      )}
      {/* Ready to type: the prompt box is at the foot whenever the column is open,
          conversation or none (docs/frontends/gui.md#chat). */}
      {noAgent ? null : needsModel && !chatRunning ? <NeedsModel /> : <PromptBox sessionId={chatId} />}
    </div>
  )
}

// The model chooser in place of the prompt box, while a tab has no model to call.
// Mirrors docs/frontends/wasm.md#chat.
function NeedsModel() {
  return (
    <div className="chat-model">
      <p className="chat-model-head">Chat needs a model in this browser.</p>
      <ModelSourcePanel />
    </div>
  )
}

// The loaded-set panel: the status block as the serving renders it, re-rendered
// live on every mutating tool reply and in full on graph_status, the same cadence
// the model sees. Read-only. Mirrors docs/frontends/gui.md#chat.
function extractLoaded(texts: string[]): string | null {
  for (let i = texts.length - 1; i >= 0; i--) {
    const t = texts[i]
    const at = t.lastIndexOf('## Loaded')
    if (at >= 0) return t.slice(at)
  }
  return null
}

function LoadedPanel({ block }: { block: string | null }) {
  const [open, setOpen] = useState(true)
  const dev = useApp((a) => a.dev)
  if (!block || !dev) return null
  return (
    <div className="chat-plan" style={{ borderTop: '1px solid var(--line)' }}>
      <button className="chat-toggle" onClick={() => setOpen(!open)}>
        {open ? '▾' : '▸'} loaded set
      </button>
      {open && <pre style={{ margin: 0, maxHeight: 180, overflow: 'auto', fontSize: 11 }}>{block}</pre>}
    </div>
  )
}

// `working` is the pulsing line at the end while a turn runs: what the agent is
// doing (docs/frontends/gui.md#chat).
function Transcript({ rows, follow, working }: { rows: Row[]; follow: boolean; working?: string | null }) {
  const endRef = useRef<HTMLDivElement>(null)
  const dev = useApp((a) => a.dev)
  useEffect(() => {
    if (follow) endRef.current?.scrollIntoView({ block: 'end' })
  }, [rows.length, follow, working])
  return (
    <div className="chat-transcript">
      {rows.map((r) => (
        <div key={r.key} className={`chat-row ${r.kind} ${r.status ?? ''}`}>
          {r.kind === 'tool' ? (
            <details open={dev || undefined}>
              <summary>
                <span className={`tool-dot ${r.status}`} /> {r.text}
              </summary>
              {r.detail && <pre>{r.detail}</pre>}
            </details>
          ) : (
            <pre>{r.text}</pre>
          )}
        </div>
      ))}
      {working && (
        <div className="chat-row working" role="status">
          <pre>{working}</pre>
        </div>
      )}
      <div ref={endRef} />
    </div>
  )
}

// The current selection as context for a question: the inspected node, or the
// open document (docs/frontends/gui.md#chat).
function useSelectionContext(): string | null {
  const loc = useLocation()
  const [sp] = useSearchParams()
  const node = sp.get('node')
  if (node) return node
  const prefixes = ['/docs/', '/model/entities/', '/model/requirements/', '/product/files/']
  for (const pre of prefixes) if (loc.pathname.startsWith(pre)) return decodeURIComponent(loc.pathname.slice(pre.length))
  return null
}

function ChatView({ id }: { id: string }) {
  const updates = useApp((a) => a.chatUpdates[id])
  const dev = useApp((a) => a.dev)
  const seedChatUpdates = useApp((a) => a.seedChatUpdates)
  const info = useApp((a) => a.chatSessions[id])

  // Replay the session on first open (a reload arrives mid-conversation).
  useEffect(() => {
    if (updates !== undefined) return
    get<{ updates: { n: number; update: Record<string, unknown> }[] }>(`/api/chat/sessions/${id}`)
      .then((r) => seedChatUpdates(id, r.updates ?? []))
      .catch(() => seedChatUpdates(id, []))
  }, [id, updates, seedChatUpdates])

  const rows = useMemo(() => {
    const all = chatRows(updates ?? [])
    return dev ? all : all.filter((r) => r.kind !== 'thought')
  }, [updates, dev])
  const running = info?.state === 'running'
  // The loaded set as the agent's tool replies rendered it, newest wins.
  const loadedBlock = useMemo(
    () =>
      extractLoaded(
        (updates ?? []).map(({ update }) => {
          const u = update as { fields?: { rawOutput?: unknown }; rawOutput?: unknown }
          const raw = u.fields?.rawOutput ?? u.rawOutput
          return typeof raw === 'string' ? raw : ''
        }),
      ),
    [updates],
  )

  return (
    <>
      <Transcript rows={rows} follow working={running ? workingLine(rows) : null} />
      <LoadedPanel block={loadedBlock} />
      {(info?.pending ?? []).map((ask) => (
        <PermissionAsk key={ask.id} sessionId={id} ask={ask as never} />
      ))}
    </>
  )
}

// The prompt box at the foot of the column. Sending with no conversation selected
// creates one and selects it, so the first question needs no button first; while
// a turn runs Send reads Stop (docs/frontends/gui.md#chat).
function PromptBox({ sessionId }: { sessionId: string | null }) {
  const caps = useCapabilities()
  const context = useSelectionContext()
  const select = useApp((a) => a.selectChat)
  const info = useApp((a) => (sessionId ? a.chatSessions[sessionId] : undefined))
  const [text, setText] = useState('')
  const [commands, setCommands] = useState<Command[]>([])
  const [sending, setSending] = useState(false)
  useEffect(() => {
    get<{ commands?: Command[] }>(`/api/chat/sessions`)
      .then((r) => setCommands(r.commands ?? []))
      .catch(() => {})
  }, [])
  const running = info?.state === 'running'

  const send = async () => {
    const t = text.trim()
    if (!t || running || sending) return
    setSending(true)
    setText('')
    try {
      let id = sessionId
      if (!id) {
        const r = await openChat(caps.data)
        id = r.id
        select(id)
      }
      await post(`/api/chat/sessions/${id}/prompt`, { text: t })
    } catch {
      // The transcript carries the turn's end, error included.
    } finally {
      setSending(false)
    }
  }

  const completions = text.startsWith('/') ? commands.filter((c) => c.name.startsWith(text.trim())) : []

  return (
    <div className="chat-input">
      {completions.length > 0 && (
        <div className="chat-completions">
          {completions.map((c) => (
            <button key={c.name} onClick={() => setText(c.name + ' ')}>
              <span className="mono">{c.name}</span> <span className="muted">{c.description}</span>
            </button>
          ))}
        </div>
      )}
      {context && !text && (
        <button className="chat-context" onClick={() => setText(`About ${context}: `)} title="start the question with what you are looking at">
          ask about {context.length > 40 ? `${context.slice(0, 38)}…` : context}
        </button>
      )}
      <textarea
        value={text}
        placeholder={running ? 'the agent is working…' : commands.length > 0 ? 'ask the agent, or / for commands' : 'ask the agent about the project'}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault()
            void send()
          }
        }}
      />
      <div className="chat-input-row">
        {running && sessionId ? (
          <button onClick={() => void post(`/api/chat/sessions/${sessionId}/cancel`)} title="cancel the running turn">
            Stop
          </button>
        ) : (
          <button onClick={() => void send()} disabled={!text.trim() || sending}>
            Send
          </button>
        )}
      </div>
    </div>
  )
}

interface Ask {
  id: string
  request?: {
    toolCall?: { title?: string }
    options?: { optionId: string; name: string; kind: string }[]
  }
}

function PermissionAsk({ sessionId, ask }: { sessionId: string; ask: Ask }) {
  const answer = (optionId: string | null) =>
    void post(`/api/chat/permissions`, { sessionId, id: ask.id, optionId: optionId ?? undefined })
  return (
    <div className="chat-ask">
      <div>the agent asks permission: {ask.request?.toolCall?.title ?? 'a tool call'}</div>
      <div className="chat-ask-buttons">
        {(ask.request?.options ?? []).map((o) => (
          <button key={o.optionId} onClick={() => answer(o.optionId)}>
            {o.name}
          </button>
        ))}
        <button onClick={() => answer(null)}>dismiss</button>
      </div>
    </div>
  )
}

// A running build, rendered from the same trace the activity panel shows: the plan
// checklist on top, the stream below. Follow mode moves the editor along with the
// work. Mirrors docs/frontends/gui.md#chat.
function FollowView({ jobId }: { jobId: number }) {
  const trace = useApp((a) => a.trace)
  const turns = useApp((a) => a.turns)
  const followMode = useApp((a) => a.chatFollow)
  const setFollowMode = useApp((a) => a.setChatFollow)
  const navigate = useNavigate()
  const dev = useApp((a) => a.dev)
  const job = useApp((a) => a.jobs[jobId])

  const live = !!job && (job.state === 'running' || job.state === 'queued')
  const rows = useMemo(() => {
    const out = followRows(trace as never, jobId, dev)
    if (job && !live) {
      const r = job.result as { verdict?: { state?: string } | string } | null
      const v = r?.verdict
      const counted = r
        ? Object.entries(r)
            .filter(([k, x]) => k !== 'verdict' && typeof x === 'number' && x !== 0)
            .map(([k, x]) => `${x} ${k.replace(/([A-Z])/g, ' $1').toLowerCase()}`)
            .join(', ')
        : ''
      const line =
        typeof v === 'string'
          ? dev
            ? v
            : v.replace(/^converged.*/, 'Up to date').replace(/^incomplete.*/, 'Sync incomplete')
          : v && typeof v === 'object'
            ? dev
              ? verdictText(v as never)
              : verdictWords(v as never, false)
            : counted || job.state
      out.push({ key: 'end', kind: 'marker', text: job.state === 'failed' ? `✗ ${line}` : `■ ${line}` })
    }
    return out
  }, [trace, jobId, dev, job])
  const plan: TurnProgress[] = useMemo(
    () => Object.values(turns).sort((a, b) => a.since - b.since),
    [turns],
  )
  // The running batch, the same way a chat turn shows its work: the tool being
  // called, else what the batch is doing (docs/frontends/gui.md#chat).
  const working = useMemo(() => {
    if (!live) return null
    const last = [...trace].reverse().find((r) => r.jobId === jobId)
    if (last && last.event.kind === 'toolCall') return toolWords(String(last.event.name ?? ''))
    const running = plan.find((t) => t.state === 'running')
    return running ? `${batchWords([{ kind: running.task, target: running.target }], dev)}…` : 'Thinking…'
  }, [live, trace, jobId, plan, dev])
  // The loaded-set panel beside the transcript: the last status block a tool
  // reply carried (condensed on mutating replies, full on graph_status).
  const loadedBlock = useMemo(
    () =>
      extractLoaded(
        trace
          .filter((r) => r.jobId === jobId && r.event.kind === 'toolResult')
          .map((r) => `${String(r.event.full ?? '')}\n${String(r.event.summary ?? '')}`),
      ),
    [trace, jobId],
  )

  // Follow mode: the editor shows what the agent is touching. Section events name
  // the place; the doc route reveals it.
  const lastActive = useRef<string | null>(null)
  useEffect(() => {
    if (!followMode) return
    const running = plan.find((t) => t.state === 'running' && t.doc)
    if (!running) return
    const key = `${running.doc}#${running.active ?? ''}`
    if (key === lastActive.current) return
    lastActive.current = key
    const section = running.active ? `?section=${encodeURIComponent(running.active)}` : ''
    navigate(`/docs/${running.doc}${section}`)
  }, [plan, followMode, navigate])

  return (
    <>
      <div className="chat-plan">
        <label className="chat-follow-toggle">
          <input type="checkbox" checked={followMode} onChange={(e) => setFollowMode(e.target.checked)} />
          follow in editor
        </label>
        {plan.map((t) => (
          <div key={t.label} className={`plan-entry ${t.state}${t.state === 'running' ? ' working' : ''}`}>
            <span className="plan-mark">
              {t.state === 'done' ? '✓' : t.state === 'failed' ? '✗' : t.state === 'running' ? '●' : '○'}
            </span>{' '}
            {batchWords([{ kind: t.task, target: t.target }], dev)}
            {t.sections.length > 0 && (
              <span className="muted">
                {' '}
                {t.touched.length}/{t.sections.length}
              </span>
            )}
            {dev && <span className="muted"> {t.label}</span>}
          </div>
        ))}
      </div>
      <Transcript rows={rows} follow working={working} />
      <LoadedPanel block={loadedBlock} />
    </>
  )
}
