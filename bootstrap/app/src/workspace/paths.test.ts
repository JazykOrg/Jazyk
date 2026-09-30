import { describe, expect, it } from 'vitest'
import { hrefPath, isShellPath, itemKey, nodePagePath, splitSelection, touchRecent } from './paths'

describe('itemKey', () => {
  it('is the path without its query or fragment', () => {
    expect(itemKey('/docs/a.md?section=%2Fa%2Fb')).toBe('/docs/a.md')
    expect(itemKey('/docs/a.md#heading')).toBe('/docs/a.md')
    expect(itemKey('/model/entities/ent%3Aquay')).toBe('/model/entities/ent%3Aquay')
  })
})

describe('splitSelection', () => {
  it('takes the node and the explorer entity out of the path', () => {
    expect(splitSelection('/docs/a.md?node=ent%3Aquay&entity=ent%3Aquay')).toEqual({ path: '/docs/a.md', node: 'ent:quay' })
  })
  it('keeps the other params and the fragment', () => {
    expect(splitSelection('/docs/a.md?section=s&node=req%3Ax#h')).toEqual({ path: '/docs/a.md?section=s#h', node: 'req:x' })
  })
  it('reads the explorer entity alone as the selection', () => {
    expect(splitSelection('/model?entity=ent%3Ab')).toEqual({ path: '/model', node: 'ent:b' })
  })
  it('leaves a path without a query as it is', () => {
    expect(splitSelection('/inbox')).toEqual({ path: '/inbox', node: null })
  })
})

describe('isShellPath', () => {
  it('names Home, connect, and the documents empty state', () => {
    expect(isShellPath('/')).toBe(true)
    expect(isShellPath('/connect')).toBe(true)
    expect(isShellPath('/docs')).toBe(true)
    expect(isShellPath('/docs/a.md')).toBe(false)
    expect(isShellPath('/inbox')).toBe(false)
  })
})

describe('nodePagePath', () => {
  it('maps a node to its page', () => {
    expect(nodePagePath('ent:quay')).toBe('/model/entities/ent%3Aquay')
    expect(nodePagePath('req:r1')).toBe('/model/requirements/req%3Ar1')
    expect(nodePagePath('view:v')).toBe('/model/diagrams/view%3Av')
    expect(nodePagePath('diag:d')).toBe('/inbox')
    expect(nodePagePath('crit:c')).toBeNull()
  })
})

describe('touchRecent', () => {
  it('moves the key to the front and caps the list', () => {
    expect(touchRecent(['a', 'b', 'c'], 'c', 3)).toEqual(['c', 'a', 'b'])
    expect(touchRecent(['a', 'b', 'c'], 'd', 3)).toEqual(['d', 'a', 'b'])
    expect(touchRecent([], 'a', 8)).toEqual(['a'])
  })
})

describe('hrefPath', () => {
  it('reads a hash router href as a path', () => {
    expect(hrefPath('#/docs/a.md')).toBe('/docs/a.md')
    expect(hrefPath('/docs/a.md')).toBe('/docs/a.md')
    expect(hrefPath('http://localhost:4174/#/docs/b.md?x=1')).toBe('/docs/b.md?x=1')
    expect(hrefPath(null)).toBeNull()
  })
})
