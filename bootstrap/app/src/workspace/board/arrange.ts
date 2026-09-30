// `Arrange`: the windows and the chips laid out by the graph, layered left to
// right so relations flow one way (plans/workspace.md#board). Loaded on first use.
import dagre from '@dagrejs/dagre'
import type { Point, Rect } from './model'

export interface ArrangeInput {
  nodes: { id: string; w: number; h: number }[]
  edges: { source: string; target: string; weight?: number }[]
}

export function arrange(input: ArrangeInput): Map<string, Point> {
  const g = new dagre.graphlib.Graph({ multigraph: true })
  g.setGraph({ rankdir: 'LR', nodesep: 48, ranksep: 140, marginx: 0, marginy: 0 })
  g.setDefaultEdgeLabel(() => ({}))
  const ids = new Set(input.nodes.map((n) => n.id))
  for (const n of input.nodes) g.setNode(n.id, { width: n.w, height: n.h })
  input.edges.forEach((e, i) => {
    if (ids.has(e.source) && ids.has(e.target) && e.source !== e.target) g.setEdge(e.source, e.target, { weight: e.weight ?? 1 }, `e${i}`)
  })
  dagre.layout(g)
  const out = new Map<string, Point>()
  for (const n of input.nodes) {
    const p = g.node(n.id) as { x: number; y: number } | undefined
    if (p) out.set(n.id, { x: Math.round(p.x - n.w / 2), y: Math.round(p.y - n.h / 2) })
  }
  return out
}

export type { Rect }
