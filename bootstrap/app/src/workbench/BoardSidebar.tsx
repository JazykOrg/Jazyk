// The board sidebar: counts by class, kind, and state, the verdict line as the CLI
// prints it, filters, and this build's cost. Mirrors docs/frontends/gui.md#board.
import { useSearchParams } from 'react-router'
import { useBoard, useStatus } from '../lib/queries'

export default function BoardSidebar() {
  const board = useBoard()
  const status = useStatus()
  const [sp, setSp] = useSearchParams()

  const setFilter = (k: string, v: string | null) => {
    const next = new URLSearchParams(sp)
    if (v === null || v === '') next.delete(k)
    else next.set(k, v)
    setSp(next, { replace: true })
  }

  const c = board.data?.counts
  const costs = status.data?.costs
  const kinds = Object.keys(c?.by_kind ?? {})

  return (
    <div className="wb-side-pad">
      {board.error && <p className="error-inline">{board.error.message}</p>}
      {!board.data && !board.error && <p className="muted">loading…</p>}
      {board.data && (
        <>
          <p className="mono" title="the verdict line as jazyk compile prints it">
            {board.data.verdict}
          </p>
          {c && (
            <>
              <p className="muted mono" style={{ margin: '4px 0' }}>
                {c.open} open · {c.ready} ready · {c.blocked} blocked
                <br />
                {c.parked} parked · {c.failed} failed · {c.optional} optional
                {c.gated > 0 && (
                  <>
                    <br />
                    {c.gated} gated, awaiting release
                  </>
                )}
              </p>
              <div className="muted mono" style={{ margin: '4px 0' }}>
                {Object.entries(c.by_class).map(([cls, n]) => (
                  <div key={cls}>
                    {n} {cls}
                  </div>
                ))}
              </div>
              {kinds.length > 0 && (
                <div className="muted mono" style={{ margin: '4px 0' }}>
                  {kinds.map((k) => (
                    <div key={k}>
                      {c.by_kind[k]} {k}
                    </div>
                  ))}
                </div>
              )}
            </>
          )}
          <label className="muted" style={{ display: 'block', marginTop: 8 }}>
            class
            <select value={sp.get('class') ?? ''} onChange={(e) => setFilter('class', e.target.value)}>
              <option value="">all</option>
              <option value="compile">compile</option>
              <option value="gc">gc</option>
            </select>
          </label>
          <label className="muted" style={{ display: 'block' }}>
            kind
            <select value={sp.get('kind') ?? ''} onChange={(e) => setFilter('kind', e.target.value)}>
              <option value="">all</option>
              {kinds.map((k) => (
                <option key={k} value={k}>
                  {k}
                </option>
              ))}
            </select>
          </label>
          <label className="muted" style={{ display: 'block' }}>
            state
            <select value={sp.get('state') ?? ''} onChange={(e) => setFilter('state', e.target.value)}>
              <option value="">all</option>
              {['open', 'blocked', 'parked', 'failed'].map((s) => (
                <option key={s} value={s}>
                  {s}
                </option>
              ))}
            </select>
          </label>
          <label className="muted" style={{ display: 'block' }}>
            document
            <input
              type="text"
              placeholder="docs/…"
              value={sp.get('doc') ?? ''}
              onChange={(e) => setFilter('doc', e.target.value)}
            />
          </label>
          {costs && (costs.sessions ?? 0) > 0 && (
            <>
              <p className="muted" style={{ marginTop: 12, marginBottom: 2 }}>
                this build's cost
              </p>
              <p className="muted mono" style={{ margin: 0 }}>
                {costs.sessions} sessions · {Math.round((costs.tokens ?? 0) / 1000)}k tok
              </p>
              {Object.entries(costs.by_class ?? {}).map(([k, line]) => (
                <p key={k} className="muted mono" style={{ margin: 0, paddingLeft: 8 }}>
                  {k}: {line.sessions} · {Math.round(line.tokens / 1000)}k
                </p>
              ))}
              {Object.entries(costs.by_kind ?? {}).map(([k, line]) => (
                <p key={k} className="muted mono" style={{ margin: 0, paddingLeft: 16 }}>
                  {k}: {line.sessions} · {Math.round(line.tokens / 1000)}k
                </p>
              ))}
            </>
          )}
          {costs && (costs.sessions ?? 0) === 0 && (
            <p className="muted" style={{ marginTop: 12 }}>no cost recorded for this build yet</p>
          )}
        </>
      )}
    </div>
  )
}
