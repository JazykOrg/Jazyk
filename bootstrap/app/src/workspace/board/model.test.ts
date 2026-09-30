import { beforeEach, describe, expect, it } from 'vitest'
import type { Entity, Graph, Relationship } from '../../lib/api'
import {
  buildGroups,
  buildLayer,
  chipId,
  chipSpot,
  componentOf,
  levelOf,
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
  type Rect,
} from './model'

function graph(entities: Record<string, Partial<Entity>>, relationships: Record<string, Relationship> = {}): Graph {
  const es: Record<string, Entity> = {}
  for (const [id, e] of Object.entries(entities)) es[id] = { name: id.slice(4), ...e } as Entity
  return { generation: 1, entities: es, requirements: {}, views: {}, relationships, stateMachines: {}, diagnostics: {}, redirects: {} }
}

const m = (doc: string) => ({ doc, section: '/', quote: 'q' })

const G = graph(
  {
    'ent:erp': { name: 'ERP', mentions: [m('docs/main.md')] },
    'ent:backend': { name: 'Backend', parent: 'ent:erp', mentions: [m('docs/main.md'), m('docs/backend.md')] },
    'ent:inventory': { name: 'Inventory Management', parent: 'ent:backend', mentions: [m('docs/backend.md'), m('docs/sub/inventory.md')] },
    'ent:product': { name: 'Product', mentions: [m('docs/sub/inventory.md')] },
  },
  {
    'rel:1': { members: ['ent:backend', 'ent:inventory'], contributions: [{ a: 'ent:backend', b: 'ent:inventory', type: 'composition', requirements: [] }] },
    'rel:2': {
      members: ['ent:inventory', 'ent:product'],
      contributions: [
        { a: 'ent:inventory', b: 'ent:product', type: 'dependency', requirements: [] },
        { a: 'ent:inventory', b: 'ent:product', type: 'aggregation', requirements: [] },
      ],
    },
  },
)

describe('levels', () => {
  it('follows the plan table', () => {
    expect(levelOf(1)).toBe('live')
    expect(levelOf(0.8)).toBe('live')
    expect(levelOf(0.6)).toBe('still')
    expect(levelOf(0.3)).toBe('card')
    expect(levelOf(0.1)).toBe('group')
  })
  it('Escape steps out one level, and the overview is the last', () => {
    expect(levelOf(zoomOut('live')!)).toBe('still')
    expect(levelOf(zoomOut('still')!)).toBe('card')
    expect(levelOf(zoomOut('card')!)).toBe('group')
    expect(zoomOut('group')).toBeNull()
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
  it('brings a document’s defined entities with a dashed edge and its mentions without one', () => {
    const l = buildLayer([{ path: '/docs/docs/backend.md' }], G, true)
    // backend.md defines Inventory Management (its first mention) and mentions Backend.
    expect(l.chips.map((c) => c.entity)).toEqual(['ent:inventory', 'ent:backend'])
    expect(l.edges.filter((e) => e.kind === 'def').map((e) => e.target)).toEqual([chipId('ent:inventory')])
    const rel = l.edges.find((e) => e.kind === 'rel')!
    expect(rel).toMatchObject({ source: chipId('ent:backend'), target: chipId('ent:inventory'), label: 'is made of' })
  })
  it('draws one edge per direction with the strongest type', () => {
    const l = buildLayer([{ path: '/docs/docs/sub/inventory.md' }], G, true)
    const rel = l.edges.filter((e) => e.kind === 'rel')
    expect(rel).toHaveLength(1)
    expect(rel[0]).toMatchObject({ type: 'aggregation', label: 'has' })
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

describe('groups', () => {
  it('holds an entity by the direct child of its root, a leaf under a root by the root', () => {
    expect(componentOf(G, 'ent:inventory')).toBe('ent:backend')
    expect(componentOf(G, 'ent:backend')).toBe('ent:backend')
    expect(componentOf(G, 'ent:erp')).toBe('ent:erp')
    expect(componentOf(G, 'ent:product')).toBe('')
    const leaf = graph({ 'ent:root': {}, 'ent:leaf': { parent: 'ent:root' } })
    expect(componentOf(leaf, 'ent:leaf')).toBe('ent:root')
  })
  it('groups documents by folder and entities by component, the edges lifted with counts, no regions overlapping', () => {
    const items = [{ path: '/docs/docs/main.md' }, { path: '/docs/docs/sub/inventory.md' }, { path: '/docs/docs/backend.md' }]
    const layer = buildLayer(items, G, true)
    const rects = new Map<string, Rect>()
    items.forEach((it, i) => rects.set(windowId(it.path), { x: i * 1300, y: 0, w: WIN_W, h: WIN_H }))
    for (const c of layer.chips) {
      const p = chipSpot(rects.get(windowId(c.owner))!, c.index, 1)
      rects.set(chipId(c.entity), { ...p, w: 200, h: 34 })
    }
    const { groups, edges } = buildGroups(items, layer, G, rects)
    const labels = groups.map((g) => g.label).sort()
    expect(labels).toEqual(['Backend', 'Other entities', 'docs/', 'docs/sub/', 'ERP'].sort())
    expect(groups.find((g) => g.label === 'docs/')!.documents).toBe(2)
    expect(groups.find((g) => g.label === 'Backend')!.entities).toBe(2)
    expect(edges.length).toBeGreaterThan(0)
    for (const a of groups)
      for (const b of groups) {
        if (a === b) continue
        const apart = a.rect.x + a.rect.w <= b.rect.x || b.rect.x + b.rect.w <= a.rect.x || a.rect.y + a.rect.h <= b.rect.y || b.rect.y + b.rect.h <= a.rect.y
        expect(apart).toBe(true)
      }
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
