// The writes a diagnostic takes from a person, and the triage actions every surface
// shows (docs/frontends/gui.md#problems). A write shows which control is pending,
// then its result; a refusal shows inline in the backend's words, never silently.
import { useState } from 'react'
import { useQueryClient, type QueryClient } from '@tanstack/react-query'
import { post, type Diagnostic } from '../lib/api'
import { refreshAfterWrite } from '../lib/events'
import { useApp } from '../lib/store'
import { triageWords } from '../lib/words'

// Everything an answer or a triage can change, refetched: the graph, the documents
// an edit option wrote, and the Inbox's own rollup.
export function refreshInbox(qc: QueryClient) {
  refreshAfterWrite(qc)
}

// One write: which control is pending, the note a success leaves, and the refusal.
export function useWrite() {
  const qc = useQueryClient()
  const [pending, setPending] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [note, setNote] = useState<string | null>(null)
  const run = async <T,>(key: string, path: string, body: unknown, after?: (r: T) => string | null) => {
    setPending(key)
    setError(null)
    setNote(null)
    try {
      const r = await post<T>(path, body)
      setNote(after ? after(r) : null)
      refreshInbox(qc)
    } catch (e) {
      const msg = (e as Error).message
      setError(msg && msg !== 'undefined' ? msg : 'the request failed with no reason given')
    } finally {
      setPending(null)
    }
  }
  return { pending, error, note, run }
}

export type Write = ReturnType<typeof useWrite>

// Seen, Hidden, Won't fix: a click sets it, a click on the active one clears it.
// Given a card's write, the refusal shows under the card; alone, beside the buttons.
export default function Triage({ id, d, write }: { id: string; d: Diagnostic; write?: Write }) {
  const own = useWrite()
  const w = write ?? own
  const dev = useApp((a) => a.dev)
  return (
    <span className="triage">
      {triageWords(dev).map((t) => {
        const active = d.triage === t.value
        const busy = w.pending === `triage:${t.value}`
        return (
          <button
            key={t.value}
            className={`${active ? 'active' : ''}${busy ? ' pending' : ''}`}
            aria-pressed={active}
            aria-busy={busy}
            disabled={w.pending !== null}
            title={active ? 'click again to clear' : undefined}
            onClick={() => void w.run(`triage:${t.value}`, `/api/diagnostics/${encodeURIComponent(id)}/triage`, { triage: active ? null : t.value })}
          >
            {busy ? `${t.label}…` : t.label}
          </button>
        )
      })}
      {!write && own.error && (
        <span className="inbox-error" role="alert">
          {own.error}
        </span>
      )}
    </span>
  )
}
