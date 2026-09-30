// The board's links: a straight line between two nodes' borders, so a portal and
// the linkage (links.tsx) sit on the line the eye follows. The linkage draws in
// screen space over the board, not here.
import { memo } from 'react'
import { BaseEdge, useInternalNode, type Edge, type EdgeProps, type InternalNode } from '@xyflow/react'
import { borderPoint, type EdgeKind } from './model'

export type BoardEdgeData = { kind: EdgeKind; hot: boolean }
export type BoardEdgeType = Edge<BoardEdgeData, 'floating'>

function rect(n: InternalNode) {
  const { x, y } = n.internals.positionAbsolute
  return { x, y, w: n.measured?.width ?? n.width ?? 0, h: n.measured?.height ?? n.height ?? 0 }
}

export const FloatingEdge = memo(function FloatingEdge({ id, source, target, data, markerEnd, markerStart }: EdgeProps<BoardEdgeType>) {
  const s = useInternalNode(source)
  const t = useInternalNode(target)
  if (!s || !t) return null
  const a = rect(s)
  const b = rect(t)
  const p = borderPoint(a, { x: b.x + b.w / 2, y: b.y + b.h / 2 })
  const q = borderPoint(b, { x: a.x + a.w / 2, y: a.y + a.h / 2 })
  const path = `M ${p.x},${p.y} L ${q.x},${q.y}`
  return <BaseEdge id={id} path={path} markerEnd={markerEnd} markerStart={markerStart} className={`be be-${data?.kind ?? 'rel'}${data?.hot ? ' be-hot' : ''}`} />
})

export const edgeTypes = { floating: FloatingEdge }
