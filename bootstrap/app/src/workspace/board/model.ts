// The board's pure logic (docs/frontends/gui.md#the-board, plans/workspace.md#board):
// where a window goes and how big, the links between the open windows with the
// requirements behind each, the focused window's fan of unopened entities, the zoom
// levels, the focus zoom, and where a link's portals and linkage sit on screen. No React here, so the
// rules are unit tested.
import type { Graph } from '../../lib/api'
import { relationPhrase } from '../../lib/present'
import { itemKind, itemTitle, type Item } from '../items'
import { itemKey } from '../paths'

export interface Rect {
  x: number
  y: number
  w: number
  h: number
}

export interface Point {
  x: number
  y: number
}

// What the board remembers per project: every window's rect by its item's page
// (the path without its query), the fan's toggle, and the viewport.
export interface Saved {
  windows: Record<string, Rect>
  graph: boolean
  viewport?: { x: number; y: number; zoom: number }
}

// A page window's size when the board's size is not known yet; otherwise it opens
// at FOCUS of the board (pageSize).
export const WIN_W = 760
export const WIN_H = 620
// An entity window is a card, not a page.
export const ENTITY_W = 440
export const ENTITY_H = 320
// The share of the board a focused window fills; the rest is its links' margin.
export const FOCUS = 0.8
// The most a focus zooms in: a page reads at 100 percent, a card may grow.
export const FOCUS_MAX_PAGE = 1
export const FOCUS_MAX_CARD = 1.25
export const WIN_MIN_W = 320
export const WIN_MIN_H = 220
// The horizontal gap between two windows side by side: out of view when one is
// focused, so the other shows as a portal.
export const WIN_GAP = 320
// The windows live at once (plans/workspace.md#cost).
export const LIVE_MAX = 6

export type Level = 'live' | 'still' | 'card' | 'title'

// The zoom levels of plans/workspace.md#zoom-levels.
export function levelOf(zoom: number): Level {
  if (zoom >= 0.8) return 'live'
  if (zoom >= 0.4) return 'still'
  if (zoom >= 0.15) return 'card'
  return 'title'
}

// The zoom `Escape` steps out to from a level.
export function zoomOut(level: Level): number | null {
  switch (level) {
    case 'live':
      return 0.6
    case 'still':
      return 0.27
    case 'card':
      return 0.1
    default:
      return null
  }
}

export interface Size {
  w: number
  h: number
}

// A new window's size: an entity is a card; any other page opens at FOCUS of the
// board, so it is focused at 100 percent.
export function windowSize(kind: string, board: Size | null): Size {
  if (kind === 'entity') return { w: ENTITY_W, h: ENTITY_H }
  if (!board || board.w < 200 || board.h < 200) return { w: WIN_W, h: WIN_H }
  return { w: Math.max(WIN_MIN_W, Math.round(board.w * FOCUS)), h: Math.max(WIN_MIN_H, Math.round(board.h * FOCUS)) }
}

// The zoom a focus lands on: the window fills FOCUS of the board, a page never above
// 100 percent and a card never above FOCUS_MAX_CARD.
export function focusZoom(r: Rect, board: Size, kind: string): number {
  const cap = kind === 'entity' ? FOCUS_MAX_CARD : FOCUS_MAX_PAGE
  const z = Math.min((board.w * FOCUS) / Math.max(r.w, 1), (board.h * FOCUS) / Math.max(r.h, 1))
  return Math.max(0.05, Math.min(cap, z))
}

const storeKey = (project: string) => `jazyk.board:${project}`

export function emptySaved(): Saved {
  return { windows: {}, graph: true }
}

function finite(...n: unknown[]): boolean {
  return n.every((v) => typeof v === 'number' && Number.isFinite(v))
}

export function readSaved(project: string | null): Saved {
  const out = emptySaved()
  if (!project) return out
  try {
    const raw = localStorage.getItem(storeKey(project))
    if (!raw) return out
    const v = JSON.parse(raw) as Partial<Saved>
    for (const [k, r] of Object.entries(v.windows ?? {})) {
      if (r && finite(r.x, r.y, r.w, r.h)) out.windows[k] = { x: r.x, y: r.y, w: r.w, h: r.h }
    }
    if (typeof v.graph === 'boolean') out.graph = v.graph
    const vp = v.viewport
    if (vp && finite(vp.x, vp.y, vp.zoom) && vp.zoom > 0) out.viewport = { x: vp.x, y: vp.y, zoom: vp.zoom }
  } catch {
    // a corrupt entry reads as none
  }
  return out
}

