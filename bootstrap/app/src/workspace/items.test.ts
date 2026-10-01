import { describe, expect, it } from 'vitest'
import { isFileItem, prune, type Item } from './items'

const A = '/docs/docs/a.md'
const B = '/model/entities/ent%3Ab'
const C = '/model/entities/ent%3Ac'

describe('held items', () => {
  it('files stay; others are transient', () => {
    expect(isFileItem(A)).toBe(true)
    expect(isFileItem('/product/files/src/x.js')).toBe(true)
    expect(isFileItem(B)).toBe(false)
    expect(isFileItem('/inbox')).toBe(false)
  })
  const items: Item[] = [{ path: A }, { path: B, from: A }, { path: C, from: B }]
  it('the focus holds the unpinned items it came from', () => {
    expect(prune(items, C).map((i) => i.path)).toEqual([A, B, C])
  })
  it('focusing the file lets the unpinned items go', () => {
    expect(prune(items, A).map((i) => i.path)).toEqual([A])
  })
  it('a pinned item holds nothing but itself', () => {
    const pinned = items.map((i) => (i.path === C ? { ...i, pinned: true } : i))
    expect(prune(pinned, C).map((i) => i.path)).toEqual([A, C])
    // and it stays when the focus moves away
    expect(prune(pinned, A).map((i) => i.path)).toEqual([A, C])
  })
  it('with nothing focused, everything stays', () => {
    expect(prune(items, null)).toBe(items)
  })
})
