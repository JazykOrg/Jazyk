// Release diff: per-node before and after between two generations,
// reconstructed by replaying the journal server-side.
import { useSearchParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { get } from '../lib/api'
import { useStatus } from '../lib/queries'
import NodeLink from '../components/NodeLink'
import './routes.css'

interface Change {
  kind: 'entity' | 'requirement' | 'diagnostic'
  change: 'added' | 'removed' | 'changed'
  before?: unknown
  after?: unknown
}

interface Diff {
  from: number
  to: number
  changes: Record<string, Change>
}

type DiffLine = { text: string; mark: '' | 'del' | 'add' }

// Line-level LCS diff over the pretty-printed bodies; no library.
function lineDiff(before: unknown, after: unknown): DiffLine[] {
  const a = JSON.stringify(before ?? null, null, 2).split('\n')
  const b = JSON.stringify(after ?? null, null, 2).split('\n')
  const n = a.length
  const m = b.length
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0))
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1])
  const out: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ text: a[i], mark: '' })
      i++
      j++
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      out.push({ text: a[i], mark: 'del' })
      i++
    } else {
      out.push({ text: b[j], mark: 'add' })
      j++
    }
  }
  while (i < n) out.push({ text: a[i++], mark: 'del' })
  while (j < m) out.push({ text: b[j++], mark: 'add' })
  return out
}

const CHIP: Record<Change['change'], string> = { added: 'v-ok', removed: 'v-bad', changed: 'sev-info' }
const KINDS: Change['kind'][] = ['entity', 'requirement', 'diagnostic']
const CHANGES: Change['change'][] = ['added', 'changed', 'removed']

export default function ReleaseDiff() {
  const status = useStatus()
  const [sp, setSp] = useSearchParams()
  const current = status.data?.generation ?? 0
  const to = Number(sp.get('to')) || current
  const from = Number(sp.get('from')) || Math.max(1, to - 10)

  const q = useQuery({
    queryKey: ['journal', 'diff', from, to],
    queryFn: () => get<Diff>(`/api/diff?from=${from}&to=${to}`),
    enabled: to > 0,
  })

  const setRange = (f: number, t: number) => {
    const next = new URLSearchParams(sp)
    next.set('from', String(f))
    next.set('to', String(t))
    setSp(next, { replace: true })
  }

  const changes = Object.entries(q.data?.changes ?? {})
  const count = (kind: string, change: string) =>
    changes.filter(([, c]) => c.kind === kind && c.change === change).length

  return (
    <div>
      <h1>Release diff</h1>
      <div className="filterbar mono">
        from{' '}
        <input
          type="number"
          min={1}
          value={from}
          style={{ width: 80 }}
          onChange={(e) => setRange(Number(e.target.value), to)}
        />{' '}
        to{' '}
        <input
          type="number"
          min={1}
          value={to}
          style={{ width: 80 }}
          onChange={(e) => setRange(from, Number(e.target.value))}
        />
      </div>

      {q.error && (
        <p className="error-inline">
          {q.error.message}{' '}
          <a href="#retry" onClick={(e) => { e.preventDefault(); q.refetch() }}>retry</a>
        </p>
      )}
      {!q.data && !q.error && <p className="muted">loading…</p>}

      {q.data && (
        <>
          <p className="muted">
            {KINDS.map((k) =>
              CHANGES.map((c) => (count(k, c) ? `${count(k, c)} ${k} ${c}` : null))
                .filter(Boolean)
                .join(', '),
            )
              .filter(Boolean)
              .join(' · ') || 'no changes in this range'}
          </p>
          {KINDS.map((kind) => {
            const ofKind = changes.filter(([, c]) => c.kind === kind)
            if (ofKind.length === 0) return null
            return (
              <section key={kind}>
                <h2>{kind}s</h2>
                {CHANGES.map((change) => {
                  const group = ofKind
                    .filter(([, c]) => c.change === change)
                    .sort(([a], [b]) => a.localeCompare(b))
                  if (group.length === 0) return null
                  return group.map(([id, c]) => (
                    <div key={id} className="card">
                      <p style={{ margin: '2px 0' }}>
                        <span className={`chip ${CHIP[change]}`}>{change}</span>
                        <NodeLink id={id} />
                      </p>
                      {change === 'changed' && (
                        <pre className="pack">
                          {lineDiff(c.before, c.after).map((l, i) => (
                            <span
                              key={i}
                              className={`diff-line ${l.mark === 'del' ? 'diff-del' : l.mark === 'add' ? 'diff-add' : ''}`}
                            >
                              {l.text || ' '}
                            </span>
                          ))}
                        </pre>
                      )}
                      {change !== 'changed' && (
                        <details>
                          <summary>body</summary>
                          <pre className="pack">
                            {JSON.stringify(change === 'added' ? c.after : c.before, null, 2)}
                          </pre>
                        </details>
                      )}
                    </div>
                  ))
                })}
              </section>
            )
          })}
        </>
      )}
    </div>
  )
}