export function writeSaved(project: string | null, saved: Saved) {
  if (!project) return
  try {
    localStorage.setItem(storeKey(project), JSON.stringify(saved))
  } catch {
    // not remembered
  }
}

function overlaps(a: Rect, b: Rect, margin: number): boolean {
  return a.x < b.x + b.w + margin && b.x < a.x + a.w + margin && a.y < b.y + b.h + margin && b.y < a.y + a.h + margin
}

// The first free slot in a column starting at (x, y), stepping down past the
// windows already there.
function freeSlot(x: number, y: number, w: number, h: number, taken: Rect[]): Rect {
  let r = { x, y, w, h }
  for (let i = 0; i < 40; i++) {
    const hit = taken.find((t) => overlaps(r, t, 24))
    if (!hit) return r
    r = { x, y: hit.y + hit.h + 60, w, h }
  }
  return r
}

// A window opened from another goes to its right, below any window already there.
export function placeBeside(anchor: Rect, taken: Rect[], w = WIN_W, h = WIN_H): Rect {
  return freeSlot(anchor.x + anchor.w + WIN_GAP, anchor.y, w, h, taken)
}

// A window opened from nowhere goes right of the rightmost window, level with it.
export function placeRight(taken: Rect[], w = WIN_W, h = WIN_H): Rect {
  if (taken.length === 0) return { x: 0, y: 0, w, h }
  const right = taken.reduce((a, b) => (b.x + b.w > a.x + a.w ? b : a))
  return freeSlot(right.x + right.w + WIN_GAP, right.y, w, h, taken)
}

// The id a node name resolves to through the merge redirects.
export function canonical(g: Graph | undefined, id: string): string {
  let cur = id
  for (let i = 0; i < 8 && g?.redirects?.[cur]; i++) cur = g.redirects[cur]
  return cur
}

function decodedTail(path: string, prefix: string): string {
  const p = path.split('?')[0]
  return p.startsWith(prefix) ? decodeURIComponent(p.slice(prefix.length)) : ''
}

// The document a `/docs/<path>` item shows.
export function docOf(path: string): string {
  return decodedTail(path, '/docs/')
}

// The graph node an item shows: an entity, a requirement, a diagram.
export function nodeOf(path: string): string {
  const p = path.split('?')[0]
  for (const prefix of ['/model/entities/', '/model/requirements/', '/model/diagrams/']) {
    if (p.startsWith(prefix)) return decodedTail(path, prefix)
  }
  return ''
}

// The document an entity is defined in: its quote's, else its first mention's.
export function definingDoc(g: Graph, id: string): string | undefined {
  const e = g.entities[id]
  return e?.provenance?.quote?.doc ?? e?.mentions?.[0]?.doc
}

// A window is its item's page: the path without the query (paths.ts).
export const windowId = (path: string) => `w:${itemKey(path)}`

// defines (a document to an entity it defines), mentions (a document to one it
// mentions), names (a requirement or a diagram to its entities), a relationship,
// and opened-from (a window to one opened from inside it).
export type EdgeKind = 'rel' | 'def' | 'men' | 'ref' | 'from'

export interface LayerEdge {
  id: string
  source: string
  target: string
  kind: EdgeKind
  // The verb the linkage shows.
  label: string
  // The relationship type, strongest of the pair's contributions.
  type?: string
  // A relationship with contributions both ways: arrowheads at both ends.
  both?: boolean
  // The requirements behind the link (docs/frontends/gui.md#the-board).
  reqs: string[]
}

export interface Layer {
  edges: LayerEdge[]
  // The window each open entity is drawn as.
  entityNode: Map<string, string>
}

