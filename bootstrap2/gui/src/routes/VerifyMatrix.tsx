// The verification matrix: every ledger row with its derived status, grouped by
// entity, with the staleness cascade explained per row and run actions.
import { useState } from 'react'
import { post, type VerifyRow } from '../lib/api'
import { useMatrix } from '../lib/queries'
import { useApp } from '../lib/store'
import NodeLink from '../components/NodeLink'
import { VerifyChip, verifyClass } from '../components/Chip'
import { aggClass } from './Ir'
import './routes.css'

export default function VerifyMatrix() {
  const matrix = useMatrix()
  const jobs = useApp((a) => a.jobs)
  const busy = Object.values(jobs).some((j) => j.state === 'running' || j.state === 'queued')
  const [status, setStatus] = useState<string | null>(null)
  const [entity, setEntity] = useState('')
  const [kind, setKind] = useState<string | null>(null)

  if (matrix.error)
    return (
      <p className="error-inline">
        {matrix.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); matrix.refetch() }}>retry</a>
      </p>
    )
  if (!matrix.data) return <p className="muted">loading…</p>
  const { rows, counts } = matrix.data

  const kinds = [...new Set(Object.values(rows).map((r) => r.test?.kind).filter((k): k is string => !!k))].sort()

  const visible = Object.entries(rows).filter(([, r]) => {
    if (status && r.status !== status) return false
    if (entity && !(r.entity ?? '').toLowerCase().includes(entity.toLowerCase())) return false
    if (kind && r.test?.kind !== kind) return false
    return true
  })

  // Group by owning entity.
  const groups = new Map<string, [string, VerifyRow][]>()
  for (const [id, r] of visible) {
    const key = r.entity ?? '(no entity)'
    const g = groups.get(key)
    if (g) g.push([id, r])
    else groups.set(key, [[id, r]])
  }

  return (
    <div>
      <h1>Verification</h1>
      <div className="actionrow">
        <button disabled={busy} onClick={() => post('/api/jobs', { kind: 'verify', targets: [] })}>
          run all pending
        </button>
        <button disabled={busy} onClick={() => post('/api/jobs', { kind: 'audit' })}>
          audit
        </button>
      </div>

      <div className="filterbar">
        {Object.entries(counts)
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([st, n]) => (
            <span
              key={st}
              className={`chip facet ${verifyClass(st)} ${status && status !== st ? 'off' : ''}`}
              onClick={() => setStatus(status === st ? null : st)}
            >
              {n} {st}
            </span>
          ))}
        <input
          type="search"
          placeholder="filter by entity"
          value={entity}
          onChange={(e) => setEntity(e.target.value)}
          style={{ maxWidth: 240 }}
        />
        {kinds.map((k) => (
          <span
            key={k}
            className={`chip facet ${kind && kind !== k ? 'off' : ''}`}
            onClick={() => setKind(kind === k ? null : k)}
          >
            {k}
          </span>
        ))}
      </div>

      {groups.size === 0 && <p className="empty">no verification rows match</p>}
      {[...groups.entries()]
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([ent, list]) => {
          const agg = aggClass(list.map(([id]) => id), rows)
          return (
            <div key={ent} className={`card ${agg}`}>
              <p className="row" style={{ margin: '2px 0 6px' }}>
                {ent.startsWith('ent:') ? <NodeLink id={ent} /> : <span className="mono">{ent}</span>}
                <span className={`chip ${agg.replace('agg-', 'v-')}`}>
                  {list.length} rows
                </span>
                {ent.startsWith('ent:') && (
                  <button
                    disabled={busy}
                    onClick={() => post('/api/jobs', { kind: 'verify', targets: [ent] })}
                  >
                    run entity
                  </button>
                )}
              </p>
              {list
                .sort(([a], [b]) => a.localeCompare(b))
                .map(([id, r]) => (
                  <div key={id} style={{ margin: '4px 0', borderTop: '1px solid var(--line)', paddingTop: 4 }}>
                    <p className="row" style={{ margin: 0 }}>
                      <NodeLink id={id} />
                      <VerifyChip status={r.status} />
                      {r.test?.kind && <span className="muted">{r.test.kind}</span>}
                      {r.test?.run && <span className="mono muted oneline">{r.test.run}</span>}
                      {r.lastRun && <span className="muted">{r.lastRun}</span>}
                      <button
                        disabled={busy}
                        onClick={() => post('/api/jobs', { kind: 'verify', targets: [id] })}
                      >
                        run
                      </button>
                    </p>
                    {r.evidence && (
                      <p className="muted oneline" style={{ margin: '2px 0 0' }} title={r.evidence}>
                        {r.evidence}
                      </p>
                    )}
                    {r.status.startsWith('stale') && (
                      <p className="v-stale" style={{ margin: '2px 0 0' }}>
                        statement changed since generation
                        {r.reason ? ` (${r.reason})` : ''}
                        {r.entity && (
                          <>
                            ; regenerate <NodeLink id={r.entity} />{' '}
                            <button
                              disabled={busy}
                              onClick={() => post('/api/jobs', { kind: 'gen', entities: [r.entity] })}
                            >
                              gen
                            </button>
                          </>
                        )}
                      </p>
                    )}
                  </div>
                ))}
            </div>
          )
        })}
    </div>
  )
}
