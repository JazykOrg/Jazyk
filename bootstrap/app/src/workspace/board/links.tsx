// The links' portals and linkage (docs/frontends/gui.md#the-board), drawn in screen
// space over the board so they read at any zoom. Where a link leaves the view, a
// portal on it names the node at the far end, an arrow pointing the way; a click
// focuses that node. The linkage (the verb and the requirements behind the link,
// collapsed) sits centered on the link's visible part, between its portals; with one
// end out of view it folds into that end's portal.
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

// Keeps clear of the view's edges by this much.
const INSET = 10
const PORTAL_H = 26
const LABEL_H = 22
// A portal narrowed to its margin keeps at least this width; its full name shows
// on hover.
const MIN_W = 72
// A portal or a linkage narrowed to a thin margin: the name wraps to two lines, the
// linkage keeps only its count.
const NARROW_H = 38
// The width a folded-in count adds to a portal.
const COUNT_W = 34
const PANEL_W = 300

function portalWidth(name: string, count: boolean): number {
  return Math.min(220 + (count ? COUNT_W : 0), 44 + name.length * 7 + (count ? COUNT_W : 0))
}

function panelHeight(n: number): number {
  return Math.min(260, 30 + Math.min(n, 6) * 36)
}

interface PortalPlaced {
  kind: 'portal'
  key: string
  edge: string
  node: string
  box: Rect
  angle: number
  name: NodeName
  narrow: boolean
  // The link's linkage, folded in when this is the link's only portal.
  linkage?: LayerEdge
}

type Placed =
  | PortalPlaced
  | { kind: 'label'; key: string; edge: LayerEdge; box: Rect; narrow: boolean }
  | { kind: 'panel'; key: string; edge: LayerEdge; box: Rect }

function Count({ edge, open, onToggle }: { edge: LayerEdge; open: boolean; onToggle: () => void }) {
  return (
    <button
      className="bl-count"
      onClick={onToggle}
      aria-expanded={open}
      title={`${edge.label}: ${edge.reqs.length} ${edge.reqs.length === 1 ? 'requirement' : 'requirements'} behind this link`}
    >
      {edge.reqs.length} {open ? '▾' : '▸'}
    </button>
  )
}

