// History: the list of builds with the human edits that started each cascade, in
// product words (docs/frontends/gui.md#history). One build opens its changes
// (the Changeset page); a range opens the diff.
import { Link } from 'react-router'
import { entryLabel, type JournalEntry } from '../lib/api'
import { useJournal } from '../lib/queries'
import { useWords } from '../lib/words'

const HUMAN: Record<string, string> = {
  edit: 'you edited a document',
  'dual-write': 'you edited a fact and its sentence',
  decree: 'you edited a fact in the model',
  ratify: 'you accepted a proposal',
  answer: 'you answered a question',
  triage: 'you triaged a problem',
}

function kindWords(e: JournalEntry, dev: boolean): string {
  if (e.kind && HUMAN[e.kind]) return HUMAN[e.kind]
  switch (e.kind) {
    case 'session':
      return dev ? entryLabel(e) : `a build read ${(e.batch ?? []).length || 'some'} item${(e.batch ?? []).length === 1 ? '' : 's'}`
    case 'gc':
      return 'the model was tidied'
    case 'align':
      return 'moved sections were re-anchored'
    case 'checks':
      return 'checks ran'
    case 'settle-diagnostics':
      return 'problems were settled'
    default:
      return e.kind ?? 'a change'
  }
}

export default function History() {
  const w = useWords()
  const journal = useJournal(200)
  const entries = journal.data?.entries ?? []
  if (journal.error) return <p className="error-inline">{journal.error.message}</p>
  return (
    <div className="ws-scroll narrow">
      <div className="page-head">
        <h1>{w.history}</h1>
        {journal.data && (
          <span className="muted">
            {w.buildNumber(journal.data.generation)} is the latest.{' '}
            {entries.length > 1 && (
              <Link to={`/history/diff?from=${Math.max(1, entries[entries.length - 1].generation)}&to=${entries[0].generation}`}>What changed across all of them →</Link>
            )}
          </span>
        )}
      </div>
      {entries.length === 0 && journal.data && (
        <div className="empty-state">
          <b>Nothing yet</b>
          The first build writes the first entry here.
        </div>
      )}
      {entries.map((e) => {
        const human = !!(e.kind && HUMAN[e.kind])
        return (
          <div key={e.generation} className="hist-row">
            <Link to={`/history/${e.generation}`} className="mono">
              {w.buildNumber(e.generation)}
            </Link>
            <span className={human ? 'human' : ''}>
              {kindWords(e, w.dev)}
              {e.dirtied && e.dirtied.length > 0 && <span className="muted"> · {e.dirtied.join(', ')}</span>}
              {(e.resolved_goals ?? []).length > 0 && <span className="muted"> · {(e.resolved_goals ?? []).length} resolved</span>}
              {(e.opened_goals ?? []).length > 0 && <span className="muted"> · {(e.opened_goals ?? []).length} opened</span>}
            </span>
            <span className="muted mono">
              {e.mutations.length} change{e.mutations.length === 1 ? '' : 's'}
              {e.tokens > 0 ? ` · ${Math.round(e.tokens / 1000)}k tok` : ''}
            </span>
          </div>
        )
      })}
    </div>
  )
}
