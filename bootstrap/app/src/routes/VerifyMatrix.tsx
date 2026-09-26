// The verification matrix: every ledger row with its derived status, grouped by
// entity, with the staleness cascade explained per row and run actions. A backend
// that runs no tests shows the rows without the actions
// (docs/frontends/wasm.md#the-product).
import { useState } from 'react'
import { getText, post, tokenParam, type VerifyRow } from '../lib/api'
import { backend } from '../lib/backend'
import { useCanRun } from '../lib/capabilities'
import { useStatus } from '../lib/queries'
import { useWords } from '../lib/words'
import { useMatrix } from '../lib/queries'
import { useApp } from '../lib/store'
import NodeLink from '../components/NodeLink'
import { VerifyChip, verifyClass } from '../components/Chip'
import { aggClass } from '../components/Cards'
import './routes.css'

export default function VerifyMatrix() {
  const matrix = useMatrix()
  const jobs = useApp((a) => a.jobs)
  const busy = Object.values(jobs).some((j) => j.state === 'running' || j.state === 'queued')
  const [status, setStatus] = useState<string | null>(null)
  const [entity, setEntity] = useState('')
  const [kind, setKind] = useState<string | null>(null)
  const w = useWords()
  const { data: proj } = useStatus()
  const canRun = useCanRun()
  const canVerify = canRun('verify')
  const canGen = canRun('gen')
  const exportHref = (fmt: string) => `/api/verify/matrix?format=${fmt}${tokenParam() ? `&${tokenParam()}` : ''}`
  // The server answers the link itself; a backend in the tab has no URL to link to,
  // so the page reads the export through it and saves the text as a file.
  const saveExport = (fmt: string) => async (e: React.MouseEvent) => {
    if (backend().kind === 'server') return
    e.preventDefault()
    const text = await getText(`/api/verify/matrix?format=${fmt}`)
    const url = URL.createObjectURL(new Blob([text], { type: fmt === 'csv' ? 'text/csv' : 'text/markdown' }))
    const a = document.createElement('a')
    a.href = url
    a.download = `trace-matrix.${fmt}`
    a.click()
    window.setTimeout(() => URL.revokeObjectURL(url), 10_000)
  }

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
      <div className="page-head">
        <h1>{w.trace}</h1>
        <span className="muted">every requirement against the product: its test, its verdict, its repair</span>
      </div>
      <div className="stat-strip">
        {Object.entries(counts).map(([st, n]) => (
          <span key={st} className={`stat ${verifyClass(st)}`}>
            <b>{n}</b> {st}
          </span>
        ))}
        {proj && (
          <span className="stat" title="sections of the documents the model has read">
            <b>{proj.coverage.covered}</b>/{proj.coverage.total} sections read
          </span>
        )}
      </div>
      <div className="actionrow">
        {canVerify && (
          <button disabled={busy} onClick={() => post('/api/jobs', { kind: 'verify', targets: [] })} title="run every test whose row asks for it">
            {w.check} everything pending
          </button>
        )}
        {canRun('audit') && (
          <button disabled={busy} onClick={() => post('/api/jobs', { kind: 'audit' })} title="re-run every test, verified rows included">
            Audit
          </button>
        )}
        {!canVerify && <span className="muted">Tests do not run here: this backend has no shell.</span>}
        <span className="muted">export</span>
        <a href={exportHref('csv')} download="trace-matrix.csv" onClick={saveExport('csv')}>
          csv
        </a>
        <a href={exportHref('md')} download="trace-matrix.md" onClick={saveExport('md')}>
          markdown
        </a>
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
                {canVerify && ent.startsWith('ent:') && (
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
                      {canVerify && (
                        <button
                          disabled={busy}
                          onClick={() => post('/api/jobs', { kind: 'verify', targets: [id] })}
                        >
                          run
                        </button>
                      )}
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
                            {canGen && (
                              <button
                                disabled={busy}
                                onClick={() => post('/api/jobs', { kind: 'gen', entities: [r.entity] })}
                              >
                                gen
                              </button>
                            )}
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
