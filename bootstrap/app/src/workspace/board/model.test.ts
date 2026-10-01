import { beforeEach, describe, expect, it } from 'vitest'
import type { Entity, Graph, Relationship } from '../../lib/api'
import {
  buildLayer,
  clampBox,
  caretEntities,
  layoutFan,
  windowEntities,
  fitMargin,
  visiblePieces,
  clipSegment,
  flyFrame,
  focusZoom,
  levelOf,
  linkOnScreen,
  linkRequirements,
  nearest,
  placeBeside,
  placeRight,
  readSaved,
  windowId,
  writeSaved,
  zoomOut,
  WIN_GAP,
  WIN_H,
  WIN_W,
  windowSize,
  ENTITY_W,
  type Rect,
} from './model'

function graph(entities: Record<string, Partial<Entity>>, relationships: Record<string, Relationship> = {}): Graph {
  const es: Record<string, Entity> = {}
  for (const [id, e] of Object.entries(entities)) es[id] = { name: id.slice(4), ...e } as Entity
  return { generation: 1, entities: es, requirements: {}, views: {}, relationships, stateMachines: {}, diagnostics: {}, redirects: {} }
}

const m = (doc: string) => ({ doc, section: '/', quote: 'q' })

const G0 = graph(
  {
    'ent:erp': { name: 'ERP', mentions: [m('docs/main.md')] },
    'ent:backend': { name: 'Backend', parent: 'ent:erp', mentions: [m('docs/main.md'), m('docs/backend.md')] },
    'ent:inventory': { name: 'Inventory Management', parent: 'ent:backend', mentions: [m('docs/backend.md'), m('docs/sub/inventory.md')] },
    'ent:product': { name: 'Product', mentions: [m('docs/sub/inventory.md')] },
  },
  {
    'rel:1': { members: ['ent:backend', 'ent:inventory'], contributions: [{ a: 'ent:backend', b: 'ent:inventory', type: 'composition', requirements: ['req:holds'] }] },
    'rel:2': {
      members: ['ent:inventory', 'ent:product'],
      contributions: [
        { a: 'ent:inventory', b: 'ent:product', type: 'dependency', requirements: [] },
        { a: 'ent:inventory', b: 'ent:product', type: 'aggregation', requirements: [] },
        { a: 'ent:product', b: 'ent:inventory', type: 'dependency', requirements: ['req:stock'] },
      ],
    },
  },
)
const G: Graph = {
  ...G0,
  requirements: {
    'req:holds': { statement: 'The backend holds inventory management.', entities: ['ent:backend', 'ent:inventory'], source: { doc: 'docs/backend.md', section: '/', quote: 'q' } },
    'req:stock': { statement: 'Products are counted in stock.', entities: ['ent:product', 'ent:inventory'], source: { doc: 'docs/sub/inventory.md', section: '/', quote: 'q' } },
  },
}

describe('levels', () => {
  it('follows the plan table', () => {
    expect(levelOf(1)).toBe('live')
    expect(levelOf(0.8)).toBe('live')
    expect(levelOf(0.6)).toBe('still')
    expect(levelOf(0.3)).toBe('card')
    expect(levelOf(0.1)).toBe('title')
  })
  it('Escape steps out one level, and the titles are the last', () => {
    expect(levelOf(zoomOut('live')!)).toBe('still')
    expect(levelOf(zoomOut('still')!)).toBe('card')
    expect(levelOf(zoomOut('card')!)).toBe('title')
    expect(zoomOut('title')).toBeNull()
  })
})

describe('focus', () => {
  it('opens a page at 80 percent of the board and an entity as a card', () => {
    expect(windowSize('doc', { w: 1000, h: 700 })).toEqual({ w: 800, h: 560 })
    expect(windowSize('entity', { w: 1000, h: 700 }).w).toBe(ENTITY_W)
    expect(windowSize('doc', null)).toEqual({ w: WIN_W, h: WIN_H })
  })
  it('zooms a window to fill 80 percent, a page never above 100 percent', () => {
    const board = { w: 1000, h: 700 }
    expect(focusZoom({ x: 0, y: 0, w: 800, h: 560 }, board, 'doc')).toBeCloseTo(1)
    expect(focusZoom({ x: 0, y: 0, w: 1600, h: 560 }, board, 'doc')).toBeCloseTo(0.5)
    expect(focusZoom({ x: 0, y: 0, w: 400, h: 280 }, board, 'doc')).toBe(1)
    expect(focusZoom({ x: 0, y: 0, w: 400, h: 280 }, board, 'entity')).toBe(1.25)
  })
})