// The types strongest first, so two contributions between one pair draw as one edge.
const STRENGTH = ['generalization', 'realization', 'composition', 'aggregation', 'association', 'dependency']
const strength = (t: string | undefined) => {
  const i = STRENGTH.indexOf(t ?? 'dependency')
  return i < 0 ? STRENGTH.length : i
}

const KIND_VERB: Record<Exclude<EdgeKind, 'rel'>, string> = {
  def: 'defines',
  men: 'mentions',
  ref: 'names',
  from: 'opened from',
}

export interface Brought {
  entity: string
  // The link to the window once the entity is open; an entity window's neighbours
  // link through their relationships instead.
  edge?: 'def' | 'men' | 'ref'
}

// The entities a window brings, the defined ones first, then the most mentioned.
export function windowEntities(g: Graph, item: Item): Brought[] {
  const kind = itemKind(item.path)
  const cap = <T,>(xs: T[]) => xs
  if (kind === 'doc') {
    const doc = docOf(item.path)
    const defined: string[] = []
    const mentioned: { id: string; n: number }[] = []
    for (const [id, e] of Object.entries(g.entities)) {
      const n = (e.mentions ?? []).filter((m) => m.doc === doc).length
      if (definingDoc(g, id) === doc) defined.push(id)
      else if (n > 0) mentioned.push({ id, n })
    }
    const byName = (a: string, b: string) => (g.entities[a]?.name ?? a).localeCompare(g.entities[b]?.name ?? b)
    defined.sort(byName)
    mentioned.sort((a, b) => b.n - a.n || byName(a.id, b.id))
    return cap([...defined.map((entity) => ({ entity, edge: 'def' as const })), ...mentioned.map((m) => ({ entity: m.id, edge: 'men' as const }))])
  }
  if (kind === 'entity') {
    const self = canonical(g, nodeOf(item.path))
    const seen = new Set<string>()
    for (const rel of Object.values(g.relationships)) {
      const members = rel.members.map((m) => canonical(g, m))
      if (!members.includes(self)) continue
      for (const m of members) if (m !== self && g.entities[m]) seen.add(m)
    }
    return cap([...seen].map((entity) => ({ entity })))
  }
  if (kind === 'requirement') {
    const r = g.requirements[canonical(g, nodeOf(item.path))]
    return cap((r?.entities ?? []).map((e) => canonical(g, e)).filter((e) => g.entities[e]).map((entity) => ({ entity, edge: 'ref' as const })))
  }
  if (kind === 'diagram') {
    const v = g.views[nodeOf(item.path)]
    return cap((v?.members ?? []).map((e) => canonical(g, e)).filter((e) => g.entities[e]).map((entity) => ({ entity, edge: 'ref' as const })))
  }
  return []
}

// What a node on the board stands for, for the requirements behind a link.
export type Subject = { doc: string } | { entity: string } | { requirement: string } | Record<string, never>

export function subjectOfPath(g: Graph, path: string): Subject {
  const kind = itemKind(path)
  if (kind === 'doc') return { doc: docOf(path) }
  if (kind === 'entity') return { entity: canonical(g, nodeOf(path)) }
  if (kind === 'requirement') return { requirement: canonical(g, nodeOf(path)) }
  return {}
}

// The document a requirement is quoted from.
function reqDoc(g: Graph, id: string): string | undefined {
  const r = g.requirements[id]
  return r?.source?.doc ?? r?.provenance?.quote?.doc
}

// The requirements behind a link between two subjects: a relationship's
// contributions both ways, the requirements a document quotes that name an entity,
// a requirement itself beside its entity or its document.
export function linkRequirements(g: Graph, a: Subject, b: Subject): string[] {
  const out = new Set<string>()
  const ent = (s: Subject) => ('entity' in s ? s.entity : null)
  const doc = (s: Subject) => ('doc' in s ? s.doc : null)
  const req = (s: Subject) => ('requirement' in s ? s.requirement : null)
  const [ea, eb] = [ent(a), ent(b)]
  if (ea && eb) {
    for (const rel of Object.values(g.relationships)) {
      for (const c of rel.contributions) {
        const x = canonical(g, c.a)
        const y = canonical(g, c.b)
        if ((x === ea && y === eb) || (x === eb && y === ea)) for (const r of c.requirements ?? []) out.add(canonical(g, r))
      }
    }
  }
  for (const [s, t] of [
    [a, b],
    [b, a],
  ] as const) {
    const d = doc(s)
    const e = ent(t)
    const r = req(s)
    if (d && e) {
      for (const [id, q] of Object.entries(g.requirements)) {
        if (reqDoc(g, id) === d && (q.entities ?? []).some((x) => canonical(g, x) === e)) out.add(id)
      }
    }
    if (r && g.requirements[r]) {
      if (e && (g.requirements[r].entities ?? []).some((x) => canonical(g, x) === e)) out.add(r)
      const dt = doc(t)
      if (dt && reqDoc(g, r) === dt) out.add(r)
    }
  }
  return [...out].filter((id) => g.requirements[id]).sort()
}

