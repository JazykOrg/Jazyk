// One Inbox item: the card every surface shows for an open diagnostic, the Inbox's
// lists and the inspector's problem panel alike (docs/frontends/gui.md#questions,
// docs/frontends/gui.md#inspector). The message, the question with its options and
// the freeform box, the related requirements by statement, the reasoning, the other
// subjects, and the triage actions. Every write shows its pending state and then its
// result; a refusal shows under the card in the backend's words, never silently.
import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { post, type Diagnostic } from '../lib/api'
import { needsBuildModel, useCapabilities } from '../lib/capabilities'
import { useApp } from '../lib/store'
import { docHref } from '../lib/nav'
import { SevChip } from './Chip'
import NodeLink from './NodeLink'
import SectionLink from './SectionLink'
import { NameLink } from './Present'
import Triage, { useWrite, type Write } from './Triage'
import '../shell/shell.css'

export const PROPOSAL_RULES = new Set(['ratification-pending', 'invented-choice'])

// A section reference among the subjects, for the jump to the editor.
export function sectionOf(d: Diagnostic): { doc: string; section?: string } | null {
  for (const s of d.subjects ?? []) {
    if (s.endsWith('.md')) return { doc: s }
    const m = s.match(/^(.+\.md)#(.*)$/)
    if (m) return { doc: m[1], section: m[2] }
  }
  return null
}

export interface AnswerReply {
  status?: string
  doc?: string
  note?: string
  edit?: { doc: string; section?: string; newText: string }
}

// What every answer leaves behind in the page: the time it was sent, which names the
// changes that follow it, and for an applied option the text it landed, marked in the
// editor until the mark fades (docs/frontends/gui.md#questions).
export function noteAnswer(r: AnswerReply) {
  const app = useApp.getState()
  app.noteAnswered()
  if (r.status === 'applied' && r.edit?.newText) app.addFlash({ doc: r.edit.doc, text: r.edit.newText, label: 'changed by your answer' })
}

// The words a reply leaves on the card.
export function replyWords(r: AnswerReply, tab: boolean): string | null {
  if (r.status === 'applied') return r.doc ? `Applied: the edit landed in ${r.doc}.` : 'Applied.'
  if (r.status === 'handling') return tab ? 'Queued for the next build.' : 'Recorded; an answer session acts on it now.'
  return null
}

// The generation a diagnostic was last written in, for the link to its change.
function generationOf(d: Diagnostic): number | null {
  const m = /^g(\d+)$/.exec(d.updated ?? '')
  return m ? Number(m[1]) : null
}

// A recorded answer in a tab waits for a build: this starts one.
function BuildNow() {
  const caps = useCapabilities()
  const running = useApp((a) => Object.values(a.jobs).some((j) => j.state === 'running'))
  const [error, setError] = useState<string | null>(null)
  const [starting, setStarting] = useState(false)
  const noModel = needsBuildModel(caps.data)
  const start = async () => {
    setStarting(true)
    setError(null)
    try {
      await post('/api/jobs', { kind: 'compile' })
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setStarting(false)
    }
  }
  return (
    <>
      <button
        className="build-now"
        disabled={noModel || running || starting}
        title={noModel ? 'choose a model first: the arrow beside Build' : running ? 'a build is running; it acts on the answer if it has not started its sessions' : 'run the build now'}
        onClick={() => void start()}
      >
        {running ? 'Building…' : starting ? 'Starting…' : 'Build now'}
      </button>
      {error && (
        <span className="inbox-error" role="alert">
          {error}
        </span>
      )}
    </>
  )
}

// The answer area: the options and the freeform box while the question stands, the
// recorded answer and its progress once it is answered.
function Answer({ id, d, proposal, write }: { id: string; d: Diagnostic; proposal?: boolean; write: Write }) {
  const [text, setText] = useState('')
  const caps = useCapabilities()
  const qc = useQueryClient()
  // A demo is a tab too: nothing but the server runs an answer session on the reply.
  const tab = !!caps.data && caps.data.backend !== 'server'
  const status = d.answer?.status
  const url = `/api/questions/${encodeURIComponent(id)}/answer`
  const said = (r: AnswerReply) => {
    noteAnswer(r)
    return replyWords(r, tab)
  }
  const send = () => {
    const t = text.trim()
    if (!t || write.pending) return
    void write.run<AnswerReply>('text', url, { text: t }, (r) => {
      setText('')
      return said(r)
    })
  }
  // On a server the answer session runs now: the card follows it to its outcome,
  // refreshed on every commit and polled while it runs (docs/frontends/gui.md#questions).
  const handlingOnServer = status === 'handling' && !tab && d.lifecycle === 'open'
  useEffect(() => {
    if (!handlingOnServer) return
    const t = window.setInterval(() => void qc.invalidateQueries({ queryKey: ['graph'] }), 4000)
    return () => window.clearInterval(t)
  }, [handlingOnServer, qc])
  const gen = generationOf(d)
  const change = gen !== null ? <Link to={`/history/${gen}`}>see the change</Link> : null
  if (d.lifecycle && d.lifecycle !== 'open')
    return (
      <p className="muted answer-state" data-state="resolved">
        Resolved{d.answer?.text ? `, answered “${d.answer.text}”` : ''}.{change && <> {change}.</>}
      </p>
    )
  if (!d.prompt) return null
  if (status === 'handling')
    return tab ? (
      <p className="muted answer-state" data-state="handling">
        <span className="dot queued" /> Answered{d.answer?.text ? `: “${d.answer.text}”` : ''}. Queued for the next build. <BuildNow />
      </p>
    ) : (
      <p className="muted answer-state" data-state="handling">
        <span className="dot running" /> Answered{d.answer?.text ? `: “${d.answer.text}”` : ''}. Being handled…
      </p>
    )
  if (status === 'applied' || status === 'handled')
    return (
      <p className="muted answer-state" data-state={status}>
        Answered{d.answer?.text ? `: “${d.answer.text}”` : ''}. {status === 'applied' ? 'Applied.' : 'Handled.'}
        {change && <> {change}.</>}
      </p>
    )
  return (
    <>
      {status === 'failed' && (
        <p className="inbox-error answer-state" data-state="failed">
          The answer “{d.answer?.text}” could not be acted on{d.answer?.error ? `: ${d.answer.error}` : ''}. Answer again.
        </p>
      )}
      <div className="opts">
        {(d.prompt.options ?? []).map((o, i) => {
          const busy = write.pending === `option:${i}`
          return (
            <button
              key={i}
              disabled={write.pending !== null}
              aria-busy={busy}
              className={`${o.edit ? 'edit' : ''}${busy ? ' pending' : ''}`}
              title={o.edit ? (proposal ? 'accept: the sentence lands in the document' : 'applies the edit to the document and resolves') : tab ? 'recorded; the next build acts on it' : 'recorded and handled by the agent'}
              onClick={() => void write.run<AnswerReply>(`option:${i}`, url, { option: i }, said)}
            >
              {o.edit ? (proposal ? 'Accept: ' : '✎ ') : ''}
              {o.label}
              {busy ? ' …' : ''}
            </button>
          )
        })}
      </div>
      {d.prompt.freeform && (
        <div className="free-row">
          <textarea
            className="free"
            rows={1}
            aria-label="answer in your own words"
            disabled={write.pending !== null}
            value={text}
            placeholder="answer in your own words, enter to send"
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              // Enter sends; Shift+Enter makes a new line.
              if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault()
                send()
              }
            }}
          />
          <button className="send" title="send the answer (Enter)" aria-busy={write.pending === 'text'} disabled={write.pending !== null || !text.trim()} onClick={send}>
            {write.pending === 'text' ? 'Sending…' : 'Send'}
          </button>
        </div>
      )}
    </>
  )
}