describe('placement', () => {
  it('puts the first window at the origin and the next right of the rightmost', () => {
    const a = placeRight([])
    expect(a).toEqual({ x: 0, y: 0, w: WIN_W, h: WIN_H })
    const b = placeRight([a])
    expect(b.x).toBe(WIN_W + WIN_GAP)
    expect(b.y).toBe(0)
  })
  it('puts a window beside the one it came from, below one already there', () => {
    const src: Rect = { x: 100, y: 50, w: 700, h: 500 }
    const first = placeBeside(src, [src])
    expect(first).toMatchObject({ x: 100 + 700 + WIN_GAP, y: 50 })
    const second = placeBeside(src, [src, first])
    expect(second.x).toBe(first.x)
    expect(second.y).toBeGreaterThanOrEqual(first.y + first.h)
  })
})

describe('the links between open windows', () => {
  const BACKEND_DOC = { path: '/docs/docs/backend.md' }
  const INV = { path: '/model/entities/ent%3Ainventory', from: '/docs/docs/backend.md' }
  const BACK = { path: '/model/entities/ent%3Abackend' }
  const PROD = { path: '/model/entities/ent%3Aproduct' }
  it('a document alone has no links; what it brings is its fan', () => {
    const l = buildLayer([BACKEND_DOC], G)
    expect(l.edges).toEqual([])
    // backend.md defines Inventory Management (its first mention) and mentions Backend.
    expect(windowEntities(G, BACKEND_DOC)).toEqual([
      { entity: 'ent:inventory', edge: 'def' },
      { entity: 'ent:backend', edge: 'men' },
    ])
  })
  it('links a document to the open entities it defines and mentions, each with the requirements behind it', () => {
    const l = buildLayer([BACKEND_DOC, INV, BACK], G)
    expect(l.edges.find((e) => e.kind === 'def')).toMatchObject({ target: windowId(INV.path), label: 'defines', reqs: ['req:holds'] })
    expect(l.edges.find((e) => e.kind === 'men')).toMatchObject({ target: windowId(BACK.path), label: 'mentions', reqs: ['req:holds'] })
    const rel = l.edges.find((e) => e.kind === 'rel')!
    expect(rel).toMatchObject({ source: windowId(BACK.path), target: windowId(INV.path), label: 'is made of', reqs: ['req:holds'] })
    // The document already links to the entity, so no second link for `from`.
    expect(l.edges.some((e) => e.kind === 'from')).toBe(false)
  })
  it('draws one link per pair with the strongest type, both ways when contributions run both ways', () => {
    const rel = buildLayer([INV, PROD], G).edges.filter((e) => e.kind === 'rel')
    expect(rel).toHaveLength(1)
    expect(rel[0]).toMatchObject({ type: 'aggregation', label: 'has', both: true, reqs: ['req:stock'] })
  })
  it('links a window to one opened from inside it', () => {
    const l = buildLayer([{ path: '/docs/docs/main.md' }, { path: '/docs/docs/backend.md', from: '/docs/docs/main.md' }], G)
    expect(l.edges.map((e) => e.kind)).toEqual(['from'])
  })
})