// The links between the open windows: a window to the open entities it brings, the
// relationships between open entities (one per pair, its verb the strongest type's),
// and a window to one opened from inside it. Every link carries the requirements
// behind it.
export function buildLayer(items: Item[], g: Graph | undefined): Layer {
  const entityNode = new Map<string, string>()
  const edges: LayerEdge[] = []
  const paths = new Set(items.map((i) => itemKey(i.path)))
  const subjectOfNode = new Map<string, Subject>()
  if (g) {
    for (const item of items) {
      subjectOfNode.set(windowId(item.path), subjectOfPath(g, item.path))
      if (itemKind(item.path) !== 'entity') continue
      const id = canonical(g, nodeOf(item.path))
      if (g.entities[id] && !entityNode.has(id)) entityNode.set(id, windowId(item.path))
    }
    for (const item of items) {
      for (const { entity, edge } of windowEntities(g, item)) {
        const target = entityNode.get(entity)
        if (!edge || !target || target === windowId(item.path)) continue
        edges.push({
          id: `${edge}:${itemKey(item.path)}->${entity}`,
          source: windowId(item.path),
          target,
          kind: edge,
          label: KIND_VERB[edge],
          reqs: linkRequirements(g, subjectOfPath(g, item.path), { entity }),
        })
      }
    }
    // One link per pair: the first direction seen, the strongest type's verb, and
    // arrowheads both ways when the pair has contributions both ways.
    const pairs = new Map<string, { a: string; b: string; type: string; ab: boolean; ba: boolean }>()
    for (const rel of Object.values(g.relationships)) {
      for (const c of rel.contributions) {
        const a = canonical(g, c.a)
        const b = canonical(g, c.b)
        if (a === b || !entityNode.has(a) || !entityNode.has(b)) continue
        const key = [a, b].sort().join('|')
        const type = c.type || 'dependency'
        const had = pairs.get(key)
        if (!had) {
          pairs.set(key, { a, b, type, ab: true, ba: false })
          continue
        }
        if (had.a === a) had.ab = true
        else had.ba = true
        if (strength(type) < strength(had.type)) had.type = type
      }
    }
    for (const p of pairs.values()) {
      edges.push({
        id: `rel:${p.a}->${p.b}`,
        source: entityNode.get(p.a)!,
        target: entityNode.get(p.b)!,
        kind: 'rel',
        type: p.type,
        both: p.ab && p.ba,
        label: relationPhrase(p.type, true),
        reqs: linkRequirements(g, { entity: p.a }, { entity: p.b }),
      })
    }
  }
  const linked = new Set(edges.map((e) => [e.source, e.target].sort().join('|')))
  for (const item of items) {
    if (!item.from || !paths.has(itemKey(item.from))) continue
    const s = windowId(item.from)
    const t = windowId(item.path)
    if (s === t || linked.has([s, t].sort().join('|'))) continue
    linked.add([s, t].sort().join('|'))
    const reqs = g ? linkRequirements(g, subjectOfNode.get(s) ?? {}, subjectOfNode.get(t) ?? {}) : []
    edges.push({ id: `from:${itemKey(item.from)}->${itemKey(item.path)}`, source: s, target: t, kind: 'from', label: KIND_VERB.from, reqs })
  }
  return { edges, entityNode }
}

// ---- references: the unopened entities the caret's text concerns, below it ----

