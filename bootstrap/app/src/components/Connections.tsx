// Connections: one picture of an entity and its one-hop neighbors, each arrow
// labeled with its relation in words, the parent as `is part of` and, when the
// entity has no picture of its own inside, the children as `contains`. Selecting a
// neighbor lists the sentences that connect the two. The picture is the only place
// an entity's relations are stated (docs/frontends/gui.md#model).
import { useMemo, useState } from 'react'
import { Link } from 'react-router'
import type { Graph } from '../lib/api'
import { relationPhrase, shorten } from '../lib/present'
import { NameLink } from './Present'

const MAX = 12
const W = 560
const H = 320
const RX = 205
const RY = 118

interface Neighbor {
  id: string
  name: string
  // What the entity is to the neighbor, in words: "uses", "is part of".
  phrases: string[]
  // The requirements behind the connection.
  requirements: string[]
}

export default function Connections({ id, graph, withChildren }: { id: string; graph: Graph; withChildren: boolean }) {
  const [picked, setPicked] = useState<string | null>(null)
  const neighbors = useMemo<Neighbor[]>(() => {
    const by = new Map<string, Neighbor>()
    const at = (other: string) => {
      let n = by.get(other)
      if (!n) {
        n = { id: other, name: graph.entities[other]?.name ?? other.replace(/^ent:/, ''), phrases: [], requirements: [] }
        by.set(other, n)
      }
      return n
    }
    const e = graph.entities[id]
    if (e?.parent && graph.entities[e.parent]) at(e.parent).phrases.push('is part of')
    for (const rel of Object.values(graph.relationships)) {
      if (!rel.members.includes(id)) continue
      for (const c of rel.contributions ?? []) {
        const other = c.a === id ? c.b : c.b === id ? c.a : null
        if (!other || other === id) continue
        const n = at(other)
        const phrase = relationPhrase(c.type, c.a === id)
        // A composition toward the parent says the same as `is part of`.
        if (!n.phrases.includes(phrase) && !(phrase === 'is part of' && n.phrases.includes('is part of'))) n.phrases.push(phrase)
        for (const r of c.requirements) if (!n.requirements.includes(r)) n.requirements.push(r)
      }
    }
    if (withChildren)
      for (const [cid, c] of Object.entries(graph.entities)) {
        if (c.parent !== id) continue
        const n = at(cid)
        if (!n.phrases.some((p) => p === 'is made of')) n.phrases.push('contains')
      }
    return [...by.values()].slice(0, MAX)
  }, [graph, id, withChildren])

  if (neighbors.length === 0) return <p className="muted">No connections found in the documents.</p>
  const me = graph.entities[id]
  const cx = W / 2
  const cy = H / 2
  const sel = neighbors.find((n) => n.id === picked) ?? null
  return (
    <div className="connections">
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" role="group" aria-label={`${me?.name ?? id} and what it connects to`}>
        {neighbors.map((n, i) => {
          const a = (i / neighbors.length) * Math.PI * 2 - Math.PI / 2
          const x = cx + Math.cos(a) * RX
          const y = cy + Math.sin(a) * RY
          const lx = cx + (x - cx) * 0.52
          const ly = cy + (y - cy) * 0.52
          const on = picked === n.id
          const choose = () => setPicked(on ? null : n.id)
          return (
            <g
              key={n.id}
              className={`cn-n${on ? ' on' : ''}`}
              onClick={choose}
              onKeyDown={(ev) => (ev.key === 'Enter' || ev.key === ' ') && (ev.preventDefault(), choose())}
              tabIndex={0}
              role="button"
              aria-pressed={on}
              aria-label={`${me?.name} ${n.phrases.join(', ')} ${n.name}`}
            >
              <line x1={cx} y1={cy} x2={x} y2={y} className="cn-edge" />
              <text x={lx} y={ly} className="cn-rel" textAnchor="middle" dominantBaseline="middle">
                {n.phrases.join(', ')}
              </text>
              <rect x={x - 62} y={y - 15} width={124} height={30} rx={6} className="cn-box" />
              <text x={x} y={y} className="cn-name" textAnchor="middle" dominantBaseline="middle">
                {shorten(n.name, 20)}
              </text>
            </g>
          )
        })}
        <rect x={cx - 70} y={cy - 17} width={140} height={34} rx={7} className="cn-me" />
        <text x={cx} y={cy} className="cn-name me" textAnchor="middle" dominantBaseline="middle">
          {shorten(me?.name ?? id, 22)}
        </text>
      </svg>
      {sel ? (
        <div className="cn-why">
          <p className="cn-why-head">
            {me?.name} {sel.phrases.join(', ')} <NameLink id={sel.id} />
          </p>
          {sel.requirements.length > 0 ? (
            <div className="req-list">
              {sel.requirements.map((r) => (
                <div key={r} className="req-row">
                  <NameLink id={r} />
                </div>
              ))}
            </div>
          ) : (
            <p className="muted">The containment is stated by the documents' structure, not by one sentence.</p>
          )}
          <p>
            <Link to={`/model/entities/${encodeURIComponent(sel.id)}`}>Open {sel.name} →</Link>
          </p>
        </div>
      ) : (
        <p className="muted cn-hint">Select a box to see the sentences that connect them.</p>
      )}
    </div>
  )
}
