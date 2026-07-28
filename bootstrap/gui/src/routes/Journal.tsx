// Journal: the changeset timeline, one row per generation.
import { useState } from 'react'
import { Link } from 'react-router'
import type { JournalEntry } from '../lib/api'
import { useJournal } from '../lib/queries'
import './routes.css'

// +created ~updated/merged -deleted, by op prefix.
export function opCounts(mutations: Record<string, unknown>[]): [number, number, number] {
  let a = 0
  let u = 0
  let d = 0
  for (const m of mutations) {
    const op = String(m.op ?? '')
    if (op.startsWith('create')) a++
    else if (op.startsWith('update') || op.startsWith('merge')) u++
    else if (op.startsWith('delete')) d++
  }
  return [a, u, d]
}

function matches(e: JournalEntry, f: string): boolean {
  if (!f) return true
  const q = f.toLowerCase()
  if (e.workItem.task.toLowerCase().includes(q)) return true
  if (e.workItem.target.toLowerCase().includes(q)) return true
  return e.mutations.some((m) => String(m.id ?? '').toLowerCase().includes(q))
}

export default function Journal() {
  const journal = useJournal(200)
  const [filter, setFilter] = useState('')

  if (journal.error)
    return (
      <p className="error-inline">
        {journal.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); journal.refetch() }}>retry</a>
      </p>
    )
  if (!journal.data) return <p className="muted">loading…</p>

  const entries = journal.data.entries.filter((e) => matches(e, filter))

  return (
    <div>
      <h1>Journal</h1>
      <div className="filterbar">
        <input
          type="search"
          placeholder="filter by target, task, or node id"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <Link to="/journal/diff">release diff</Link>
      </div>
      {entries.length === 0 && <p className="empty">no changesets match</p>}
      {entries.map((e) => {
        const [a, u, d] = opCounts(e.mutations)
        return (
          <p key={e.generation} className="mono oneline">
            <Link to={`/journal/${e.generation}`}>g{e.generation}</Link> · {e.workItem.task} ·{' '}
            {e.workItem.target} · <span className="v-ok">+{a}</span>{' '}
            <span className="sev-info">~{u}</span> <span className="v-bad">-{d}</span> · {e.rounds}{' '}
            rounds · {e.tokens} tok
          </p>
        )
      })}
    </div>
  )
}