// The entities a caret concerns, in order: the mention's entity, then the entities
// of the requirements whose quote holds the caret, each once, the open ones left out.
export function caretEntities(g: Graph, entity: string | null, reqs: string[], open: Set<string>): string[] {
  const out: string[] = []
  const add = (e: string) => {
    const c = canonical(g, e)
    if (g.entities[c] && !open.has(c) && !out.includes(c)) out.push(c)
  }
  if (entity) add(entity)
  for (const r of reqs) for (const e of g.requirements[canonical(g, r)]?.entities ?? []) add(e)
  return out
}

export const FAN_CHIP_H = 28
export const FAN_GAP = 10
// The drop from the window to the row: FAN_DROP when there is room, never less
// than FAN_MIN_DROP (below that, the row sits along the view's bottom edge).
export const FAN_DROP = 52
export const FAN_MIN_DROP = 18
export const FAN_MORE_W = 64

// A fan chip's width for its name.
export function fanChipWidth(name: string): number {
  return Math.max(90, Math.min(200, 44 + name.length * 7))
}

export interface FanSlot {
  // The index into the ordered entities, or -1 for the `+N` chip.
  index: number
  box: Rect
}

export interface FanLayout {
  // Lines leave `anchor` for each slot's top center, when there is room below.
  anchor: Point | null
  slots: FanSlot[]
  // How many entities the row leaves out (the `+N` chip lists them).
  more: number
}

// The fan's row under a window on screen: as many chips as fit, the first straight
// under the window's bottom center and the next alternating right and left, a `+N`
// chip at the right end for the rest. With no room below the window, the row sits
// along the view's bottom edge and draws no lines.
export function layoutFan(win: Rect, widths: number[], view: Rect, h = FAN_CHIP_H): FanLayout {
  const room = Math.min(view.w - 2 * FAN_GAP, Math.max(win.w, 420))
  const avail = view.y + view.h - 4 - (win.y + win.h) - h
  const below = avail >= FAN_MIN_DROP
  const drop = Math.min(FAN_DROP, avail)
  const taken: number[] = []
  let used = 0
  for (let i = 0; i < widths.length; i++) {
    const rest = widths.length - i - 1
    const need = used + (used ? FAN_GAP : 0) + widths[i] + (rest > 0 ? FAN_GAP + FAN_MORE_W : 0)
    if (need > room) break
    used += (used ? FAN_GAP : 0) + widths[i]
    taken.push(i)
  }
  const more = widths.length - taken.length
  // Center out: 0 in the middle, then 1 right, 2 left, 3 right, ...
  const row: number[] = []
  for (const i of taken) {
    if (i % 2 === 1) row.push(i)
    else row.unshift(i)
  }
  if (more > 0) row.push(-1)
  const w = (i: number) => (i < 0 ? FAN_MORE_W : widths[i])
  const total = row.reduce((a, i) => a + w(i), 0) + FAN_GAP * Math.max(row.length - 1, 0)
  const anchorX = Math.min(Math.max(win.x + win.w / 2, view.x), view.x + view.w)
  // The first chip sits under the anchor, the row kept inside the view.
  const first = row.indexOf(taken[0] ?? -1)
  const left = first > 0 ? row.slice(0, first).reduce((a, i) => a + w(i) + FAN_GAP, 0) : 0
  const want = first >= 0 ? anchorX - w(row[first]) / 2 - left : anchorX - total / 2
  let x = Math.min(Math.max(want, view.x + FAN_GAP), view.x + view.w - FAN_GAP - total)
  const y = below ? win.y + win.h + drop : view.y + view.h - h - 8
  const slots: FanSlot[] = row.map((i) => {
    const box = { x, y, w: w(i), h }
    x += w(i) + FAN_GAP
    return { index: i, box }
  })
  return { anchor: below ? { x: anchorX, y: win.y + win.h } : null, slots, more }
}

// ---- links on screen: where a link's portals and its linkage sit ----