describe('references', () => {
  it('brings the mention’s entity, then the entities of the caret’s requirements, never an open one', () => {
    expect(caretEntities(G, 'ent:backend', ['req:stock'], new Set())).toEqual(['ent:backend', 'ent:product', 'ent:inventory'])
    expect(caretEntities(G, null, ['req:holds'], new Set(['ent:backend']))).toEqual(['ent:inventory'])
    expect(caretEntities(G, null, [], new Set())).toEqual([])
  })
  const view: Rect = { x: 0, y: 0, w: 1000, h: 800 }
  it('puts the first chip under the window, the next right and left, and a +N for the rest', () => {
    const win: Rect = { x: 250, y: 50, w: 500, h: 600 }
    const f = layoutFan(win, [100, 100, 100, 100, 100, 100], view)
    expect(f.anchor).toEqual({ x: 500, y: 650 })
    const first = f.slots.find((s) => s.index === 0)!
    expect(first.box.x + first.box.w / 2).toBe(500)
    expect(f.slots.find((s) => s.index === 1)!.box.x).toBeGreaterThan(first.box.x)
    expect(f.slots.find((s) => s.index === 2)!.box.x).toBeLessThan(first.box.x)
    expect(f.more).toBeGreaterThan(0)
    expect(f.slots[f.slots.length - 1].index).toBe(-1)
    for (const s of f.slots) expect(s.box.y).toBe(650 + 52)
  })
  it('drops less when the room below is short', () => {
    const f = layoutFan({ x: 250, y: 50, w: 500, h: 700 }, [100], view)
    expect(f.anchor).toEqual({ x: 500, y: 750 })
    expect(f.slots[0].box.y).toBe(750 + 18)
  })
  it('without room below, sits along the view’s bottom edge with no lines', () => {
    const f = layoutFan({ x: 100, y: 50, w: 800, h: 740 }, [100, 100], view)
    expect(f.anchor).toBeNull()
    expect(f.more).toBe(0)
    expect(f.slots[0].box.y + f.slots[0].box.h).toBeLessThanOrEqual(800)
  })
})

describe('requirements behind a link', () => {
  it('between two entities, a document and an entity, a requirement and an entity', () => {
    expect(linkRequirements(G, { entity: 'ent:inventory' }, { entity: 'ent:product' })).toEqual(['req:stock'])
    expect(linkRequirements(G, { doc: 'docs/backend.md' }, { entity: 'ent:inventory' })).toEqual(['req:holds'])
    expect(linkRequirements(G, { entity: 'ent:inventory' }, { doc: 'docs/main.md' })).toEqual([])
    expect(linkRequirements(G, { requirement: 'req:stock' }, { entity: 'ent:product' })).toEqual(['req:stock'])
    expect(linkRequirements(G, { doc: 'docs/sub/inventory.md' }, { requirement: 'req:stock' })).toEqual(['req:stock'])
  })
})

describe('the focus move', () => {
  it('moves and zooms together, passing 75 percent on a long move and no lower', () => {
    const a = { cx: 0, cy: 0, z: 1 }
    const b = { cx: 3000, cy: 0, z: 1 }
    expect(flyFrame(a, b, 0, true)).toEqual(a)
    expect(flyFrame(a, b, 1, true)).toEqual(b)
    const mid = flyFrame(a, b, 0.5, true)
    expect(mid.z).toBeCloseTo(0.75)
    expect(mid.cx).toBeCloseTo(1500)
    for (let t = 0; t <= 1; t += 0.05) expect(flyFrame(a, b, t, true).z).toBeGreaterThanOrEqual(0.75 - 1e-9)
  })
  it('never dips below where it starts or ends, and a short move zooms straight', () => {
    const far = { cx: 0, cy: 0, z: 0.3 }
    expect(flyFrame(far, { cx: 2000, cy: 0, z: 1 }, 0.5, true).z).toBeGreaterThan(0.3)
    expect(flyFrame({ cx: 0, cy: 0, z: 1 }, { cx: 50, cy: 0, z: 1 }, 0.5, false).z).toBe(1)
  })
})

