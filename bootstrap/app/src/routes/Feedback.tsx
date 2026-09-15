// Feedback: what models reported about jazyk's own prompts and tools, newest first.
// The log is written by the report_feedback tool and read by jazyk's developers; the
// project's authors never see it (docs/frontends/gui.md#feedback).
import { useSearchParams } from 'react-router'
import { useFeedback } from '../lib/queries'
import { useOpenActivity } from '../lib/nav'
import type { FeedbackEntry } from '../lib/api'
import './routes.css'

const KINDS = ['ambiguous', 'wrong', 'confusing', 'missing', 'other'] as const

// The kind carries the sting: a wrong tool is worse than an ambiguous one.
function kindClass(kind: string): string {
  if (kind === 'wrong') return 'v-bad'
  if (kind === 'ambiguous' || kind === 'confusing') return 'v-stale'
  if (kind === 'missing') return 'sev-info'
  return 'v-none'
}

const ts = (t: string) => (t ? new Date(t).toLocaleString() : '')

export default function Feedback() {
  const [sp, setSp] = useSearchParams()
  const kind = sp.get('kind')
  const openActivity = useOpenActivity()
  const q = useFeedback()

  const setKind = (k: string | null) => {
    const next = new URLSearchParams(sp)
    if (k) next.set('kind', k)
    else next.delete('kind')
    setSp(next, { replace: true })
  }

  if (q.error)
    return (
      <p className="error-inline">
        {q.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); q.refetch() }}>retry</a>
      </p>
    )
  if (!q.data) return <p className="muted">loading…</p>

  const all = q.data.entries
  const counts: Record<string, number> = {}
  for (const e of all) counts[e.kind] = (counts[e.kind] ?? 0) + 1
  const rows = kind ? all.filter((e) => e.kind === kind) : all

  return (
    <div>
      <h1>Feedback</h1>
      <p className="muted">
        What the models reported about jazyk itself: a prompt, a tool, a schema, or an error message that was
        ambiguous, wrong, or confusing. Written by the <span className="mono">report_feedback</span> tool into{' '}
        <span className="mono">feedback.jsonl</span> in the out directory. Findings about this project's documents are
        diagnostics, not feedback.
      </p>

      <div className="actionrow">
        <a
          href="#all"
          className={kind ? 'muted' : ''}
          onClick={(e) => { e.preventDefault(); setKind(null) }}
        >
          all {all.length}
        </a>
        {KINDS.filter((k) => counts[k]).map((k) => (
          <a
            key={k}
            href={`#${k}`}
            className={kind === k ? '' : 'muted'}
            onClick={(e) => { e.preventDefault(); setKind(kind === k ? null : k) }}
          >
            {k} {counts[k]}
          </a>
        ))}
      </div>

      {rows.length === 0 && <p className="empty">no feedback recorded</p>}
      {rows.map((e, i) => (
        <Entry key={`${e.at}-${i}`} entry={e} openRun={openActivity} />
      ))}
    </div>
  )
}

function Entry({ entry, openRun }: { entry: FeedbackEntry; openRun: (run?: string) => void }) {
  const refs = [
    entry.source,
    entry.task,
    entry.target,
    entry.client,
    entry.model,
    entry.codec,
    entry.generation !== undefined ? `g${entry.generation}` : null,
  ].filter(Boolean)
  return (
    <div className="card">
      <p className="row" style={{ margin: '2px 0' }}>
        <span className={`chip ${kindClass(entry.kind)}`}>{entry.kind}</span>
        {entry.subject && <span className="mono">{entry.subject}</span>}
        <span className="muted" style={{ marginLeft: 'auto' }}>{ts(entry.at)}</span>
      </p>
      <p style={{ margin: '6px 0', whiteSpace: 'pre-wrap' }}>{entry.message}</p>
      <p className="muted mono" style={{ margin: '2px 0', fontSize: 11 }}>
        {refs.join(' · ')}
        {entry.run && (
          <>
            {refs.length > 0 ? ' · ' : ''}
            <a href={`#run`} onClick={(ev) => { ev.preventDefault(); openRun(entry.run) }}>
              {entry.run}
            </a>
          </>
        )}
      </p>
    </div>
  )
}
