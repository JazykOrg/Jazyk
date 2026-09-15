// The Inbox: every request the compiler makes of a human, in one place, with one
// vocabulary (docs/frontends/gui.md#inbox). Sections: questions, proposals,
// problems, pending changes, drafts.
import { useMemo, useState } from 'react'
import { Link, useParams } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { post, type Diagnostic } from '../lib/api'
import { useInbox } from '../lib/product'
import { useDocs, useGenPending, useGraph } from '../lib/queries'
import { useApp } from '../lib/store'
import { docHref, useNodeHref } from '../lib/nav'
import { triageWords, useWords } from '../lib/words'
import { SevChip } from '../components/Chip'
import NodeLink from '../components/NodeLink'
import SectionLink from '../components/SectionLink'
import '../shell/shell.css'

const PROPOSAL_RULES = new Set(['ratification-pending', 'invented-choice'])
const DRAFT_RULE = 'unratified'
const SEV_RANK: Record<string, number> = { error: 0, warning: 1, info: 2, none: 3 }

type Item = [string, Diagnostic]

function useOpenDiagnostics(): Item[] {
  const graph = useGraph()
  return useMemo(() => {
    const all = Object.entries(graph.data?.diagnostics ?? {}) as Item[]
    return all
      .filter(([, d]) => (d.lifecycle ?? 'open') === 'open' && d.triage !== 'suppressed')
      .sort((a, b) => (SEV_RANK[a[1].severity] ?? 3) - (SEV_RANK[b[1].severity] ?? 3))
  }, [graph.data])
}

// A section reference among the subjects, for the jump to the editor.
function sectionOf(d: Diagnostic): { doc: string; section?: string } | null {
  for (const s of d.subjects ?? []) {
    if (s.endsWith('.md')) return { doc: s }
    const m = s.match(/^(.+\.md)#(.*)$/)
    if (m) return { doc: m[1], section: m[2] }
  }
  return null
}

function Subjects({ d }: { d: Diagnostic }) {
  const sec = sectionOf(d)
  return (
    <div className="subjects">
      {(d.subjects ?? [])
        .filter((s) => !s.includes('.md'))
        .slice(0, 6)
        .map((s) => (
          <NodeLink key={s} id={s} />
        ))}
      {sec && (
        <SectionLink doc={sec.doc} section={sec.section}>
          open in the editor →
        </SectionLink>
      )}
    </div>
  )
}

function Triage({ id, d }: { id: string; d: Diagnostic }) {
  const qc = useQueryClient()
  const dev = useApp((a) => a.dev)
  const set = async (t: string | null) => {
    await post(`/api/diagnostics/${encodeURIComponent(id)}/triage`, { triage: t })
    void qc.invalidateQueries({ queryKey: ['graph'] })
    void qc.invalidateQueries({ queryKey: ['inbox'] })
  }
  return (
    <span className="triage">
      {triageWords(dev).map((t) => (
        <button key={t.value} className={d.triage === t.value ? 'active' : ''} onClick={() => void set(d.triage === t.value ? null : t.value)}>
          {t.label}
        </button>
      ))}
    </span>
  )
}

function QuestionItem({ id, d, proposal }: { id: string; d: Diagnostic; proposal?: boolean }) {
  const qc = useQueryClient()
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const respond = async (body: Record<string, unknown>) => {
    setBusy(true)
    try {
      await post(`/api/questions/${encodeURIComponent(id)}/answer`, body)
    } finally {
      setBusy(false)
      void qc.invalidateQueries({ queryKey: ['graph'] })
      void qc.invalidateQueries({ queryKey: ['inbox'] })
    }
  }
  const handling = d.answer?.status === 'handling'
  const sec = sectionOf(d)
  return (
    <div className="inbox-item">
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
      </div>
      <p style={{ margin: '4px 0' }}>{d.message}</p>
      {d.prompt && <p className="q">{d.prompt.question}</p>}
      {handling ? (
        <p className="muted">
          <span className="dot running" /> being handled…
        </p>
      ) : (
        <>
          <div className="opts">
            {(d.prompt?.options ?? []).map((o, i) => (
              <button
                key={i}
                disabled={busy}
                className={o.edit ? 'edit' : ''}
                title={o.edit ? (proposal ? 'accept: the sentence lands in the document' : 'applies the edit to the document and resolves') : 'recorded and handled by the agent'}
                onClick={() => void respond({ option: i })}
              >
                {o.edit ? (proposal ? 'Accept: ' : '✎ ') : ''}
                {o.label}
              </button>
            ))}
          </div>
          {d.prompt?.freeform && (
            <input
              className="free"
              type="text"
              disabled={busy}
              value={text}
              placeholder="answer in your own words, enter to send"
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && text.trim()) void respond({ text: text.trim() })
              }}
            />
          )}
        </>
      )}
      {d.reasoning && <p className="reasoning">{d.reasoning}</p>}
      <Subjects d={d} />
    </div>
  )
}