function Requirements({ edge, onOpen }: { edge: LayerEdge; onOpen: (id: string, edge: LayerEdge) => void }) {
  const { data: g } = useGraph()
  return (
    <ul className="bl-reqs">
      {edge.reqs.map((id) => (
        <li key={id}>
          <button onClick={() => onOpen(id, edge)} title="Open the requirement beside the link">
            {shorten(g?.requirements[id]?.statement ?? id, 140)}
          </button>
        </li>
      ))}
    </ul>
  )
}

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

  const placed = useMemo(() => {
    const out: Placed[] = []
    if (width < 50 || height < 50) return out
    const view: Rect = { x: INSET, y: INSET, w: width - 2 * INSET, h: height - 2 * INSET }
    const screen = (r: Rect): Rect => ({ x: r.x * zoom + tx, y: r.y * zoom + ty, w: r.w * zoom, h: r.h * zoom })
    const boxes: Rect[] = []
    // The windows on screen: a link under one is hidden there, and a portal or a
    // linkage keeps to the margin beside them.
    const covers = [...rects.entries()]
      .map(([id, r]) => [id, screen(r)] as [string, Rect])
      .filter(([, r]) => r.x < view.x + view.w && view.x < r.x + r.w && r.y < view.y + view.h && view.y < r.y + r.h)
    const coverRects = covers.map(([, r]) => r)
    const portalSides = new Set<string>()
    const labels: { edge: LayerEdge; at: Point }[] = []
    const showLinkage = (e: LayerEdge) => level !== 'title' && (e.reqs.length > 0 || e.kind === 'rel')
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
      // One end out of view: its portal carries the linkage (a lone portal).
      const lone = link.sourceOut !== link.targetOut && showLinkage(e)
      let folded = false
      for (const [show, node, point, angle] of ends) {
        if (!show) continue
        const name = names.get(node) ?? { glyph: '▢', name: node }
        const withCount = lone && e.reqs.length > 0
        let box = clampBox(point, portalWidth(name.name, withCount), PORTAL_H, view)
        // One portal per node on each side of the view.
        const side = `${node}|${sideOf(box, view)}`
        if (portalSides.has(side)) continue
        portalSides.add(side)
        // Narrowed, the name wraps and a folded count stacks under it.
        const fitted = fitMargin(box, coverRects, view, MIN_W)
        const narrow = fitted.w < box.w
        box = narrow ? { ...fitted, ...clampBox({ x: fitted.x + fitted.w / 2, y: point.y }, fitted.w, NARROW_H + (withCount ? 20 : 0), view) } : fitted
        box = avoid(box, boxes, view)
        boxes.push(box)
        const p: PortalPlaced = { kind: 'portal', key: `p:${e.id}:${node}`, edge: e.id, node, box, angle, name, narrow }
        if (lone) {
          p.linkage = e
          folded = true
        }
        out.push(p)
      }
      if (showLinkage(e) && !folded) {
        const [s0, s1] = seen.reduce((m, p) => (p[1] - p[0] > m[1] - m[0] ? p : m))
        labels.push({ edge: e, at: at((s0 + s1) / 2) })
      }
    }
    for (const { edge, at } of labels) {
      const text = edge.label + (edge.reqs.length ? ` · ${edge.reqs.length}` : '')
      const open = expanded === edge.id && edge.reqs.length > 0
      const w = open ? PANEL_W : Math.min(240, 30 + text.length * 6.6)
      const h = open ? panelHeight(edge.reqs.length) : LABEL_H
      let box = clampBox(at, w, h, view)
      let narrow = false
      if (!open) {
        const fitted = fitMargin(box, coverRects, view, 40)
        narrow = fitted.w < box.w
        const nw = Math.min(fitted.w, 48)
        box = narrow ? { ...fitted, w: nw, x: sideOf(fitted, view) === 'left' ? fitted.x : fitted.x + fitted.w - nw } : fitted
      }
      box = avoid(box, boxes, view)
      boxes.push(box)
      out.push({ kind: 'label', key: `l:${edge.id}`, edge, box, narrow })
    }
    // A folded linkage expands as a panel under its portal.
    for (const p of out) {
      if (p.kind !== 'portal' || !p.linkage || expanded !== p.linkage.id || p.linkage.reqs.length === 0) continue
      const h = panelHeight(p.linkage.reqs.length)
      const box = clampBox({ x: p.box.x + p.box.w / 2, y: p.box.y + p.box.h + 4 + h / 2 }, PANEL_W, h, view)
      out.push({ kind: 'panel', key: `r:${p.linkage.id}`, edge: p.linkage, box })
      break
    }
    return out
  }, [edges, rects, names, level, tx, ty, zoom, width, height, expanded])

  return (
    <div className="board-links">
      {placed.map((p) => {
        if (p.kind === 'portal') {
          const right = p.box.x + p.box.w >= width - INSET - 1
          return (
            <div
              key={p.key}
              className={`bl-portal${p.edge === hotEdge ? ' hot' : ''}${p.narrow ? ' narrow' : ''}`}
              style={{ ...(right ? { right: width - p.box.x - p.box.w } : { left: p.box.x }), top: p.box.y, width: p.box.w, height: p.box.h }}
              data-node={p.node}
            >
              <button className="bl-go" onClick={() => onGo(p.node)} title={`Go to ${p.name.name}`}>
                <span className="bl-arrow" style={{ transform: `rotate(${p.angle}rad)` }} aria-hidden>
                  ➜
                </span>
                <span className="bl-glyph">{p.name.glyph}</span>
                <span className="bl-name">{p.name.name}</span>
              </button>
              {p.linkage && p.linkage.reqs.length > 0 && <Count edge={p.linkage} open={expanded === p.linkage.id} onToggle={() => toggle(p.linkage!.id)} />}
            </div>
          )
        }
        if (p.kind === 'panel') {
          return (
            <div key={p.key} className="bl-label bl-panel open" style={{ left: p.box.x, top: p.box.y, width: p.box.w, maxHeight: p.box.h }} data-edge={p.edge.id}>
              <div className="bl-head">
                <span className="bl-verb">{p.edge.label}</span>
              </div>
              <Requirements edge={p.edge} onOpen={onOpenRequirement} />
            </div>
          )
        }
        const open = expanded === p.edge.id
        return (
          <div
            key={p.key}
            className={`bl-label bl-${p.edge.kind}${p.edge.id === hotEdge ? ' hot' : ''}${open ? ' open' : ''}${p.narrow ? ' narrow' : ''}`}
            style={{ left: p.box.x, top: p.box.y, width: p.box.w, maxHeight: p.box.h }}
            data-edge={p.edge.id}
          >
            <button
              className="bl-head"
              disabled={p.edge.reqs.length === 0}
              onClick={() => toggle(p.edge.id)}
              aria-expanded={p.edge.reqs.length ? open : undefined}
              title={`${p.edge.label}${p.edge.reqs.length ? `: ${p.edge.reqs.length} ${p.edge.reqs.length === 1 ? 'requirement' : 'requirements'} behind this link` : ''}`}
            >
              {!p.narrow && <span className="bl-verb">{p.edge.label}</span>}
              {p.edge.reqs.length > 0 && (
                <span className="bl-count">
                  {p.edge.reqs.length} {open ? '▾' : '▸'}
                </span>
              )}
            </button>
            {open && p.edge.reqs.length > 0 && <Requirements edge={p.edge} onOpen={onOpenRequirement} />}
          </div>
        )
      })}
    </div>
  )
})
