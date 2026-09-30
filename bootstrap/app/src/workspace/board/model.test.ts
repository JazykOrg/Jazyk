import { beforeEach, describe, expect, it } from 'vitest'
import type { Entity, Graph, Relationship } from '../../lib/api'
import {
  buildLayer,
  chipId,
  chipSpot,
  clampBox,
  fitMargin,
  visiblePieces,
  clipSegment,
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

describe('the graph layer', () => {
  it('links a document to the entities it defines and to the ones it mentions, each with the requirements behind it', () => {
    const l = buildLayer([{ path: '/docs/docs/backend.md' }], G, true)
    // backend.md defines Inventory Management (its first mention) and mentions Backend.
    expect(l.chips.map((c) => c.entity)).toEqual(['ent:inventory', 'ent:backend'])
    expect(l.chips.every((c) => c.count === 2)).toBe(true)
    expect(l.edges.filter((e) => e.kind === 'def').map((e) => e.target)).toEqual([chipId('ent:inventory')])
    const men = l.edges.find((e) => e.kind === 'men')!
    expect(men).toMatchObject({ target: chipId('ent:backend'), label: 'mentions', reqs: ['req:holds'] })
    const rel = l.edges.find((e) => e.kind === 'rel')!
    expect(rel).toMatchObject({ source: chipId('ent:backend'), target: chipId('ent:inventory'), label: 'is made of', reqs: ['req:holds'] })
  })
  it('draws one link per pair with the strongest type, both ways when contributions run both ways', () => {
    const l = buildLayer([{ path: '/docs/docs/sub/inventory.md' }], G, true)
    const rel = l.edges.filter((e) => e.kind === 'rel')
    expect(rel).toHaveLength(1)
    expect(rel[0]).toMatchObject({ type: 'aggregation', label: 'has', both: true, reqs: ['req:stock'] })
  })
  it('an open entity window stands for its entity and brings its neighbours', () => {
    const l = buildLayer([{ path: '/docs/docs/backend.md' }, { path: '/model/entities/ent%3Ainventory', from: '/docs/docs/backend.md' }], G, true)
    expect(l.entityNode.get('ent:inventory')).toBe(windowId('/model/entities/ent%3Ainventory'))
    expect(l.chips.map((c) => c.entity)).toContain('ent:product')
    expect(l.edges.some((e) => e.kind === 'def' && e.target === windowId('/model/entities/ent%3Ainventory'))).toBe(true)
    // The document already links to the window, so no second edge for `from`.
    expect(l.edges.some((e) => e.kind === 'from')).toBe(false)
  })
  it('with the layer off only the opened-from edges remain', () => {
    const l = buildLayer([{ path: '/docs/docs/main.md' }, { path: '/docs/docs/backend.md', from: '/docs/docs/main.md' }], G, false)
    expect(l.chips).toEqual([])
    expect(l.edges.map((e) => e.kind)).toEqual(['from'])
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

describe('chip spots', () => {
  it('spread the column over the window and center it', () => {
    const owner = { x: 0, y: 0, w: 800, h: 600 }
    const first = chipSpot(owner, 0, 3, 1)
    const last = chipSpot(owner, 2, 3, 1)
    expect(first.x).toBeGreaterThan(800)
    expect(first.y).toBeGreaterThanOrEqual(0)
    expect(last.y + 34).toBeLessThanOrEqual(600)
    expect(Math.round((first.y + last.y + 34) / 2)).toBe(300)
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
  it('keeps the windows, the pinned chips, the layer toggle, and the viewport per project', () => {
    writeSaved('demo:erp', { windows: { '/docs/a.md': { x: 1, y: 2, w: 3, h: 4 } }, chips: { 'ent:x': { x: 5, y: 6 } }, graph: false, viewport: { x: 1, y: 1, zoom: 0.5 } })
    const back = readSaved('demo:erp')
    expect(back.windows['/docs/a.md']).toEqual({ x: 1, y: 2, w: 3, h: 4 })
    expect(back.chips['ent:x']).toEqual({ x: 5, y: 6 })
    expect(back.graph).toBe(false)
    expect(back.viewport?.zoom).toBe(0.5)
    expect(readSaved('demo:other').windows).toEqual({})
    localStorage.setItem('jazyk.board:bad', '{nope')
    expect(readSaved('bad').graph).toBe(true)
  })
})
