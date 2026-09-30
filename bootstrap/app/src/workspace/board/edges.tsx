// The board's edges: a straight line between two nodes' borders, the verb at its
// middle, so a relationship reads the same whichever side the nodes sit on.
import { memo } from 'react'
import { BaseEdge, EdgeLabelRenderer, useInternalNode, useStore, type Edge, type EdgeProps, type InternalNode } from '@xyflow/react'
import type { EdgeKind } from './model'

export type BoardEdgeData = { kind: EdgeKind | 'lift'; label?: string; scale: number }
export type BoardEdgeType = Edge<BoardEdgeData, 'floating'>

function box(n: InternalNode) {
  const w = n.measured?.width ?? n.width ?? 0
  const h = n.measured?.height ?? n.height ?? 0
  const { x, y } = n.internals.positionAbsolute
  return { x, y, w, h, cx: x + w / 2, cy: y + h / 2 }
}

// Where the line from a box's center toward (tx, ty) leaves the box.
function border(b: ReturnType<typeof box>, tx: number, ty: number) {
  const dx = tx - b.cx
  const dy = ty - b.cy
  if (dx === 0 && dy === 0) return { x: b.cx, y: b.cy }
  const sx = dx === 0 ? Infinity : b.w / 2 / Math.abs(dx)
  const sy = dy === 0 ? Infinity : b.h / 2 / Math.abs(dy)
  const s = Math.min(sx, sy)
  return { x: b.cx + dx * s, y: b.cy + dy * s }
}

export const FloatingEdge = memo(function FloatingEdge({ id, source, target, data, markerEnd }: EdgeProps<BoardEdgeType>) {
  const s = useInternalNode(source)
  const t = useInternalNode(target)
  const zoom = useStore((st) => (data?.kind === 'lift' ? st.transform[2] : 1))
  if (!s || !t) return null
  const a = box(s)
  const b = box(t)
  const p = border(a, b.cx, b.cy)
  const q = border(b, a.cx, a.cy)
  const path = `M ${p.x},${p.y} L ${q.x},${q.y}`
  const scale = data?.kind === 'lift' ? 1 / Math.max(zoom, 0.01) : (data?.scale ?? 1)
  return (
    <>
      <BaseEdge id={id} path={path} markerEnd={markerEnd} className={`be be-${data?.kind ?? 'rel'}`} style={data?.kind === 'lift' ? { strokeWidth: 2.5 * scale } : undefined} />
      {data?.label && (
        <EdgeLabelRenderer>
          <div
            className={`be-label be-label-${data.kind}`}
            style={{
              transform: `translate(-50%, -50%) translate(${(p.x + q.x) / 2}px, ${(p.y + q.y) / 2}px)`,
              fontSize: 11 * scale,
              padding: `${1 * scale}px ${5 * scale}px`,
            }}
          >
            {data.label}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  )
})

export const edgeTypes = { floating: FloatingEdge }
