// The open document's outline under the tree: its sections with their read
// state, the requirements and entities each carries, and the verification
// rollup. Clicking a section scrolls the editor to it, and the outline follows
// the editor back: the section holding the cursor (in Write) or the top of the
// view (in Read) is highlighted as the reader moves (docs/frontends/gui.md#docs).
import { useEffect, useMemo, useRef } from 'react'
import { Link, useLocation, useSearchParams } from 'react-router'
import { useCoverage, useGraph, useMatrix } from '../lib/queries'
import { useApp } from '../lib/store'
import { coverageWord } from '../lib/words'
import { aggClass } from '../components/Cards'
import { docHref } from '../lib/nav'
import { outlined, sectionAtLine, sortedSections } from '../lib/sections'

// The editor listens for this: the outline asks it to scroll to a section even
// when the route already names it.
export const REVEAL_EVENT = 'jazyk:reveal-section'

export default function Outline() {
  const loc = useLocation()
  const [sp] = useSearchParams()
  const docPath = loc.pathname.startsWith('/docs/') ? decodeURIComponent(loc.pathname.slice('/docs/'.length)) : ''
  const coverage = useCoverage()
  const graph = useGraph()
  const matrix = useMatrix()
  const dev = useApp((a) => a.dev)
  const editorLine = useApp((a) => a.editorLine)
  const rec = coverage.data?.[docPath]
  // Where the reader is wins over where the route pointed.
  const followed = rec && editorLine && editorLine.doc === docPath ? sectionAtLine(rec.sections, editorLine.line) : null
  const current = followed ?? sp.get('section') ?? ''
  const rowRefs = useRef(new Map<string, HTMLAnchorElement>())
  useEffect(() => {
    if (current) rowRefs.current.get(current)?.scrollIntoView({ block: 'nearest' })
  }, [current])

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
  // Every heading, the top-level ones included (a document's `# heading`, or
  // several), in document order.
  const sections = sortedSections(rec.sections).filter(([, s]) => outlined(s))
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
            ref={(el) => {
              if (el) rowRefs.current.set(ref, el)
              else rowRefs.current.delete(ref)
            }}
            to={docHref(docPath, ref)}
            className={`outline-row${current === ref ? ' active' : ''}`}
            style={{ paddingLeft: 12 + depth * 10 }}
            onClick={() => window.dispatchEvent(new CustomEvent(REVEAL_EVENT, { detail: { doc: docPath, section: ref } }))}
            title={[
              `${coverageWord(state, dev)}${cov?.note ? `: ${cov.note}` : ''}`,
              reqs.length > 0 ? `${reqs.length} requirement${reqs.length === 1 ? '' : 's'}` : '',
              ents > 0 ? `mentions ${ents} entit${ents === 1 ? 'y' : 'ies'}` : '',
            ]
              .filter(Boolean)
              .join(' · ')}
          >
            <span className={`outline-dot ${state === 'covered' ? 'read' : state === 'non-normative' ? 'info' : 'unread'}`} />
            <span className="outline-title">{s.title || ref}</span>
            {reqs.length > 0 && (
              <span className={`outline-chip ${agg}`} title={`${reqs.length} requirement${reqs.length === 1 ? '' : 's'} in this section`}>
                {reqs.length}
              </span>
            )}
          </Link>
        )
      })}
    </div>
  )
}
