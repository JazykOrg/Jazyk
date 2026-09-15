// The open document's outline under the tree: its sections with their read
// state, the requirements and entities each carries, and the verification
// rollup, the linkage rail beside the text (docs/frontends/gui.md#docs).
import { useMemo } from 'react'
import { Link, useLocation, useSearchParams } from 'react-router'
import { useCoverage, useGraph, useMatrix } from '../lib/queries'
import { useApp } from '../lib/store'
import { coverageWord } from '../lib/words'
import { aggClass } from '../components/Cards'
import { docHref } from '../lib/nav'

export default function Outline() {
  const loc = useLocation()
  const [sp] = useSearchParams()
  const docPath = loc.pathname.startsWith('/docs/') ? decodeURIComponent(loc.pathname.slice('/docs/'.length)) : ''
  const coverage = useCoverage()
  const graph = useGraph()
  const matrix = useMatrix()
  const dev = useApp((a) => a.dev)
  const current = sp.get('section') ?? ''
  const rec = coverage.data?.[docPath]

  const perSection = useMemo(() => {
    const g = graph.data
    const m = new Map<string, { reqs: string[]; ents: Set<string> }>()
    if (!g || !docPath) return m
    const at = (ref: string) => {
      let x = m.get(ref)
      if (!x) {
        x = { reqs: [], ents: new Set() }
        m.set(ref, x)
      }
      return x
    }
    for (const [rid, r] of Object.entries(g.requirements)) if (r.source?.doc === docPath) at(r.source.section).reqs.push(rid)
    for (const [eid, e] of Object.entries(g.entities)) for (const mention of e.mentions ?? []) if (mention.doc === docPath) at(mention.section).ents.add(eid)
    return m
  }, [graph.data, docPath])

  if (!docPath || !rec) return null
  const rows = matrix.data?.rows ?? {}
  const sections = Object.entries(rec.sections)
    .filter(([, s]) => s.kind !== 'root')
    .sort((a, b) => a[1].order - b[1].order)
  if (sections.length === 0) return null
  return (
    <div className="wb-explorer-section outline">
      <div className="wb-explorer-label">outline</div>
      {sections.map(([ref, s]) => {
        const cov = (rec.coverage ?? {})[ref]
        const state = cov?.state
        const depth = Math.max(0, ref.split('/').length - 2)
        const x = perSection.get(ref)
        const reqs = x?.reqs ?? []
        const ents = x?.ents.size ?? 0
        const agg = reqs.length > 0 ? aggClass(reqs, rows) : ''
        return (
          <Link
            key={ref}
            to={docHref(docPath, ref)}
            className={`outline-row${current === ref ? ' active' : ''}`}
            style={{ paddingLeft: 12 + depth * 10 }}
            title={`${coverageWord(state, dev)}${cov?.note ? `: ${cov.note}` : ''}`}
          >
            <span className={`outline-dot ${state === 'covered' ? 'read' : state === 'non-normative' ? 'info' : 'unread'}`} />
            <span className="outline-title">{s.title || ref}</span>
            {reqs.length > 0 && (
              <span className={`outline-chip ${agg}`} title={`${reqs.length} requirement${reqs.length === 1 ? '' : 's'}`}>
                {reqs.length}r
              </span>
            )}
            {ents > 0 && (
              <span className="outline-chip" title={`${ents} entit${ents === 1 ? 'y' : 'ies'} mentioned`}>
                {ents}e
              </span>
            )}
          </Link>
        )
      })}
    </div>
  )
}