export default function InboxCard({ id, d, proposal }: { id: string; d: Diagnostic; proposal?: boolean }) {
  const write = useWrite()
  const sec = sectionOf(d)
  const subjects = d.subjects ?? []
  const requirements = subjects.filter((s) => s.startsWith('req:'))
  const others = subjects.filter((s) => !s.startsWith('req:') && !s.includes('.md'))
  return (
    <div className="inbox-item" data-diag={id}>
      <div className="head">
        <SevChip severity={d.severity} />
        <span className="muted mono">{d.rule}</span>
        {sec && (
          <Link to={docHref(sec.doc, sec.section)} className="muted">
            {sec.doc}
            {sec.section ? `#${sec.section}` : ''}
          </Link>
        )}
        <NodeLink id={id} />
        <Triage id={id} d={d} write={write} />
      </div>
      <p style={{ margin: '4px 0' }}>{d.message}</p>
      {d.prompt && <p className="q">{d.prompt.question}</p>}
      <Answer id={id} d={d} proposal={proposal} write={write} />
      {write.error && (
        <p className="inbox-error" role="alert">
          {write.error}
        </p>
      )}
      {write.note && (
        <p className="muted inbox-note" role="status">
          {write.note}
        </p>
      )}
      {requirements.length > 0 && (
        <div className="related">
          <span className="muted">Related</span>
          <ul>
            {requirements.map((r) => (
              <li key={r}>
                <NameLink id={r} />
              </li>
            ))}
          </ul>
        </div>
      )}
      {d.reasoning && <p className="reasoning">{d.reasoning}</p>}
      {(others.length > 0 || sec) && (
        <div className="subjects">
          {others.slice(0, 6).map((s) => (
            <NodeLink key={s} id={s} />
          ))}
          {sec && (
            <SectionLink doc={sec.doc} section={sec.section}>
              open in the editor →
            </SectionLink>
          )}
        </div>
      )}
    </div>
  )
}
