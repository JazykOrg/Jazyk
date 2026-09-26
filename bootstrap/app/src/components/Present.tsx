// The pieces every page is built from under the presentation rules
// (docs/frontends/gui.md#presentation): a node by its name, a status in words, a
// sentence with where it comes from, a list that starts short, and the Developer
// details that keep the compiler's records without putting them in the way.
import { useState } from 'react'
import { Link } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { post, type Diagnostic } from '../lib/api'
import { useApp } from '../lib/store'
import { docHref, useNodeHref } from '../lib/nav'
import { shorten, statusWord, useNodeName, useSectionLabel } from '../lib/present'
import { useResolveId } from './NodeLink'

// Text from the documents with its inline code marked: `sort` renders as code.
export function InlineText({ text }: { text: string }) {
  const parts = text.split(/(`[^`]+`)/g)
  return (
    <>
      {parts.map((p, i) =>
        p.length > 2 && p.startsWith('`') && p.endsWith('`') ? <code key={i}>{p.slice(1, -1)}</code> : <span key={i}>{p}</span>,
      )}
    </>
  )
}

// A node by its name. The id is the tooltip, and sits beside the name in Developer
// mode. A long statement is cut at `max` characters, whole in the tooltip.
export function NameLink({ id, max, children }: { id: string; max?: number; children?: React.ReactNode }) {
  const resolved = useResolveId(id)
  const href = useNodeHref()
  const name = useNodeName()(resolved)
  const dev = useApp((a) => a.dev)
  const text = max ? shorten(name, max) : name
  return (
    <>
      <Link className="name-link" to={href(resolved)} title={name === text ? resolved : `${name}\n${resolved}`}>
        {children ?? <InlineText text={text} />}
      </Link>
      {dev && <span className="mono muted dev-id"> {resolved}</span>}
    </>
  )
}

export function StatusWord({ status }: { status?: string | null }) {
  const { word, cls } = statusWord(status)
  const dev = useApp((a) => a.dev)
  return (
    <span className="status-word" title={status ?? 'no trace row'}>
      <span className={`status-dot ${cls}`} />
      {word}
      {dev && status && <span className="mono muted"> ({status})</span>}
    </span>
  )
}

// Where a sentence comes from: the file and the section's title, opening the editor
// at the quote, and the quote itself.
export function SourceQuote({ doc, section, quote }: { doc: string; section?: string; quote?: string }) {
  const label = useSectionLabel()
  return (
    <div className="source-quote">
      <Link className="source-ref" to={docHref(doc, section, quote)} title={section ? `${doc}#${section}` : doc}>
        {label(doc, section)}
      </Link>
      {quote && (
        <blockquote>
          <InlineText text={quote} />
        </blockquote>
      )}
    </div>
  )
}

// A list that shows its first few rows and a toggle for the rest.
export function ShowMore<T>({ items, first = 5, render }: { items: T[]; first?: number; render: (t: T, i: number) => React.ReactNode }) {
  const [all, setAll] = useState(false)
  const shown = all ? items : items.slice(0, first)
  return (
    <>
      {shown.map(render)}
      {items.length > first && (
        <button className="show-more" onClick={() => setAll(!all)}>
          {all ? 'show fewer' : `show all ${items.length}`}
        </button>
      )}
    </>
  )
}

// A titled part of a page. Nothing renders for an empty part unless it says why.
export function Part({ title, children, note }: { title: string; children?: React.ReactNode; note?: string }) {
  return (
    <section className="part">
      <h2>{title}</h2>
      {note && <p className="muted part-note">{note}</p>}
      {children}
    </section>
  )
}

// The compiler's own records about a node, at the foot of its page, only in
// Developer mode.
export function DevDetails({ children }: { children: React.ReactNode }) {
  const dev = useApp((a) => a.dev)
  if (!dev) return null
  return (
    <details className="dev-details">
      <summary>Developer details</summary>
      {children}
    </details>
  )
}

const SEVERITY: Record<string, string> = { error: 'Error', warning: 'Warning', info: 'Note', note: 'Note' }
const TRIAGE: Record<string, string> = { acknowledged: 'Seen', suppressed: 'Hidden', wontfix: "Won't fix" }

// A problem in the product's words: its severity, its message, its question, and
// the triage actions (docs/frontends/gui.md#vocabulary).
export function ProblemItem({ id, d }: { id: string; d: Diagnostic }) {
  const qc = useQueryClient()
  const dev = useApp((a) => a.dev)
  const triage = (t: string | null) =>
    post(`/api/diagnostics/${encodeURIComponent(id)}/triage`, { triage: t }).then(() => {
      qc.invalidateQueries({ queryKey: ['graph'] })
      qc.invalidateQueries({ queryKey: ['status'] })
      qc.invalidateQueries({ queryKey: ['board'] })
    })
  return (
    <div className={`problem sev-${d.severity}`}>
      <p className="problem-line">
        <span className={`chip sev-${d.severity}`}>{SEVERITY[d.severity] ?? d.severity}</span> {d.message}
        {d.triage && <span className="muted"> · {TRIAGE[d.triage] ?? d.triage}</span>}
        {dev && <span className="mono muted"> {d.rule} {id}</span>}
      </p>
      {d.prompt && <p className="problem-q">{d.prompt.question}</p>}
      {d.reasoning && (
        <details>
          <summary>why</summary>
          <p className="muted">{d.reasoning}</p>
        </details>
      )}
      <p className="row problem-actions">
        {d.triage !== 'acknowledged' && <button onClick={() => triage('acknowledged')}>Seen</button>}
        {d.triage !== 'wontfix' && <button onClick={() => triage('wontfix')}>Won't fix</button>}
        {d.triage !== 'suppressed' && <button onClick={() => triage('suppressed')}>Hide</button>}
        {d.triage && <button onClick={() => triage(null)}>Undo</button>}
      </p>
    </div>
  )
}

// A plain-language list of the requirement ids given, each as its statement with
// its status, opening its page.
export function RequirementList({ ids, rows, first = 6 }: { ids: string[]; rows: Record<string, { status?: string }>; first?: number }) {
  if (ids.length === 0) return <p className="muted">None yet.</p>
  return (
    <div className="req-list">
      <ShowMore
        items={ids}
        first={first}
        render={(rid) => (
          <div key={rid} className="req-row">
            <StatusDot status={rows[rid]?.status} />
            <NameLink id={rid} />
          </div>
        )}
      />
    </div>
  )
}

export function StatusDot({ status }: { status?: string | null }) {
  const { word, cls } = statusWord(status)
  return <span className={`status-dot ${cls}`} title={word} aria-label={word} />
}
