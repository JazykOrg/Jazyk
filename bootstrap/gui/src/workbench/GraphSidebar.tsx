// The graph sidebar: one text filter plus facet lists over the live shards
// (docs/frontends/gui.md#graph). A row opens the node in the inspector and
// focuses it on the map.
import { useMemo } from 'react'
import { useSearchParams } from 'react-router'
import { useCoverage, useGraph, useMatrix } from '../lib/queries'
import { verifyClass } from '../components/Chip'
import { reverseIndex } from '../components/Cards'

const LISTS = ['entities', 'requirements', 'diagnostics', 'coverage'] as const
type ListKind = (typeof LISTS)[number]
const WINDOW = 200

export default function GraphSidebar() {
  const [sp, setSp] = useSearchParams()
  const list: ListKind = (LISTS as readonly string[]).includes(sp.get('list') ?? '')
    ? (sp.get('list') as ListKind)
    : 'entities'
  const q = (sp.get('q') ?? '').toLowerCase()
  const node = sp.get('node')

  const graph = useGraph()
  const matrix = useMatrix()
  const coverage = useCoverage()
  const rows = matrix.data?.rows ?? {}
  const revIdx = useMemo(
    () => (graph.data ? reverseIndex(graph.data) : new Map<string, string[]>()),
    [graph.data],
  )

  const setParam = (k: string, v: string | null) => {
    const next = new URLSearchParams(sp)
    if (v === null) next.delete(k)
    else next.set(k, v)
    setSp(next, { replace: true })
  }

  // Open in the inspector and focus on the map, one click.
  const openAndFocus = (id: string) => {
    const next = new URLSearchParams(sp)
    next.set('node', id)
    next.set('focus', id)
    setSp(next)
  }

  const windowed = <T,>(items: T[], render: (t: T) => React.ReactNode) => (
    <>
      {items.slice(0, WINDOW).map(render)}
      {items.length > WINDOW && (
        <p className="muted" style={{ padding: '2px 12px', fontSize: 11 }}>
          showing {WINDOW} of {items.length}, refine the filter
        </p>
      )}
    </>
  )

  const g = graph.data

  return (
    <>
      <div className="wb-side-pad" style={{ paddingBottom: 4 }}>
        <input
          type="search"
          placeholder="filter"
          value={sp.get('q') ?? ''}
          onChange={(e) => setParam('q', e.target.value || null)}
        />
      </div>
      <div className="wb-side-tabs">
        {LISTS.map((t) => (
          <a
            key={t}
            href={`#${t}`}
            className={t === list ? 'active' : ''}
            onClick={(e) => {
              e.preventDefault()
              setParam('list', t)
            }}
          >
            {t}
          </a>
        ))}
      </div>
      {graph.error && <p className="error-inline wb-side-pad">{graph.error.message}</p>}
      {!g && !graph.error && <p className="muted wb-side-pad">loading…</p>}

      {g && list === 'entities' &&
        windowed(
          Object.entries(g.entities)
            .filter(
              ([id, e]) =>
                !q ||
                `${id} ${e.name} ${e.definition ?? ''} ${(e.aliases ?? []).join(' ')}`
                  .toLowerCase()
                  .includes(q),
            )
            .sort(([a], [b]) => a.localeCompare(b)),
          ([id, e]) => (
            <a
              key={id}
              className={`wb-list-row mono${node === id ? ' active' : ''}`}
              onClick={() => openAndFocus(id)}
            >
              {e.name} <span className="sub">{id} · {(revIdx.get(id) ?? []).length} req</span>
            </a>
          ),
        )}

      {g && list === 'requirements' &&
        windowed(
          Object.entries(g.requirements)
            .filter(([id, r]) => !q || `${id} ${r.ears}`.toLowerCase().includes(q))
            .sort(([a], [b]) => a.localeCompare(b)),
          ([id, r]) => (
            <a
              key={id}
              className={`wb-list-row${node === id ? ' active' : ''}`}
              title={r.ears}
              onClick={() => openAndFocus(id)}
            >
              <span className={`mono ${verifyClass(rows[id]?.status)}`}>●</span>{' '}
              <span className="mono">{id}</span> <span className="sub">{r.ears}</span>
            </a>
          ),
        )}

      {g && list === 'diagnostics' &&
        windowed(
          Object.entries(g.diagnostics)
            .filter(([, d]) => d.triage !== 'suppressed')
            .filter(([id, d]) => !q || `${id} ${d.rule} ${d.message}`.toLowerCase().includes(q))
            .sort(([a], [b]) => a.localeCompare(b)),
          ([id, d]) => (
            <a
              key={id}
              className={`wb-list-row${node === id ? ' active' : ''}`}
              title={d.message}
              onClick={() => openAndFocus(id)}
            >
              <span className={`mono sev-${d.severity}`}>{d.severity}</span>{' '}
              <span className="mono">{d.rule}</span> <span className="sub">{d.message}</span>
            </a>
          ),
        )}

      {list === 'coverage' && (
        <>
          {!coverage.data && <p className="muted wb-side-pad">loading…</p>}
          {coverage.data &&
            Object.entries(coverage.data)
              .sort(([a], [b]) => a.localeCompare(b))
              .map(([doc, rec]) => {
                const secs = Object.keys(rec.sections)
                const covered = secs.filter(
                  (sid) => (rec.coverage ?? {})[sid]?.state === 'covered',
                ).length
                return (
                  <a
                    key={doc}
                    className="wb-list-row mono"
                    onClick={() => {
                      const next = new URLSearchParams(sp)
                      next.set('node', `doc:${doc}`)
                      next.set('focus', `doc:${doc}`)
                      setSp(next)
                    }}
                  >
                    {doc}{' '}
                    <span className="sub">
                      {covered}/{secs.length} covered
                    </span>
                  </a>
                )
              })}
        </>
      )}
    </>
  )
}
