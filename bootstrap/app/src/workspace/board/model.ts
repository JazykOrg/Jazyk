// The board's pure logic (docs/frontends/gui.md#workspace-experimental,
// plans/workspace.md#board): where a window goes, which entity chips the open
// windows bring, the edges between them, the zoom levels, and the groups the
// overview draws. No React here, so the rules are unit tested.
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
// (the path without its query), the chips a person dragged or `Arrange` placed, the
// graph layer's toggle, and the viewport.
export interface Saved {
  windows: Record<string, Rect>
  chips: Record<string, Point>
  graph: boolean
  viewport?: { x: number; y: number; zoom: number }
}

export const WIN_W = 760
export const WIN_H = 620
export const WIN_MIN_W = 320
export const WIN_MIN_H = 220
export const CHIP_W = 210
export const CHIP_H = 34
export const CHIP_STEP = 48
// The chips sit in a column this far right of their window.
export const CHIP_GAP = 44
// The horizontal gap between two windows side by side: the chips' gutter.
export const WIN_GAP = 480
// The chips a window brings, at most.
export const CHIPS_PER_WINDOW = 12
// The windows live at once (plans/workspace.md#cost).
export const LIVE_MAX = 6

export type Level = 'live' | 'still' | 'card' | 'group'

// The zoom levels of plans/workspace.md#zoom-levels.
export function levelOf(zoom: number): Level {
  if (zoom >= 0.8) return 'live'
  if (zoom >= 0.4) return 'still'
  if (zoom >= 0.15) return 'card'
  return 'group'
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

// Chips and their labels grow on the cards level so they stay named.
export function chipScale(level: Level): number {
  return level === 'card' ? 2.2 : 1
}

const storeKey = (project: string) => `jazyk.board:${project}`

export function emptySaved(): Saved {
  return { windows: {}, chips: {}, graph: true }
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
    for (const [k, p] of Object.entries(v.chips ?? {})) {
      if (p && finite(p.x, p.y)) out.chips[k] = { x: p.x, y: p.y }
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

// A window opened from a chip goes just right of the chip.
export function placeNear(p: Point, taken: Rect[], w = WIN_W, h = WIN_H): Rect {
  return freeSlot(p.x + CHIP_W + 80, p.y - 60, w, h, taken)
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
export const chipId = (entity: string) => `c:${entity}`
export const groupId = (key: string) => `g:${key}`

export type EdgeKind = 'rel' | 'def' | 'ref' | 'from'

export interface LayerEdge {
  id: string
  source: string
  target: string
  kind: EdgeKind
  label?: string
  // The relationship type, for the lifted edges' label.
  type?: string
}

export interface LayerChip {
  entity: string
  // The window whose column the chip sits in.
  owner: string
  // Its place in that column.
  index: number
}

export interface Layer {
  chips: LayerChip[]
  edges: LayerEdge[]
  // The node each present entity is drawn as: its own window, else its chip.
  entityNode: Map<string, string>
}

// The types strongest first, so two contributions between one pair draw as one edge.
const STRENGTH = ['generalization', 'realization', 'composition', 'aggregation', 'association', 'dependency']
const strength = (t: string | undefined) => {
  const i = STRENGTH.indexOf(t ?? 'dependency')
  return i < 0 ? STRENGTH.length : i
}

// The entities a window brings as chips, the defined ones (drawn with a dashed
// edge) first, at most CHIPS_PER_WINDOW.
function windowEntities(g: Graph, item: Item): { entity: string; edge?: 'def' | 'ref' }[] {
  const kind = itemKind(item.path)
  const cap = (xs: { entity: string; edge?: 'def' | 'ref' }[]) => xs.slice(0, CHIPS_PER_WINDOW)
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
    return cap([...defined.map((entity) => ({ entity, edge: 'def' as const })), ...mentioned.map((m) => ({ entity: m.id }))])
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
    return cap((v?.members ?? []).map((e) => canonical(g, e)).filter((e) => g.entities[e]).map((entity) => ({ entity })))
  }
  return []
}

// The graph layer over the open windows: the chips, the relationship edges between
// every present entity with its verb, the dashed edges from a document to the
// entities it defines, and an edge from a window to one opened from it.
export function buildLayer(items: Item[], g: Graph | undefined, graphOn: boolean): Layer {
  const entityNode = new Map<string, string>()
  const edges: LayerEdge[] = []
  const chips: LayerChip[] = []
  const paths = new Set(items.map((i) => itemKey(i.path)))
  if (g) {
    for (const item of items) {
      if (itemKind(item.path) !== 'entity') continue
      const id = canonical(g, nodeOf(item.path))
      if (g.entities[id] && !entityNode.has(id)) entityNode.set(id, windowId(item.path))
    }
  }
  if (g && graphOn) {
    const columns = new Map<string, number>()
    for (const item of items) {
      for (const { entity, edge } of windowEntities(g, item)) {
        if (!entityNode.has(entity)) {
          const owner = itemKey(item.path)
          const index = columns.get(owner) ?? 0
          columns.set(owner, index + 1)
          chips.push({ entity, owner, index })
          entityNode.set(entity, chipId(entity))
        }
        const target = entityNode.get(entity)!
        if (edge && target !== windowId(item.path)) {
          edges.push({ id: `${edge}:${itemKey(item.path)}->${entity}`, source: windowId(item.path), target, kind: edge })
        }
      }
    }
    const pairs = new Map<string, { a: string; b: string; type: string }>()
    for (const rel of Object.values(g.relationships)) {
      for (const c of rel.contributions) {
        const a = canonical(g, c.a)
        const b = canonical(g, c.b)
        if (a === b || !entityNode.has(a) || !entityNode.has(b)) continue
        const key = `${a}->${b}`
        const had = pairs.get(key)
        if (!had || strength(c.type) < strength(had.type)) pairs.set(key, { a, b, type: c.type || 'dependency' })
      }
    }
    for (const [key, p] of pairs) {
      edges.push({
        id: `rel:${key}`,
        source: entityNode.get(p.a)!,
        target: entityNode.get(p.b)!,
        kind: 'rel',
        type: p.type,
        label: relationPhrase(p.type, true),
      })
    }
  }
  const linked = new Set(edges.map((e) => [e.source, e.target].sort().join('|')))
  for (const item of items) {
    if (!item.from || !paths.has(itemKey(item.from))) continue
    const s = windowId(item.from)
    const t = windowId(item.path)
    if (linked.has([s, t].sort().join('|'))) continue
    linked.add([s, t].sort().join('|'))
    edges.push({ id: `from:${itemKey(item.from)}->${itemKey(item.path)}`, source: s, target: t, kind: 'from' })
  }
  return { chips, edges, entityNode }
}

// A chip's place when nobody moved it: in its window's column.
export function chipSpot(owner: Rect, index: number, scale: number): Point {
  return { x: owner.x + owner.w + CHIP_GAP, y: owner.y + index * CHIP_STEP * scale }
}

// The component holding an entity: its `parent` chain up to the direct child of
// the root. A leaf directly under a root joins the root's group, a root with
// children is its own, and a root alone is in no component ('').
export function componentOf(g: Graph, id: string, parents?: Set<string>): string {
  const chain = [id]
  const seen = new Set(chain)
  let cur = id
  for (let i = 0; i < 64; i++) {
    const p = g.entities[cur]?.parent
    if (!p || !g.entities[p] || seen.has(p)) break
    chain.push(p)
    seen.add(p)
    cur = p
  }
  const hasChildren = parents ?? new Set(Object.values(g.entities).map((e) => e.parent ?? ''))
  if (chain.length >= 3) return chain[chain.length - 2]
  if (chain.length === 2) return hasChildren.has(id) ? id : chain[1]
  return hasChildren.has(id) ? id : ''
}

function dirOf(doc: string): string {
  const i = doc.lastIndexOf('/')
  return i < 0 ? '' : doc.slice(0, i)
}

const KIND_LABEL: Record<string, string> = {
  product: 'Product',
  generated: 'Generated',
  requirement: 'Requirements',
  diagram: 'Diagrams',
  inbox: 'Inbox',
  history: 'History',
  other: 'Pages',
}

export interface Group {
  key: string
  label: string
  // What kind of region: a folder of documents, a component, a kind of page.
  kind: 'folder' | 'component' | 'kind'
  members: string[]
  documents: number
  entities: number
  pages: number
  // The region the overview draws, and the members' own bounds a zoom goes to.
  rect: Rect
  bounds: Rect
}

export interface GroupEdge {
  id: string
  source: string
  target: string
  count: number
  label: string
}

// The overview's groups: documents by folder, entities (windows and chips) by the
// component holding them, other pages by kind; each a region around its members.
// The edges between members lift to one edge per pair of groups with a count.
export function buildGroups(
  items: Item[],
  layer: Layer,
  g: Graph | undefined,
  rects: Map<string, Rect>,
): { groups: Group[]; edges: GroupEdge[] } {
  const byKey = new Map<string, Omit<Group, 'rect' | 'bounds'>>()
  const groupOfNode = new Map<string, string>()
  const add = (node: string, key: string, label: string, kind: Group['kind'], what: 'documents' | 'entities' | 'pages') => {
    let grp = byKey.get(key)
    if (!grp) {
      grp = { key, label, kind, members: [], documents: 0, entities: 0, pages: 0 }
      byKey.set(key, grp)
    }
    grp.members.push(node)
    grp[what]++
    groupOfNode.set(node, key)
  }
  const parents = new Set(Object.values(g?.entities ?? {}).map((e) => e.parent ?? ''))
  const entityGroup = (id: string): [string, string] => {
    const comp = g ? componentOf(g, id, parents) : id
    return comp ? [`comp:${comp}`, g?.entities[comp]?.name ?? comp] : ['comp:', 'Other entities']
  }
  for (const item of items) {
    const node = windowId(item.path)
    if (!rects.has(node)) continue
    const kind = itemKind(item.path)
    if (kind === 'doc') {
      const dir = dirOf(docOf(item.path))
      add(node, `folder:${dir}`, dir ? `${dir}/` : 'Documents', 'folder', 'documents')
    } else if (kind === 'entity' && g) {
      const [key, label] = entityGroup(canonical(g, nodeOf(item.path)))
      add(node, key, label, 'component', 'entities')
    } else if (kind === 'requirement' && g?.requirements[canonical(g, nodeOf(item.path))]?.entities?.length) {
      const first = g.requirements[canonical(g, nodeOf(item.path))].entities![0]
      const [key, label] = entityGroup(canonical(g, first))
      add(node, key, label, 'component', 'pages')
    } else {
      add(node, `kind:${kind}`, KIND_LABEL[kind] ?? itemTitle(item.path), 'kind', 'pages')
    }
  }
  for (const c of layer.chips) {
    const node = chipId(c.entity)
    if (!rects.has(node)) continue
    const [key, label] = entityGroup(c.entity)
    add(node, key, label, 'component', 'entities')
  }
  // Each group's members' bounds, then the regions laid out as a grid in the
  // order the members sit (left to right, then top to bottom), over the same
  // extent, so the overview reads as the board seen from far and no two regions
  // overlap. A region's size follows its member count.
  const bounded = [...byKey.values()].map((grp) => {
    let x0 = Infinity
    let y0 = Infinity
    let x1 = -Infinity
    let y1 = -Infinity
    for (const m of grp.members) {
      const r = rects.get(m)!
      x0 = Math.min(x0, r.x)
      y0 = Math.min(y0, r.y)
      x1 = Math.max(x1, r.x + r.w)
      y1 = Math.max(y1, r.y + r.h)
    }
    return { grp, bounds: { x: x0, y: y0, w: x1 - x0, h: y1 - y0 } }
  })
  const groups: Group[] = []
  if (bounded.length > 0) {
    const all = bounded.reduce(
      (a, { bounds: b }) => ({ x0: Math.min(a.x0, b.x), y0: Math.min(a.y0, b.y), x1: Math.max(a.x1, b.x + b.w), y1: Math.max(a.y1, b.y + b.h) }),
      { x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity },
    )
    const n = bounded.length
    const cols = Math.ceil(Math.sqrt(n * 1.5))
    const rows = Math.ceil(n / cols)
    const cell = Math.max(1400, Math.max(all.x1 - all.x0, all.y1 - all.y0) / Math.max(cols, rows))
    const most = Math.max(...bounded.map((b) => b.grp.members.length))
    const cx = (all.x0 + all.x1) / 2
    const cy = (all.y0 + all.y1) / 2
    const ordered = bounded
      .map((b) => ({ ...b, mx: b.bounds.x + b.bounds.w / 2, my: b.bounds.y + b.bounds.h / 2 }))
      .sort((a, b) => a.my - b.my)
    // Rows by height first, then each row left to right.
    const placed: typeof ordered = []
    for (let r = 0; r < rows; r++) placed.push(...ordered.slice(r * cols, (r + 1) * cols).sort((a, b) => a.mx - b.mx))
    placed.forEach(({ grp, bounds }, i) => {
      const col = i % cols
      const row = Math.floor(i / cols)
      const f = 0.45 + 0.3 * Math.sqrt(grp.members.length / most)
      const w = cell * f
      const h = cell * f * 0.7
      const x = cx + (col - (cols - 1) / 2) * cell - w / 2
      const y = cy + (row - (rows - 1) / 2) * cell * 0.8 - h / 2
      groups.push({ ...grp, rect: { x, y, w, h }, bounds })
    })
  }
  const lifted = new Map<string, { source: string; target: string; count: number; labels: Set<string> }>()
  for (const e of layer.edges) {
    const a = groupOfNode.get(e.source)
    const b = groupOfNode.get(e.target)
    if (!a || !b || a === b) continue
    const id = `${a}->${b}`
    let l = lifted.get(id)
    if (!l) {
      l = { source: groupId(a), target: groupId(b), count: 0, labels: new Set() }
      lifted.set(id, l)
    }
    l.count++
    l.labels.add(e.kind === 'rel' ? (e.label ?? '') : e.kind === 'def' ? 'defines' : e.kind === 'ref' ? 'names' : 'opened')
  }
  const edges: GroupEdge[] = [...lifted.entries()].map(([id, l]) => {
    const one = l.labels.size === 1 ? [...l.labels][0] : ''
    const label = one ? (l.count > 1 ? `${one} (${l.count})` : one) : `${l.count} links`
    return { id: `lift:${id}`, source: l.source, target: l.target, count: l.count, label }
  })
  return { groups, edges }
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