// Where the line from a box's center toward `to` leaves the box.
export function borderPoint(r: Rect, to: Point): Point {
  const cx = r.x + r.w / 2
  const cy = r.y + r.h / 2
  const dx = to.x - cx
  const dy = to.y - cy
  if (dx === 0 && dy === 0) return { x: cx, y: cy }
  const sx = dx === 0 ? Infinity : r.w / 2 / Math.abs(dx)
  const sy = dy === 0 ? Infinity : r.h / 2 / Math.abs(dy)
  const s = Math.min(sx, sy)
  return { x: cx + dx * s, y: cy + dy * s }
}

// The part of the segment p→q inside the rect, as parameters along it (Liang-Barsky).
export function clipSegment(p: Point, q: Point, r: Rect): [number, number] | null {
  const dx = q.x - p.x
  const dy = q.y - p.y
  let t0 = 0
  let t1 = 1
  const edges: [number, number][] = [
    [-dx, p.x - r.x],
    [dx, r.x + r.w - p.x],
    [-dy, p.y - r.y],
    [dy, r.y + r.h - p.y],
  ]
  for (const [pp, qq] of edges) {
    if (pp === 0) {
      if (qq < 0) return null
      continue
    }
    const t = qq / pp
    if (pp < 0) {
      if (t > t1) return null
      if (t > t0) t0 = t
    } else {
      if (t < t0) return null
      if (t < t1) t1 = t
    }
  }
  return t0 <= t1 ? [t0, t1] : null
}

export interface LinkOnScreen {
  // The visible part of the link, border to border, clipped to the view.
  from: Point
  to: Point
  // An end outside the view: a portal for that end sits at `from` (the source's)
  // or `to` (the target's).
  sourceOut: boolean
  targetOut: boolean
  // The direction from the source toward the target, in radians.
  angle: number
}

const inside = (p: Point, r: Rect) => p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h

// A link between two rects as the view shows it: the border-to-border segment
// clipped to the view, or null when none of it shows (wholly out of view, shorter
// than `min`, or the two rects overlapping so there is no line between them).
export function linkOnScreen(a: Rect, b: Rect, view: Rect, min = 12): LinkOnScreen | null {
  const ca = { x: a.x + a.w / 2, y: a.y + a.h / 2 }
  const cb = { x: b.x + b.w / 2, y: b.y + b.h / 2 }
  const p = borderPoint(a, cb)
  const q = borderPoint(b, ca)
  // Overlapping rects: the border points cross over.
  if (inside(p, b) || inside(q, a)) return null
  const t = clipSegment(p, q, view)
  if (!t) return null
  const from = { x: p.x + (q.x - p.x) * t[0], y: p.y + (q.y - p.y) * t[0] }
  const to = { x: p.x + (q.x - p.x) * t[1], y: p.y + (q.y - p.y) * t[1] }
  if (Math.hypot(to.x - from.x, to.y - from.y) < min) return null
  return { from, to, sourceOut: !inside(p, view), targetOut: !inside(q, view), angle: Math.atan2(cb.y - ca.y, cb.x - ca.x) }
}

// A box of size w×h centered at `at`, moved inside the view.
export function clampBox(at: Point, w: number, h: number, view: Rect): Rect {
  const x = Math.min(Math.max(at.x - w / 2, view.x), view.x + view.w - w)
  const y = Math.min(Math.max(at.y - h / 2, view.y), view.y + view.h - h)
  return { x, y, w, h }
}

// The box moved off the ones already placed: stepped along the axis the view's edge
// runs, so a column of portals stacks instead of piling up.
export function avoid(box: Rect, placed: Rect[], view: Rect, gap = 4): Rect {
  const hits = (r: Rect) => placed.some((o) => r.x < o.x + o.w + gap && o.x < r.x + r.w + gap && r.y < o.y + o.h + gap && o.y < r.y + r.h + gap)
  if (!hits(box)) return box
  const vertical = box.x <= view.x + 1 || box.x + box.w >= view.x + view.w - 1
  for (let i = 1; i < 24; i++) {
    for (const sign of [1, -1]) {
      const d = sign * i * ((vertical ? box.h : box.w) + gap) * 0.5
      const r = vertical ? { ...box, y: box.y + d } : { ...box, x: box.x + d }
      if (r.x < view.x || r.y < view.y || r.x + r.w > view.x + view.w || r.y + r.h > view.y + view.h) continue
      if (!hits(r)) return r
    }
  }
  return box
}