describe('links on screen', () => {
  const view: Rect = { x: 0, y: 0, w: 1000, h: 600 }
  const A: Rect = { x: -900, y: 250, w: 200, h: 100 }
  const B: Rect = { x: 1700, y: 250, w: 200, h: 100 }
  it('clips a segment to a rect', () => {
    expect(clipSegment({ x: -100, y: 50 }, { x: 100, y: 50 }, { x: 0, y: 0, w: 50, h: 100 })).toEqual([0.5, 0.75])
    expect(clipSegment({ x: -100, y: 500 }, { x: 100, y: 500 }, { x: 0, y: 0, w: 50, h: 100 })).toBeNull()
  })
  it('both ends in view: no portal', () => {
    const l = linkOnScreen({ x: 100, y: 250, w: 200, h: 100 }, { x: 700, y: 250, w: 200, h: 100 }, view)!
    expect(l.sourceOut || l.targetOut).toBe(false)
    expect(l.from.x).toBe(300)
    expect(l.to.x).toBe(700)
  })
  it('one end out of view: a portal where the link meets the edge', () => {
    const l = linkOnScreen(A, { x: 400, y: 250, w: 200, h: 100 }, view)!
    expect(l.sourceOut).toBe(true)
    expect(l.targetOut).toBe(false)
    expect(l.from).toEqual({ x: 0, y: 300 })
  })
  it('crossing the view with both ends out: a portal at each side', () => {
    const l = linkOnScreen(A, B, view)!
    expect(l.sourceOut && l.targetOut).toBe(true)
    expect(l.from.x).toBe(0)
    expect(l.to.x).toBe(1000)
  })
  it('wholly out of view, or hidden under its window: nothing', () => {
    expect(linkOnScreen(A, { x: -500, y: 250, w: 200, h: 100 }, view)).toBeNull()
    // B sits at the view's left edge; the link runs left from it, out of view.
    expect(linkOnScreen(A, { x: 0, y: 250, w: 200, h: 100 }, view)).toBeNull()
  })
  it('drops the stretch of a link under a window', () => {
    expect(visiblePieces({ x: 0, y: 0 }, { x: 100, y: 0 }, [{ x: 20, y: -10, w: 60, h: 20 }])).toEqual([
      [0, 0.2],
      [0.8, 1],
    ])
    expect(visiblePieces({ x: 0, y: 0 }, { x: 100, y: 0 }, [{ x: -10, y: -10, w: 200, h: 20 }])).toEqual([])
  })
  it('narrows a portal to the margin beside a window, never below its least width', () => {
    const win: Rect = { x: 100, y: 0, w: 800, h: 600 }
    expect(fitMargin({ x: 820, y: 100, w: 180, h: 26 }, [win], view, 72)).toEqual({ x: 904, y: 100, w: 96, h: 26 })
    expect(fitMargin({ x: 820, y: 100, w: 180, h: 26 }, [{ ...win, w: 870 }], view, 72)).toEqual({ x: 928, y: 100, w: 72, h: 26 })
    expect(fitMargin({ x: 0, y: 100, w: 180, h: 26 }, [win], view, 72)).toEqual({ x: 0, y: 100, w: 96, h: 26 })
  })
  it('keeps a portal inside the view', () => {
    expect(clampBox({ x: 0, y: 300 }, 100, 20, view)).toEqual({ x: 0, y: 290, w: 100, h: 20 })
  })
})

describe('live windows', () => {
  it('are the nearest the center, at most six', () => {
    const rects: [string, Rect][] = Array.from({ length: 9 }, (_, i) => [`w${i}`, { x: i * 1000, y: 0, w: 800, h: 600 }])
    const live = nearest(rects, { x: 4400, y: 300 })
    expect(live).toHaveLength(6)
    expect(live[0]).toBe('w4')
    expect(live).not.toContain('w0')
  })
})

describe('memory', () => {
  beforeEach(() => localStorage.clear())
  it('keeps the windows, the fan toggle, and the viewport per project', () => {
    writeSaved('demo:erp', { windows: { '/docs/a.md': { x: 1, y: 2, w: 3, h: 4 } }, graph: false, viewport: { x: 1, y: 1, zoom: 0.5 } })
    const back = readSaved('demo:erp')
    expect(back.windows['/docs/a.md']).toEqual({ x: 1, y: 2, w: 3, h: 4 })
    expect(back.graph).toBe(false)
    expect(back.viewport?.zoom).toBe(0.5)
    expect(readSaved('demo:other').windows).toEqual({})
    localStorage.setItem('jazyk.board:bad', '{nope')
    expect(readSaved('bad').graph).toBe(true)
  })
})
