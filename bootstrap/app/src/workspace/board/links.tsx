// The links' portals and linkage (docs/frontends/gui.md#the-board), drawn in screen
// space over the board so they read at any zoom. Where a link leaves the view, a
// portal on it names the node at the far end, an arrow pointing the way; a click
// focuses that node. The linkage (the verb and the requirements behind the link,
// collapsed) sits centered on the link's visible part, between its portals.
import { memo, useMemo } from 'react'
import { useStore } from '@xyflow/react'
import { useGraph } from '../../lib/queries'
import { shorten } from '../../lib/present'
import { avoid, clampBox, fitMargin, linkOnScreen, sideOf, visiblePieces, type LayerEdge, type Level, type Point, type Rect } from './model'
import { useRuntime } from './runtime'

export interface NodeName {
  glyph: string
  name: string
}

// Keeps clear of the toolbar (top left) and the minimap (bottom right) by the
// view's inset.
const INSET = 10
const PORTAL_H = 26
const LABEL_H = 22
// A portal narrowed to its margin keeps at least this width; its full name shows
// on hover.
const MIN_W = 72

function portalWidth(name: string): number {
  return Math.min(220, 44 + name.length * 7)
}

// A portal or a linkage narrowed to a thin margin: the name wraps to two lines, the
// linkage keeps only its count.
const NARROW_H = 38

type Placed =
  | { kind: 'portal'; key: string; edge: string; node: string; box: Rect; angle: number; name: NodeName; narrow: boolean }
  | { kind: 'label'; key: string; edge: LayerEdge; box: Rect; narrow: boolean }