function ProblemItem({ id, d }: { id: string; d: Diagnostic }) {
  const nodeHref = useNodeHref()
  return (
    <div className="inbox-item">
      <div className="head">
        <SevChip severity={d.severity} />
        <span className="muted mono">{d.rule}</span>
        <Link to={nodeHref(id)} className="muted mono">
          {id}
        </Link>
        <Triage id={id} d={d} />
      </div>
      <p style={{ margin: '4px 0' }}>{d.message}</p>
      {d.reasoning && <p className="reasoning">{d.reasoning}</p>}
      <Subjects d={d} />
    </div>
  )
}

function Empty({ title, text, action }: { title: string; text: string; action?: React.ReactNode }) {
  return (
    <div className="empty-state">
      <b>{title}</b>
      {text}
      {action && <div className="actions">{action}</div>}
    </div>
  )
}

export function Questions({ items }: { items: Item[] }) {
  const qs = items.filter(([, d]) => d.prompt && !PROPOSAL_RULES.has(d.rule) && d.rule !== DRAFT_RULE && (!d.answer || d.answer.status === 'failed' || d.answer.status === 'handling'))
  const w = useWords()
  if (qs.length === 0) return <Empty title={`No ${w.questions.toLowerCase()}`} text="When a sync finds a sentence it cannot settle, it asks here." />
  return (
    <>
      {qs.map(([id, d]) => (
        <QuestionItem key={id} id={id} d={d} />
      ))}
    </>
  )
}

export function Proposals({ items }: { items: Item[] }) {
  const w = useWords()
  const ps = items.filter(([, d]) => PROPOSAL_RULES.has(d.rule) && (!d.answer || d.answer.status === 'failed' || d.answer.status === 'handling'))
  if (ps.length === 0)
    return <Empty title={`No ${w.proposals.toLowerCase()}`} text="A fact inferred by a sync, or one you edited in the model, asks to be written into a document here." />
  // Grouped by target document: one reviewed draft per document.
  const groups = new Map<string, Item[]>()
  for (const it of ps) {
    const sec = sectionOf(it[1])
    const key = sec?.doc ?? 'other'
    const g = groups.get(key)
    if (g) g.push(it)
    else groups.set(key, [it])
  }
  return (
    <>
      {[...groups.entries()].map(([doc, list]) => (
        <div key={doc}>
          <h2>{doc === 'other' ? 'Without a target document' : doc}</h2>
          {list.map(([id, d]) => (
            <QuestionItem key={id} id={id} d={d} proposal />
          ))}
        </div>
      ))}
    </>
  )
}