// The parts of the segment from→to not under any of the rects (the windows drawn
// over the link), as parameter ranges along it, in order.
export function visiblePieces(from: Point, to: Point, covers: Rect[]): [number, number][] {
  let pieces: [number, number][] = [[0, 1]]
  for (const r of covers) {
    const hit = clipSegment(from, to, r)
    if (!hit) continue
    const next: [number, number][] = []
    for (const [a, b] of pieces) {
      if (hit[1] <= a || hit[0] >= b) next.push([a, b])
      else {
        if (hit[0] > a) next.push([a, hit[0]])
        if (hit[1] < b) next.push([hit[1], b])
      }
    }
    pieces = next
  }
  return pieces
}

// Which edge of the view a box sits against, if any.
export function sideOf(box: Rect, view: Rect): 'left' | 'right' | 'top' | 'bottom' | 'inside' {
  if (box.x <= view.x + 1) return 'left'
  if (box.x + box.w >= view.x + view.w - 1) return 'right'
  if (box.y <= view.y + 1) return 'top'
  if (box.y + box.h >= view.y + view.h - 1) return 'bottom'
  return 'inside'
}

// A box against the view's left or right edge narrowed to the margin between that
// edge and the windows beside it, so it covers no text; never below `min` wide.
export function fitMargin(box: Rect, covers: Rect[], view: Rect, min: number): Rect {
  const side = sideOf(box, view)
  const rows = covers.filter((o) => o.y < box.y + box.h && box.y < o.y + o.h && o.x < box.x + box.w && box.x < o.x + o.w)
  if (rows.length === 0) return box
  if (side === 'right') {
    const right = view.x + view.w
    const left = Math.max(...rows.map((o) => o.x + o.w)) + 4
    const w = Math.max(min, right - left)
    return w < box.w ? { ...box, x: right - w, w } : box
  }
  if (side === 'left') {
    const right = Math.min(...rows.map((o) => o.x)) - 4
    const w = Math.max(min, right - view.x)
    return w < box.w ? { ...box, x: view.x, w } : box
  }
  return box
}

// ---- the focus move: one short flight, moving and zooming together ----

export interface Camera {
  // The world point at the view's center, and the zoom.
  cx: number
  cy: number
  z: number
}

// How long a focus move takes, and the zoom it passes through when it travels far.
export const FLY_MS = 350
export const FLY_DIP = 0.75

const ease = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2)

// The camera at time t (0 to 1) of a flight. The center moves the whole time; the
// zoom eases down to `dip` by halfway and back up to the target, never below where it
// starts or ends, so a long move passes 75 percent instead of zooming far out. A
// short move (`far` false) zooms straight from start to end.
export function flyFrame(from: Camera, to: Camera, t: number, far: boolean, dip = FLY_DIP): Camera {
  const k = ease(Math.min(Math.max(t, 0), 1))
  const cx = from.cx + (to.cx - from.cx) * k
  const cy = from.cy + (to.cy - from.cy) * k
  const lo = far ? Math.min(dip, from.z, to.z) : Math.min(from.z, to.z)
  let z: number
  if (!far || lo >= Math.min(from.z, to.z)) z = from.z + (to.z - from.z) * k
  else if (t < 0.5) z = from.z + (lo - from.z) * ease(t * 2)
  else z = lo + (to.z - lo) * ease((t - 0.5) * 2)
  return { cx, cy, z }
}

// The windows nearest the view's center, at most LIVE_MAX.
export function nearest(rects: [string, Rect][], center: Point, max = LIVE_MAX): string[] {
  return rects
    .map(([id, r]) => {
      const dx = Math.max(r.x - center.x, 0, center.x - (r.x + r.w))
      const dy = Math.max(r.y - center.y, 0, center.y - (r.y + r.h))
      return { id, d: Math.hypot(dx, dy), c: Math.hypot(r.x + r.w / 2 - center.x, r.y + r.h / 2 - center.y) }
    })
    .sort((a, b) => a.d - b.d || a.c - b.c)
    .slice(0, max)
    .map((x) => x.id)
}
