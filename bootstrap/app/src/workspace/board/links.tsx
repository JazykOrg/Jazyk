// The links' portals and linkage (docs/frontends/gui.md#the-board), drawn in screen
// space over the board so they read at any zoom. Where a link leaves the view, a
// portal on it names the node at the far end, an arrow pointing the way; a click
// focuses that node. The linkage card (card.tsx) sits centered on the link's visible
// part, between its portals; with one end out of view it folds into that end's
// portal. Its requirement lines show only near 100 percent.
import { memo, useMemo } from 'react'
import { EdgeLabelRenderer, useStore } from '@xyflow/react'
import { useGraph } from '../../lib/queries'
import { avoid, clampBox, fitMargin, linkOnScreen, sideOf, visiblePieces, type LayerEdge, type Level, type Point, type Rect } from './model'
import { useRuntime } from './runtime'
import LinkCard, { cardBox } from './card'

export interface NodeName {
  glyph: string
  name: string
}

// Keeps clear of the view's edges by this much.
const INSET = 10
const PORTAL_H = 26
// A portal with no requirements narrowed to its margin keeps at least this width;
// in a thin margin its name wraps to two lines.
const MIN_W = 72
const NARROW_H = 38

function portalWidth(name: string): number {
  return Math.min(220, 44 + name.length * 7)
}

function verbWidth(label: string): number {
  return Math.min(200, 24 + label.length * 6.6)
}

type Placed =
  | { kind: 'portal'; key: string; edge: LayerEdge; node: string; box: Rect; angle: number; name: NodeName; narrow: boolean; card: boolean }
  | { kind: 'label'; key: string; edge: LayerEdge; box: Rect }

export function PortalHead({ name, angle, onGo }: { name: NodeName; angle: number; onGo: () => void }) {
  return (
    <button className="bl-go" onClick={onGo} title={`Go to ${name.name}`}>
      <span className="bl-arrow" style={{ transform: `rotate(${angle}rad)` }} aria-hidden>
        ➜
      </span>
      <span className="bl-glyph">{name.glyph}</span>
      <span className="bl-name">{name.name}</span>
    </button>
  )
}

