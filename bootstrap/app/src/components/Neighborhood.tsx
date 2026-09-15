// The neighborhood widget: the selected entity and its one-hop neighbors with
// typed edges, drawn from the graph as a small radial picture. The only map the
// default mode shows (docs/frontends/gui.md#explore).
import { useMemo } from 'react'
import { useNavigate } from 'react-router'
import type { Graph } from '../lib/api'
import { useNodeHref } from '../lib/nav'

const MAX = 12
const W = 300
const H = 200
const R = 78

interface Neighbor {
  id: string
  name: string
  types: string[]
  out: boolean
}

export default function Neighborhood({ id, graph }: { id: string; graph: Graph }) {
  const navigate = useNavigate()
  const href = useNodeHref()
  const neighbors = useMemo<Neighbor[]>(() => {
    const by = new Map<string, Neighbor>()
    for (const rel of Object.values(graph.relationships)) {
      if (!rel.members.includes(id)) continue
      for (const c of rel.contributions ?? []) {
        const other = c.a === id ? c.b : c.b === id ? c.a : null
        if (!other || other === id) continue
        const n = by.get(other) ?? { id: other, name: graph.entities[other]?.name ?? other.replace(/^ent:/, ''), types: [], out: c.a === id }
        if (!n.types.includes(c.type)) n.types.push(c.type)
        by.set(other, n)
      }
    }
    const parent = graph.entities[id]?.parent
    if (parent && !by.has(parent)) by.set(parent, { id: parent, name: graph.entities[parent]?.name ?? parent, types: ['part of'], out: true })
    for (const [cid, e] of Object.entries(graph.entities)) {
      if (e.parent === id && !by.has(cid)) by.set(cid, { id: cid, name: e.name, types: ['contains'], out: true })
      if (by.size >= MAX) break
    }
    return [...by.values()].slice(0, MAX)
  }, [graph, id])

  if (neighbors.length === 0) return null
  const me = graph.entities[id]
  const cx = W / 2
  const cy = H / 2
  const short = (s: string) => (s.length > 14 ? `${s.slice(0, 13)}…` : s)
  return (
    <svg className="nbh" viewBox={`0 0 ${W} ${H}`} width="100%" role="img" aria-label={`${me?.name ?? id} and its neighbors`}>
      {neighbors.map((n, i) => {
        const a = (i / neighbors.length) * Math.PI * 2 - Math.PI / 2
        const x = cx + Math.cos(a) * R
        const y = cy + Math.sin(a) * R
        const mx = (cx + x) / 2
        const my = (cy + y) / 2
        return (
          <g key={n.id} className="nbh-n" onClick={() => navigate(href(n.id))} style={{ cursor: 'pointer' }}>
            <line x1={cx} y1={cy} x2={x} y2={y} className={`nbh-edge ${n.types[0]?.replace(/\s/g, '-') ?? ''}`} />
            <text x={mx} y={my} className="nbh-type" textAnchor="middle">
              {n.types.join(', ')}
            </text>
            <circle cx={x} cy={y} r={5} className="nbh-node" />
            <text x={x} y={y + (y < cy ? -9 : 15)} className="nbh-name" textAnchor="middle">
              {short(n.name)}
            </text>
          </g>
        )
      })}
      <circle cx={cx} cy={cy} r={7} className="nbh-me" />
      <text x={cx} y={cy + 20} className="nbh-name me" textAnchor="middle">
        {short(me?.name ?? id)}
      </text>
    </svg>
  )
}
