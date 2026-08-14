// The chat pane: the persistent conversation surface on the far right. Chat sessions
// talk to the configured ACP agent; follow entries render the running build from the
// live trace the jobs already stream. Mirrors docs/frontends/gui.md#chat.
import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { get, post } from '../lib/api'
import { useApp, type ChatUpdateRow, type TurnProgress } from '../lib/store'

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
          detail: u.rawInput !== undefined ? JSON.stringify(u.rawInput) : undefined,
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
            detail: fields.rawOutput !== undefined ? String(fields.rawOutput as string) : rows[at].detail,
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

// Follow entries: the running build's trace rows, filtered by job.
function followRows(trace: { jobId: number; seq: number; event: Record<string, unknown> }[], jobId: number): Row[] {
  const rows: Row[] = []
  for (const { jobId: j, seq, event } of trace) {
    if (j !== jobId) continue
    const kind = String(event.kind ?? '')
    const label = String(event.label ?? '')
    switch (kind) {
      case 'turnStart':
        rows.push({ key: `s${seq}`, kind: 'marker', text: `▶ ${label}` })
        break
      case 'modelText':
        rows.push({ key: `m${seq}`, kind: 'thought', text: String(event.text ?? '') })
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
      case 'turnDone':
        rows.push({ key: `d${seq}`, kind: 'marker', text: `✓ ${label}` })
        break
      case 'turnFailed':
        rows.push({ key: `f${seq}`, kind: 'marker', text: `✗ ${label}: ${String(event.error ?? '')}` })
        break
      default:
        break
    }
  }
  return rows.slice(-500)
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

  // Follow entries: one per live or recent job that has trace rows.
  const followIds = useMemo(() => {
    return Object.values(jobs)
      .filter((j) => j.state === 'running' || j.state === 'queued')
      .map((j) => `follow-${j.id}`)
  }, [jobs])

  const chatIds = Object.keys(sessions)
  const active =
    selected && (chatIds.includes(selected) || followIds.includes(selected))
      ? selected
      : followIds[0] ?? chatIds[chatIds.length - 1] ?? null

  const newChat = async () => {
    const r = await post<{ id: string }>(`/api/chat/sessions`)
    select(r.id)
  }

  return (
    <div className="wb-chat">
      <div className="chat-head">
        <span className="chat-title">chat</span>
        <button className="chat-new" onClick={() => void newChat()} title="new chat session">
          +
        </button>
        <button className="chat-toggle" onClick={close} title="collapse">
          ›
        </button>
      </div>
      <Questions />
      <div className="chat-sessions">
        {followIds.map((id) => (
          <button key={id} className={`chat-sess ${active === id ? 'active' : ''}`} onClick={() => select(id)}>
            <span className="dot running" /> build {id.slice(7)}
          </button>
        ))}
        {chatIds.map((id) => (
          <button key={id} className={`chat-sess ${active === id ? 'active' : ''}`} onClick={() => select(id)}>
            <span className={`dot ${sessions[id].state}`} /> {sessions[id].title}
            {sessions[id].pending.length > 0 && <span className="ask-badge">?</span>}
          </button>
        ))}
      </div>
      {active?.startsWith('follow-') ? (
        <FollowView jobId={Number(active.slice(7))} />
      ) : active ? (
        <ChatView id={active} />
      ) : (
        <div className="chat-empty muted">
          <p>no sessions. start one with +, or run a build and follow it here.</p>
        </div>
      )}
    </div>
  )
}

function Transcript({ rows, follow }: { rows: Row[]; follow: boolean }) {
  const endRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (follow) endRef.current?.scrollIntoView({ block: 'end' })
  }, [rows.length, follow])
  return (
    <div className="chat-transcript">
      {rows.map((r) => (
        <div key={r.key} className={`chat-row ${r.kind} ${r.status ?? ''}`}>
          {r.kind === 'tool' ? (
            <details>
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
      <div ref={endRef} />
    </div>
  )
}

function ChatView({ id }: { id: string }) {
  const updates = useApp((a) => a.chatUpdates[id])
  const seedChatUpdates = useApp((a) => a.seedChatUpdates)
  const info = useApp((a) => a.chatSessions[id])
  const [text, setText] = useState('')
  const [commands, setCommands] = useState<Command[]>([])

  // Replay the session on first open (a reload arrives mid-conversation).
  useEffect(() => {
    if (updates !== undefined) return
    get<{ updates: { n: number; update: Record<string, unknown> }[] }>(`/api/chat/sessions/${id}`)
      .then((r) => seedChatUpdates(id, r.updates ?? []))
      .catch(() => seedChatUpdates(id, []))
  }, [id, updates, seedChatUpdates])
  useEffect(() => {
    get<{ commands?: Command[] }>(`/api/chat/sessions`)
      .then((r) => setCommands(r.commands ?? []))
      .catch(() => {})
  }, [])

  const rows = useMemo(() => chatRows(updates ?? []), [updates])
  const running = info?.state === 'running'

  const send = async () => {
    const t = text.trim()
    if (!t || running) return
    setText('')
    await post(`/api/chat/sessions/${id}/prompt`, { text: t }).catch(() => {})
  }

  const completions = text.startsWith('/') ? commands.filter((c) => c.name.startsWith(text.trim())) : []

  return (
    <>
      <Transcript rows={rows} follow />
      {(info?.pending ?? []).map((ask) => (
        <PermissionAsk key={ask.id} sessionId={id} ask={ask as never} />
      ))}
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
        <textarea
          value={text}
          placeholder={running ? 'the agent is working…' : 'message the agent, or / for commands'}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              void send()
            }
          }}
        />
        <div className="chat-input-row">
          {running ? (
            <button onClick={() => void post(`/api/chat/sessions/${id}/cancel`)}>cancel</button>
          ) : (
            <button onClick={() => void send()} disabled={!text.trim()}>
              send
            </button>
          )}
        </div>
      </div>
    </>
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

  const rows = useMemo(() => followRows(trace as never, jobId), [trace, jobId])
  const plan: TurnProgress[] = useMemo(
    () => Object.values(turns).sort((a, b) => a.since - b.since),
    [turns],
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
    navigate(`/files/docs/${running.doc}${section}`)
  }, [plan, followMode, navigate])

  return (
    <>
      <div className="chat-plan">
        <label className="chat-follow-toggle">
          <input type="checkbox" checked={followMode} onChange={(e) => setFollowMode(e.target.checked)} />
          follow in editor
        </label>
        {plan.map((t) => (
          <div key={t.label} className={`plan-entry ${t.state}`}>
            <span className="plan-mark">
              {t.state === 'done' ? '✓' : t.state === 'failed' ? '✗' : t.state === 'running' ? '●' : '○'}
            </span>{' '}
            {t.label}
          </div>
        ))}
      </div>
      <Transcript rows={rows} follow />
    </>
  )
}