export function Problems({ items }: { items: Item[] }) {
  const w = useWords()
  const [sev, setSev] = useState('')
  const [rule, setRule] = useState('')
  const [doc, setDoc] = useState('')
  const ps = items.filter(([, d]) => !d.prompt && !PROPOSAL_RULES.has(d.rule) && d.rule !== DRAFT_RULE)
  const rules = [...new Set(ps.map(([, d]) => d.rule))].sort()
  const shown = ps.filter(([, d]) => (!sev || d.severity === sev) && (!rule || d.rule === rule) && (!doc || (d.subjects ?? []).some((s) => s.includes(doc))))
  if (ps.length === 0) return <Empty title={`No ${w.problems.toLowerCase()}`} text="Contradictions, gaps, broken links, and failing tests land here after a sync or a check." />
  return (
    <>
      <div className="inbox-filters">
        <select value={sev} onChange={(e) => setSev(e.target.value)}>
          <option value="">every severity</option>
          <option value="error">errors</option>
          <option value="warning">warnings</option>
          <option value="info">notes</option>
        </select>
        <select value={rule} onChange={(e) => setRule(e.target.value)}>
          <option value="">every rule</option>
          {rules.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
        <input type="text" placeholder="document…" value={doc} onChange={(e) => setDoc(e.target.value)} style={{ maxWidth: 200 }} />
        <span className="muted">
          {shown.length} of {ps.length}
        </span>
      </div>
      {shown.map(([id, d]) => (
        <ProblemItem key={id} id={id} d={d} />
      ))}
    </>
  )
}

export function Pending() {
  const w = useWords()
  const inbox = useInbox()
  const pending = useGenPending()
  const { data: docs } = useDocs()
  const d = inbox.data
  const list = d?.pending.docs ?? []
  const entities = pending.data?.pending ?? []
  const manual = d?.modes.compile === 'manual'
  const genManual = d?.modes.generate === 'manual'
  if (!d) return <p className="muted">loading…</p>
  if (list.length === 0 && (!genManual || entities.length === 0))
    return <Empty title="Nothing pending" text={manual ? 'Documents you edit wait here until you run a sync.' : 'Sync on save is on: every saved change syncs by itself.'} />
  return (
    <>
      {list.length > 0 && (
        <>
          <h2>Documents changed since their last sync</h2>
          {list.map((p) => {
            const info = docs?.docs.find((x) => x.path === p)
            const goals = Object.entries(info?.goals ?? {}).reduce((a, [, n]) => a + n, 0)
            return (
              <div key={p} className="inbox-item">
                <div className="head">
                  <Link to={docHref(p)}>{p}</Link>
                  <span className="muted">{goals > 0 ? `a sync would read ${goals} section${goals === 1 ? '' : 's'} here` : 'a sync would read this document'}</span>
                </div>
              </div>
            )
          })}
          <p className="muted">Run {w.sync.toLowerCase()} from the button in the top bar.</p>
        </>
      )}
      {genManual && entities.length > 0 && (
        <>
          <h2>Entities to build</h2>
          {entities.map((e) => (
            <div key={e.entity} className="inbox-item">
              <div className="head">
                <NodeLink id={e.entity} />
                <span className="muted">{e.reason}</span>
              </div>
            </div>
          ))}
          <p className="muted">Run {w.build.toLowerCase()} from the button in the top bar.</p>
        </>
      )}
    </>
  )
}

export function Drafts({ items }: { items: Item[] }) {
  const w = useWords()
  const ds = items.filter(([, d]) => d.rule === DRAFT_RULE)
  if (ds.length === 0) return <Empty title={`No ${w.drafts.toLowerCase()}`} text={`${w.document} writes documents from the product; they wait here until you read and edit them.`} />
  return (
    <>
      {ds.map(([id, d]) => {
        const sec = sectionOf(d)
        return (
          <div key={id} className="inbox-item">
            <div className="head">
              <b>{sec?.doc ?? id}</b>
              {sec && <Link to={docHref(sec.doc)}>Open in editor →</Link>}
            </div>
            <p style={{ margin: '4px 0' }}>{d.message}</p>
            <p className="reasoning">Editing the document, even to accept it with a one-line change, clears this.</p>
          </div>
        )
      })}
    </>
  )
}

export default function Inbox() {
  const { section } = useParams()
  const items = useOpenDiagnostics()
  const w = useWords()
  const inbox = useInbox()
  const d = inbox.data
  const problems = Object.values(d?.problems ?? {}).reduce((a, b) => a + b, 0)
  const title =
    section === 'questions' ? w.questions : section === 'proposals' ? w.proposals : section === 'problems' ? w.problems : section === 'pending' ? w.pendingChanges : section === 'drafts' ? w.drafts : 'Inbox'
  return (
    <div className="ws-scroll narrow">
      <div className="page-head">
        <h1>{title}</h1>
        {!section && d && <span className="muted">{d.total === 0 ? 'nothing waiting on you' : `${d.total} waiting on you`}</span>}
      </div>
      {!section && d && (
        <div className="stat-strip">
          <Link to="/inbox/questions" className="stat">
            <b>{d.questions}</b> {w.questions.toLowerCase()}
          </Link>
          <Link to="/inbox/proposals" className="stat">
            <b>{d.proposals}</b> {w.proposals.toLowerCase()}
          </Link>
          <Link to="/inbox/problems" className="stat">
            <b>{problems}</b> {w.problems.toLowerCase()}
            {d.problems.error ? <span className="sev-error"> ({d.problems.error} errors)</span> : null}
          </Link>
          <Link to="/inbox/pending" className="stat">
            <b>{d.pending.docs.length}</b> {w.pendingChanges.toLowerCase()}
          </Link>
          <Link to="/inbox/drafts" className="stat">
            <b>{d.drafts}</b> {w.drafts.toLowerCase()}
          </Link>
        </div>
      )}
      {(!section || section === 'questions') && (
        <>
          {!section && <h2>{w.questions}</h2>}
          <Questions items={items} />
        </>
      )}
      {(!section || section === 'proposals') && (
        <>
          {!section && <h2>{w.proposals}</h2>}
          <Proposals items={items} />
        </>
      )}
      {section === 'problems' && <Problems items={items} />}
      {!section && problems > 0 && (
        <>
          <h2>{w.problems}</h2>
          <p className="muted">
            {problems} open. <Link to="/inbox/problems">Review them →</Link>
          </p>
        </>
      )}
      {(!section || section === 'pending') && (
        <>
          {!section && <h2>{w.pendingChanges}</h2>}
          <Pending />
        </>
      )}
      {section === 'drafts' && <Drafts items={items} />}
    </div>
  )
}