export const Links = memo(function Links({
  edges,
  rects,
  names,
  level,
  hotEdge,
  hotReqs,
  onGo,
  onReveal,
  onOpenRequirement,
}: {
  edges: LayerEdge[]
  rects: Map<string, Rect>
  names: Map<string, NodeName>
  level: Level
  hotEdge: string | null
  hotReqs: Set<string>
  onGo: (node: string) => void
  onReveal: (rid: string) => void
  onOpenRequirement: (rid: string, edge: LayerEdge) => void
}) {
  const [tx, ty, zoom] = useStore((s) => s.transform)
  const width = useStore((s) => s.width)
  const height = useStore((s) => s.height)
  const expanded = useRuntime((s) => s.expanded)
  const { data: g } = useGraph()
  const texts = (e: LayerEdge) => e.reqs.map((id) => g?.requirements[id]?.statement ?? id)
  // The requirement lines show only near 100 percent; farther out a link carries
  // its verb alone.
  const withReqs = level === 'live'

  const placed = useMemo(() => {
    const out: Placed[] = []
    if (width < 50 || height < 50) return out
    const view: Rect = { x: INSET, y: INSET, w: width - 2 * INSET, h: height - 2 * INSET }
    const screen = (r: Rect): Rect => ({ x: r.x * zoom + tx, y: r.y * zoom + ty, w: r.w * zoom, h: r.h * zoom })
    const boxes: Rect[] = []
    // The windows on screen: a link under one is hidden there, and a portal or a
    // linkage keeps to the margin beside them where it fits.
    const covers = [...rects.entries()]
      .map(([id, r]) => [id, screen(r)] as [string, Rect])
      .filter(([, r]) => r.x < view.x + view.w && view.x < r.x + r.w && r.y < view.y + view.h && view.y < r.y + r.h)
    const coverRects = covers.map(([, r]) => r)
    const portalSides = new Set<string>()
    const labels: { edge: LayerEdge; at: Point }[] = []
    const reqsOf = (e: LayerEdge) => (withReqs ? e.reqs.length : 0)
    const showLinkage = (e: LayerEdge) => level !== 'title' && (reqsOf(e) > 0 || e.kind === 'rel')
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
      const ends: [boolean, string, Point, number][] = [
        [link.sourceOut && seen[0][0] < 0.001, e.source, link.from, link.angle + Math.PI],
        [link.targetOut && seen[seen.length - 1][1] > 0.999, e.target, link.to, link.angle],
      ]
      // One end out of view: its portal carries the linkage.
      const lone = link.sourceOut !== link.targetOut
      let folded = false
      for (const [show, node, point, angle] of ends) {
        if (!show) continue
        const name = names.get(node) ?? { glyph: '▢', name: node }
        const card = lone && reqsOf(e) > 0
        const side = sideOf(clampBox(point, 10, 10, view), view)
        // One portal per node on each side of the view.
        if (portalSides.has(`${node}|${side}`)) continue
        portalSides.add(`${node}|${side}`)
        let box: Rect
        let narrow = false
        if (card) {
          const b = cardBox(texts(e), expanded === e.id)
          box = clampBox(point, b.w, b.h, view)
        } else {
          box = clampBox(point, portalWidth(name.name), PORTAL_H, view)
          const fitted = fitMargin(box, coverRects, view, MIN_W)
          narrow = fitted.w < box.w
          box = narrow ? { ...fitted, ...clampBox({ x: fitted.x + fitted.w / 2, y: point.y }, fitted.w, NARROW_H, view) } : fitted
        }
        box = avoid(box, boxes, view)
        boxes.push(box)
        out.push({ kind: 'portal', key: `p:${e.id}:${node}`, edge: e, node, box, angle, name, narrow, card })
        if (lone) folded = true
      }
      if (showLinkage(e) && !folded) {
        const [s0, s1] = seen.reduce((m, p) => (p[1] - p[0] > m[1] - m[0] ? p : m))
        labels.push({ edge: e, at: at((s0 + s1) / 2) })
      }
    }
    for (const { edge, at } of labels) {
      const n = reqsOf(edge)
      const b = cardBox(texts(edge), expanded === edge.id)
      let box = n > 0 ? clampBox(at, b.w, b.h, view) : clampBox(at, verbWidth(edge.label), 22, view)
      if (n === 0) box = fitMargin(box, coverRects, view, 40)
      box = avoid(box, boxes, view)
      boxes.push(box)
      out.push({ kind: 'label', key: `l:${edge.id}`, edge, box })
    }
    return out
  }, [edges, rects, names, level, withReqs, tx, ty, zoom, width, height, expanded, g])

  // Drawn in the viewport's edge-label layer, beneath the windows, with the
  // viewport's transform undone so the boxes stay in screen pixels.
  return (
    <EdgeLabelRenderer>
    <div
      className="board-links"
      style={{ width, height, transform: `translate(${-tx / zoom}px, ${-ty / zoom}px) scale(${1 / zoom})` }}
    >
      {placed.map((p) => {
        const hot = p.edge.id === hotEdge
        if (p.kind === 'portal') {
          const right = p.box.x + p.box.w >= width - INSET - 1
          const pos = { ...(right ? { right: width - p.box.x - p.box.w } : { left: p.box.x }), top: p.box.y, width: p.box.w }
          const head = <PortalHead name={p.name} angle={p.angle} onGo={() => onGo(p.node)} />
          if (p.card) {
            return (
              <div key={p.key} className={`bl-portal lc nodrag nopan${hot ? ' hot' : ''}`} style={{ ...pos, maxHeight: p.box.h }} data-node={p.node} data-edge={p.edge.id}>
                <LinkCard id={p.edge.id} head={head} reqs={p.edge.reqs} hotReqs={hotReqs} onReveal={onReveal} onOpenRequirement={(rid) => onOpenRequirement(rid, p.edge)} />
              </div>
            )
          }
          return (
            <div key={p.key} className={`bl-portal nodrag nopan${hot ? ' hot' : ''}${p.narrow ? ' narrow' : ''}`} style={{ ...pos, height: p.box.h }} data-node={p.node}>
              {head}
            </div>
          )
        }
        const n = withReqs ? p.edge.reqs.length : 0
        return (
          <div
            key={p.key}
            className={`bl-label nodrag nopan${n > 0 ? ' lc' : ''} bl-${p.edge.kind}${hot ? ' hot' : ''}`}
            style={{ left: p.box.x, top: p.box.y, width: p.box.w, maxHeight: p.box.h }}
            data-edge={p.edge.id}
          >
            {n > 0 ? (
              <LinkCard id={p.edge.id} head={<span className="bl-verb">{p.edge.label}</span>} reqs={p.edge.reqs} hotReqs={hotReqs} onReveal={onReveal} onOpenRequirement={(rid) => onOpenRequirement(rid, p.edge)} />
            ) : (
              <span className="bl-verb" title={p.edge.label}>
                {p.edge.label}
              </span>
            )}
          </div>
        )
      })}
    </div>
    </EdgeLabelRenderer>
  )
})
