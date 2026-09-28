// The chat pane: the persistent conversation surface on the far right. Chat sessions
// talk to the configured ACP agent (in a tab, the loop in the worker); follow entries render the running build from the
// live trace the jobs already stream. Mirrors docs/frontends/gui.md#chat.
import { useEffect, useMemo, useRef, useState } from 'react'
import { useLocation, useNavigate, useSearchParams } from 'react-router'
import { del, get, post, verdictText } from '../lib/api'
import { useApp, type TurnProgress } from '../lib/store'
import { needsPageModel, useCapabilities, type Capabilities } from '../lib/capabilities'
import ModelSourcePanel from '../components/ModelSourcePanel'
import { toolWords, workingLine } from '../lib/working'
import { useWords, verdictWords } from '../lib/words'
import { batchWords, chatRows, followRows, type Row } from '../lib/transcript'

interface Command {
  name: string
  description: string
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
  const hidden = useApp((a) => a.hiddenThreads)
  const hideThread = useApp((a) => a.hideThread)
  const chatUpdates = useApp((a) => a.chatUpdates)
  // The conversation whose × was clicked once and waits for the second click.
  const [confirming, setConfirming] = useState<string | null>(null)
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
      .filter((j) => !hidden.includes(j.id))
      .sort((a, b) => Number(live(b)) - Number(live(a)) || b.id - a.id)
      .slice(0, 8)
      .map((j) => `follow-${j.id}`)
  }, [jobs, hidden])

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

  // Every thread closes (docs/frontends/gui.md#chat): a conversation is deleted,
  // after a second click when it has messages; a build thread leaves the list.
  const closeChat = async (id: string) => {
    const said = (sessions[id]?.updates ?? 0) > 0 || (chatUpdates[id]?.length ?? 0) > 0
    if (said && confirming !== id) {
      setConfirming(id)
      return
    }
    setConfirming(null)
    if (sessions[id]?.state === 'running') await post(`/api/chat/sessions/${id}/cancel`).catch(() => {})
    await del(`/api/chat/sessions/${encodeURIComponent(id)}`).catch(() => {})
    const app = useApp.getState()
    const { [id]: _gone, ...rest } = app.chatSessions
    app.setChatSessions(Object.values(rest))
    if (active === id) select(null)
  }
  const closeThread = (id: string) => {
    hideThread(Number(id.slice(7)))
    if (active === id) select(null)
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
          const verb = j?.kind.kind === 'compile' ? w.build : j?.kind.kind === 'gen' ? w.generate : j?.kind.kind === 'verify' || j?.kind.kind === 'audit' ? w.review : (j?.kind.kind ?? 'build')
          return (
            <span key={id} className={`chat-sess${active === id ? ' active' : ''}${live ? ' working' : ''}`}>
              <button className="chat-sess-open" onClick={() => select(id)} title={`build thread for job ${id.slice(7)}`}>
                <span className={`dot ${live ? 'running' : j?.state === 'failed' ? 'failed' : ''}`} /> {verb} #{id.slice(7)}
              </button>
              <button className="chat-sess-close" onClick={() => closeThread(id)} title="close this build thread" aria-label={`close ${verb} #${id.slice(7)}`}>
                ×
              </button>
            </span>
          )
        })}
        {chatIds.map((id) => (
          <span key={id} className={`chat-sess${active === id ? ' active' : ''}${sessions[id].state === 'running' ? ' working' : ''}${confirming === id ? ' confirming' : ''}`}>
            <button className="chat-sess-open" onClick={() => select(id)}>
              <span className={`dot ${sessions[id].state}`} /> {sessions[id].title}
              {sessions[id].pending.length > 0 && <span className="ask-badge">?</span>}
            </button>
            <button
              className="chat-sess-close"
              onClick={() => void closeChat(id)}
              onBlur={() => confirming === id && setConfirming(null)}
              title={confirming === id ? 'click again to delete this conversation' : 'delete this conversation'}
              aria-label={confirming === id ? `delete ${sessions[id].title}: click again` : `delete ${sessions[id].title}`}
            >
              {confirming === id ? 'Delete?' : '×'}
            </button>
          </span>
        ))}
      </div>
      {active?.startsWith('follow-') ? (
        <FollowView jobId={Number(active.slice(7))} />
      ) : chatId ? (
        <ChatView id={chatId} />
      ) : (
        <div className="chat-empty muted">
          {noAgent ? (
            <p>This backend has no agent to talk to. Builds still show here as build threads while they run.</p>
          ) : (
            <p>Ask about the project, or run a build and follow it here.</p>
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

// The model's reasoning: muted lines while it streams, collapsed to its first line
// once the turn moves on, a click opening it (docs/frontends/gui.md#chat).
function Thought({ text, live }: { text: string; live: boolean }) {
  const [open, setOpen] = useState(false)
  const first = text.trim().split('\n')[0] ?? ''
  const more = text.trim() !== first
  if (live || open)
    return (
      <pre className="chat-thought-text" onClick={() => !live && setOpen(false)} title={live ? undefined : 'collapse'}>
        {text.trim()}
      </pre>
    )
  return (
    <button type="button" className="chat-thought-line" onClick={() => setOpen(true)} title="show the whole reasoning" aria-expanded={false}>
      <span className="chat-thought-mark">▸</span> {first}
      {more ? ' …' : ''}
    </button>
  )
}

// `working` is the pulsing line at the end while a turn runs: what the agent is
// doing; `live` says the last row is still being written (docs/frontends/gui.md#chat).
function Transcript({ rows, follow, working, live = false }: { rows: Row[]; follow: boolean; working?: string | null; live?: boolean }) {
  const endRef = useRef<HTMLDivElement>(null)
  const dev = useApp((a) => a.dev)
  const lastText = rows.length > 0 ? rows[rows.length - 1].text.length : 0
  useEffect(() => {
    if (follow) endRef.current?.scrollIntoView({ block: 'end' })
  }, [rows.length, lastText, follow, working])
  return (
    <div className="chat-transcript">
      {rows.map((r, i) => (
        <div key={r.key} className={`chat-row ${r.kind} ${r.status ?? ''}${r.kind === 'thought' && live && i === rows.length - 1 ? ' working' : ''}`}>
          {r.kind === 'tool' ? (
            <details open={dev || undefined}>
              <summary>
                <span className={`tool-dot ${r.status}`} /> {r.text}
              </summary>
              {r.detail && <pre>{r.detail}</pre>}
            </details>
          ) : r.kind === 'thought' ? (
            <Thought text={r.text} live={live && i === rows.length - 1} />
          ) : (
            <pre title={r.detail}>{r.text}</pre>
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
  const seedChatUpdates = useApp((a) => a.seedChatUpdates)
  const info = useApp((a) => a.chatSessions[id])

  // Replay the session on first open (a reload arrives mid-conversation).
  useEffect(() => {
    if (updates !== undefined) return
    get<{ updates: { n: number; update: Record<string, unknown> }[] }>(`/api/chat/sessions/${id}`)
      .then((r) => seedChatUpdates(id, r.updates ?? []))
      .catch(() => seedChatUpdates(id, []))
  }, [id, updates, seedChatUpdates])

  const rows = useMemo(() => chatRows(updates ?? []), [updates])
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
      <Transcript rows={rows} follow working={running ? workingLine(rows) : null} live={running} />
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
            : v.replace(/^converged.*/, 'Up to date').replace(/^incomplete.*/, 'Build incomplete')
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
    // Reasoning streaming in is the work shown itself.
    if (last && last.event.kind === 'modelText') return null
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
      <Transcript rows={rows} follow working={working} live={live} />
      <LoadedPanel block={loadedBlock} />
    </>
  )
}
