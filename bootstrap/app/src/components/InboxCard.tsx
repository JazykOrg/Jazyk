// One Inbox item: the card every surface shows for an open diagnostic, the Inbox's
// lists and the inspector's problem panel alike (docs/frontends/gui.md#questions,
// docs/frontends/gui.md#inspector). The message, the question with its options and
// the freeform box, the related requirements by statement, the reasoning, the other
// subjects, and the triage actions. Every write shows its pending state and then its
// result; a refusal shows under the card in the backend's words, never silently.
import { useState } from 'react'
import { Link } from 'react-router'
import { type Diagnostic } from '../lib/api'
import { useCapabilities } from '../lib/capabilities'
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

interface AnswerReply {
  status?: string
  doc?: string
  note?: string
}

// The answer area: the options and the freeform box while the question stands, the
// recorded answer and its progress once it is answered.
function Answer({ id, d, proposal, write }: { id: string; d: Diagnostic; proposal?: boolean; write: Write }) {
  const [text, setText] = useState('')
  const caps = useCapabilities()
  // A demo is a tab too: nothing but the server runs an answer session on the reply.
  const tab = !!caps.data && caps.data.backend !== 'server'
  const status = d.answer?.status
  const url = `/api/questions/${encodeURIComponent(id)}/answer`
  const said = (r: AnswerReply) =>
    r.status === 'applied' ? (r.doc ? `Applied: the edit landed in ${r.doc}.` : 'Applied.') : r.status === 'handling' ? (r.note ?? 'Recorded; an answer session acts on it.') : null
  const send = () => {
    const t = text.trim()
    if (!t || write.pending) return
    void write.run<AnswerReply>('text', url, { text: t }, (r) => {
      setText('')
      return said(r)
    })
  }
  if (d.lifecycle && d.lifecycle !== 'open')
    return (
      <p className="muted answer-state" data-state="resolved">
        Resolved{d.answer?.text ? `, answered “${d.answer.text}”` : ''}.
      </p>
    )
  if (!d.prompt) return null
  if (status === 'handling')
    return (
      <p className="muted answer-state" data-state="handling">
        <span className="dot running" /> Answered{d.answer?.text ? `: “${d.answer.text}”` : ''}.{' '}
        {tab ? 'The next build acts on the answer.' : 'Being handled…'}
      </p>
    )
  if (status === 'applied' || status === 'handled')
    return (
      <p className="muted answer-state" data-state={status}>
        Answered{d.answer?.text ? `: “${d.answer.text}”` : ''}. {status === 'applied' ? 'Applied.' : 'Handled.'}
      </p>
    )
  return (
    <>
      {status === 'failed' && (
        <p className="inbox-error answer-state" data-state="failed">
          The answer “{d.answer?.text}” could not be acted on. Answer again.
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
          <button className="send" aria-label="send the answer" title="send (Enter)" aria-busy={write.pending === 'text'} disabled={write.pending !== null || !text.trim()} onClick={send}>
            {write.pending === 'text' ? '…' : '↵'}
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
