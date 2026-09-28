// The Inbox: every request the compiler makes of a human, in one place, with one
// vocabulary (docs/frontends/gui.md#inbox). Sections: questions, proposals,
// problems, pending changes, drafts. Every section lists the ids the rollup names,
// so a count and its list never disagree.
import { useMemo, useState } from 'react'
import { Link, useParams } from 'react-router'
import { type Diagnostic } from '../lib/api'
import { useInbox, type InboxRollup } from '../lib/product'
import { useDocs, useGenPending, useGraph } from '../lib/queries'
import { docHref } from '../lib/nav'
import { useWords } from '../lib/words'
import InboxCard, { sectionOf } from '../components/InboxCard'
import NodeLink from '../components/NodeLink'
import '../shell/shell.css'

const SEV_RANK: Record<string, number> = { error: 0, warning: 1, info: 2, none: 3 }

type Item = [string, Diagnostic]
type Section = keyof NonNullable<InboxRollup['sections']>

// The diagnostics a section lists, by the rollup's ids, most severe first.
function useSection(section: Section): { items: Item[]; ready: boolean } {
  const graph = useGraph()
  const inbox = useInbox()
  const ids = inbox.data?.sections?.[section]
  const items = useMemo(() => {
    const all = graph.data?.diagnostics ?? {}
    return (ids ?? [])
      .map((id) => [id, all[id]] as Item)
      .filter(([, d]) => !!d)
      .sort((a, b) => (SEV_RANK[a[1].severity] ?? 3) - (SEV_RANK[b[1].severity] ?? 3))
  }, [graph.data, ids])
  return { items, ready: !!ids && !!graph.data }
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

// While the rollup loads, or why it did not.
function NotYet() {
  const inbox = useInbox()
  const graph = useGraph()
  const err = inbox.error ?? graph.error
  if (err) return <p className="inbox-error" role="alert">The Inbox could not load: {(err as Error).message}</p>
  return <p className="muted">loading…</p>
}

export function Questions() {
  const { items, ready } = useSection('questions')
  const w = useWords()
  if (!ready) return <NotYet />
  if (items.length === 0) return <Empty title={`No ${w.questions.toLowerCase()}`} text="When a build finds a sentence it cannot settle, it asks here." />
  return (
    <>
      {items.map(([id, d]) => (
        <InboxCard key={id} id={id} d={d} />
      ))}
    </>
  )
}

export function Proposals() {
  const w = useWords()
  const { items, ready } = useSection('proposals')
  if (!ready) return <NotYet />
  if (items.length === 0)
    return <Empty title={`No ${w.proposals.toLowerCase()}`} text="A fact inferred by a build, or one you edited in the model, asks to be written into a document here." />
  // Grouped by target document: one reviewed draft per document.
  const groups = new Map<string, Item[]>()
  for (const it of items) {
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
            <InboxCard key={id} id={id} d={d} proposal />
          ))}
        </div>
      ))}
    </>
  )
}

export function Problems() {
  const w = useWords()
  const [sev, setSev] = useState('')
  const [rule, setRule] = useState('')
  const [doc, setDoc] = useState('')
  const { items: ps, ready } = useSection('problems')
  if (!ready) return <NotYet />
  const rules = [...new Set(ps.map(([, d]) => d.rule))].sort()
  const shown = ps.filter(([, d]) => (!sev || d.severity === sev) && (!rule || d.rule === rule) && (!doc || (d.subjects ?? []).some((s) => s.includes(doc))))
  if (ps.length === 0) return <Empty title={`No ${w.problems.toLowerCase()}`} text="Contradictions, gaps, broken links, and failing tests land here after a build or a review." />
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
        <InboxCard key={id} id={id} d={d} />
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
  if (inbox.error && !d) return <p className="inbox-error" role="alert">Pending changes could not load: {(inbox.error as Error).message}</p>
  if (!d) return <p className="muted">loading…</p>
  const list = d.pending.docs
  const entities = pending.data?.pending ?? []
  const manual = d.modes.compile === 'manual'
  const genManual = d.modes.generate === 'manual'
  if (list.length === 0 && (!genManual || entities.length === 0))
    return <Empty title="Nothing pending" text={manual ? 'Documents you edit wait here until you run a build.' : 'Build on save is on: every saved change builds by itself.'} />
  return (
    <>
      {list.length > 0 && (
        <>
          <h2>Documents changed since their last build</h2>
          {list.map((p) => {
            const info = docs?.docs.find((x) => x.path === p)
            const goals = Object.entries(info?.goals ?? {}).reduce((a, [, n]) => a + n, 0)
            return (
              <div key={p} className="inbox-item">
                <div className="head">
                  <Link to={docHref(p)}>{p}</Link>
                  <span className="muted">{goals > 0 ? `a build would read ${goals} section${goals === 1 ? '' : 's'} here` : 'a build would read this document'}</span>
                </div>
              </div>
            )
          })}
          <p className="muted">Run {w.build.toLowerCase()} from the button in the top bar.</p>
        </>
      )}
      {genManual && entities.length > 0 && (
        <>
          <h2>Entities to generate</h2>
          {entities.map((e) => (
            <div key={e.entity} className="inbox-item">
              <div className="head">
                <NodeLink id={e.entity} />
                <span className="muted">{e.reason}</span>
              </div>
            </div>
          ))}
          <p className="muted">Run {w.generate.toLowerCase()} from the button in the top bar.</p>
        </>
      )}
    </>
  )
}

export function Drafts() {
  const w = useWords()
  const { items: ds, ready } = useSection('drafts')
  if (!ready) return <NotYet />
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
          <Questions />
        </>
      )}
      {(!section || section === 'proposals') && (
        <>
          {!section && <h2>{w.proposals}</h2>}
          <Proposals />
        </>
      )}
      {section === 'problems' && <Problems />}
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
      {section === 'drafts' && <Drafts />}
    </div>
  )
}
