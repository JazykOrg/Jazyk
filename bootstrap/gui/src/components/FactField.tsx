// One editable fact in the inspector. Saving goes through POST /api/facts/{id}/edit:
// a quote-provenanced fact answers with the proposed sentence rewrite to accept
// (the dual write), declining lands a decree with a ratification proposal.
// Mirrors docs/frontends/gui.md#inspector (editing facts).
import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { post } from '../lib/api'

interface Proposal {
  doc: string
  section: string
  old_text: string
  new_text: string
}

interface EditReply {
  proposal?: Proposal | null
  needsDecree?: boolean
  committed?: boolean
  path?: string
  ratification?: string | null
  note?: string
}

export default function FactField({
  id,
  field,
  value,
  multiline,
  label,
}: {
  id: string
  field: string
  value: string
  multiline?: boolean
  label?: string
}) {
  const qc = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState(value)
  const [proposal, setProposal] = useState<Proposal | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const refresh = () => {
    for (const key of ['graph', 'node', 'board', 'status', 'docs', 'views'])
      qc.invalidateQueries({ queryKey: [key] })
  }
  const close = () => {
    setEditing(false)
    setProposal(null)
    setNotice(null)
    setError(null)
  }

  const send = async (body: Record<string, unknown>) => {
    setBusy(true)
    setError(null)
    try {
      const r = await post<EditReply>(`/api/facts/${encodeURIComponent(id)}/edit`, body)
      if (r.proposal) {
        setProposal(r.proposal)
        setNotice(null)
      } else if (r.needsDecree) {
        setProposal(null)
        setNotice(r.note ?? 'no mechanical rewrite; a decree is the available path')
      } else if (r.committed) {
        refresh()
        close()
      }
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  if (!editing) {
    return (
      <span className="fact-field">
        {value || <span className="muted">(unset)</span>}{' '}
        <button
          className="ide-mini"
          title={`edit ${label ?? field}`}
          onClick={() => {
            setText(value)
            setEditing(true)
          }}
        >
          ✎
        </button>
      </span>
    )
  }

  return (
    <span className="fact-field" style={{ display: 'block' }}>
      {multiline ? (
        <textarea rows={3} value={text} onChange={(e) => setText(e.target.value)} style={{ width: '100%' }} />
      ) : (
        <input type="text" value={text} onChange={(e) => setText(e.target.value)} style={{ width: '100%' }} />
      )}
      {proposal && (
        <span className="card" style={{ display: 'block', margin: '4px 0' }}>
          <span className="muted">
            the sentence rewrite in {proposal.doc}#{proposal.section}:
          </span>
          <span className="v-bad" style={{ display: 'block', textDecoration: 'line-through' }}>
            {proposal.old_text}
          </span>
          <span className="v-ok" style={{ display: 'block' }}>{proposal.new_text}</span>
          <span className="row" style={{ marginTop: 4 }}>
            <button disabled={busy} onClick={() => void send({ field, value: text, proposal })}>
              accept: prose and graph together
            </button>
            <button
              disabled={busy}
              title="decline the rewrite; the edit lands graph-only with a ratification proposal"
              onClick={() => void send({ field, value: text, decree: true })}
            >
              decree instead
            </button>
          </span>
        </span>
      )}
      {notice && (
        <span className="muted" style={{ display: 'block', margin: '2px 0' }}>
          {notice}{' '}
          <button disabled={busy} onClick={() => void send({ field, value: text, decree: true })}>
            decree
          </button>
        </span>
      )}
      {error && <span className="error-inline" style={{ display: 'block' }}>{error}</span>}
      {!proposal && !notice && (
        <span className="row" style={{ marginTop: 2 }}>
          <button disabled={busy || !text.trim()} onClick={() => void send({ field, value: text })}>
            save
          </button>
          <button disabled={busy} onClick={close}>cancel</button>
        </span>
      )}
      {(proposal || notice) && (
        <span className="row" style={{ marginTop: 2 }}>
          <button disabled={busy} onClick={close}>cancel</button>
        </span>
      )}
    </span>
  )
}