export const Links = memo(function Links({
  edges,
  rects,
  names,
  level,
  hotEdge,
  onGo,
  onOpenRequirement,
}: {
  edges: LayerEdge[]
  rects: Map<string, Rect>
  names: Map<string, NodeName>
  level: Level
  hotEdge: string | null
  onGo: (node: string) => void
  onOpenRequirement: (id: string, edge: LayerEdge) => void
}) {
  const [tx, ty, zoom] = useStore((s) => s.transform)
  const width = useStore((s) => s.width)
  const height = useStore((s) => s.height)
  const expanded = useRuntime((s) => s.expanded)
  const toggle = useRuntime((s) => s.toggleLink)
  const { data: g } = useGraph()

  const placed = useMemo(() => {
    const out: Placed[] = []
    if (width < 50 || height < 50) return out
    const view: Rect = { x: INSET, y: INSET, w: width - 2 * INSET, h: height - 2 * INSET }
    const screen = (r: Rect): Rect => ({ x: r.x * zoom + tx, y: r.y * zoom + ty, w: r.w * zoom, h: r.h * zoom })
    const boxes: Rect[] = []
    // The windows on screen: a link under one is hidden there, and a portal or a
    // linkage keeps to the margin beside them.
    const covers = [...rects.entries()]
      .filter(([id]) => id.startsWith('w:'))
      .map(([id, r]) => [id, screen(r)] as [string, Rect])
      .filter(([, r]) => r.x < view.x + view.w && view.x < r.x + r.w && r.y < view.y + view.h && view.y < r.y + r.h)
    const portalSides = new Set<string>()
    const labels: { edge: LayerEdge; at: Point }[] = []
    for (const e of edges) {
      const a = rects.get(e.source)
      const b = rects.get(e.target)
      if (!a || !b) continue
      const link = linkOnScreen(screen(a), screen(b), view)
      if (!link) continue
      const over = covers.filter(([id]) => id !== e.source && id !== e.target).map(([, r]) => r)
      const pieces = visiblePieces(link.from, link.to, over)
      const len = Math.hypot(link.to.x - link.from.x, link.to.y - link.from.y)
      const seen = pieces.filter(([s0, s1]) => (s1 - s0) * len >= 12)
      if (seen.length === 0) continue
      const at = (t: number): Point => ({ x: link.from.x + (link.to.x - link.from.x) * t, y: link.from.y + (link.to.y - link.from.y) * t })
      // A portal where the link leaves the view, when that stretch of it shows.
      const ends: [boolean, string, Point, number][] = [
        [link.sourceOut && seen[0][0] < 0.001, e.source, link.from, link.angle + Math.PI],
        [link.targetOut && seen[seen.length - 1][1] > 0.999, e.target, link.to, link.angle],
      ]
      for (const [show, node, point, angle] of ends) {
        if (!show) continue
        const name = names.get(node) ?? { glyph: '▢', name: node }
        let box = clampBox(point, portalWidth(name.name), PORTAL_H, view)
        // One portal per node on each side of the view.
        const side = `${node}|${sideOf(box, view)}`
        if (portalSides.has(side)) continue
        portalSides.add(side)
        const fitted = fitMargin(box, covers.map(([, r]) => r), view, MIN_W)
        const narrow = fitted.w < box.w
        box = narrow ? { ...fitted, ...clampBox({ x: fitted.x + fitted.w / 2, y: point.y }, fitted.w, NARROW_H, view) } : fitted
        box = avoid(box, boxes, view)
        boxes.push(box)
        out.push({ kind: 'portal', key: `p:${e.id}:${node}`, edge: e.id, node, box, angle, name, narrow })
      }
      if (level !== 'title' && (e.reqs.length > 0 || e.kind === 'rel')) {
        const [s0, s1] = seen.reduce((m, p) => (p[1] - p[0] > m[1] - m[0] ? p : m))
        labels.push({ edge: e, at: at((s0 + s1) / 2) })
      }
    }
    for (const { edge, at } of labels) {
      const text = edge.label + (edge.reqs.length ? ` · ${edge.reqs.length}` : '')
      const open = !!expanded[edge.id] && edge.reqs.length > 0
      const w = open ? 300 : Math.min(240, 30 + text.length * 6.6)
      const h = open ? Math.min(260, 30 + Math.min(edge.reqs.length, 6) * 36) : LABEL_H
      let box = clampBox(at, w, h, view)
      let narrow = false
      if (!open) {
        const fitted = fitMargin(box, covers.map(([, r]) => r), view, 40)
        narrow = fitted.w < box.w
        box = narrow ? { ...fitted, w: Math.min(fitted.w, 48), x: fitted.x + fitted.w - Math.min(fitted.w, 48) } : fitted
        if (narrow && sideOf(fitted, view) === 'left') box = { ...box, x: fitted.x }
      }
      box = avoid(box, boxes, view)
      boxes.push(box)
      out.push({ kind: 'label', key: `l:${edge.id}`, edge, box, narrow })
    }
    return out
  }, [edges, rects, names, level, tx, ty, zoom, width, height, expanded])

  return (
    <div className="board-links">
      {placed.map((p) =>
        p.kind === 'portal' ? (
          <button
            key={p.key}
            className={`bl-portal${p.edge === hotEdge ? ' hot' : ''}${p.narrow ? ' narrow' : ''}`}
            style={{ ...(p.box.x + p.box.w >= width - INSET - 1 ? { right: width - p.box.x - p.box.w } : { left: p.box.x }), top: p.box.y, width: p.box.w, height: p.box.h }}
            onClick={() => onGo(p.node)}
            title={`Go to ${p.name.name}`}
            data-node={p.node}
          >
            <span className="bl-arrow" style={{ transform: `rotate(${p.angle}rad)` }} aria-hidden>
              ➜
            </span>
            <span className="bl-glyph">{p.name.glyph}</span>
            <span className="bl-name">{p.name.name}</span>
          </button>
        ) : (
          <div
            key={p.key}
            className={`bl-label bl-${p.edge.kind}${p.edge.id === hotEdge ? ' hot' : ''}${expanded[p.edge.id] ? ' open' : ''}${p.narrow ? ' narrow' : ''}`}
            style={{ left: p.box.x, top: p.box.y, width: p.box.w, maxHeight: p.box.h }}
            data-edge={p.edge.id}
          >
            <button
              className="bl-head"
              disabled={p.edge.reqs.length === 0}
              onClick={() => toggle(p.edge.id)}
              aria-expanded={p.edge.reqs.length ? !!expanded[p.edge.id] : undefined}
              title={`${p.edge.label}${p.edge.reqs.length ? `: ${p.edge.reqs.length} ${p.edge.reqs.length === 1 ? 'requirement' : 'requirements'} behind this link` : ''}`}
            >
              {!p.narrow && <span className="bl-verb">{p.edge.label}</span>}
              {p.edge.reqs.length > 0 && (
                <span className="bl-count">
                  {p.edge.reqs.length} {expanded[p.edge.id] ? '▾' : '▸'}
                </span>
              )}
            </button>
            {expanded[p.edge.id] && p.edge.reqs.length > 0 && (
              <ul className="bl-reqs">
                {p.edge.reqs.map((id) => (
                  <li key={id}>
                    <button onClick={() => onOpenRequirement(id, p.edge)} title="Open the requirement beside the link">
                      {shorten(g?.requirements[id]?.statement ?? id, 140)}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        ),
      )}
    </div>
  )
})
