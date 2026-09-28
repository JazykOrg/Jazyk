import { describe, expect, it } from 'vitest'
import { ancestors, buildTree, dirOf, globToRegExp, matchesGlob, readClosedFolders, treeProjectKey, visibleRows, writeClosedFolders } from './tree'

const paths = (names: string[]) => names.map((path) => ({ path }))

describe('buildTree', () => {
  it('puts folders first, then files, each by name', () => {
    const t = buildTree(paths(['docs/z.md', 'docs/a.md', 'docs/pages/login.md', 'docs/b/x.md', 'README.md']))
    expect(t.map((n) => `${n.kind}:${n.name}`)).toEqual(['dir:docs', 'file:README.md'])
    const docs = t[0]
    if (docs.kind !== 'dir') throw new Error('docs is a folder')
    expect(docs.children.map((n) => `${n.kind}:${n.name}`)).toEqual(['dir:b', 'dir:pages', 'file:a.md', 'file:z.md'])
    expect(docs.children[0].path).toBe('docs/b')
    expect(docs.children[3].path).toBe('docs/z.md')
  })

  it('sorts names naturally and without case', () => {
    const t = buildTree(paths(['b10.md', 'B2.md', 'a.md']))
    expect(t.map((n) => n.name)).toEqual(['a.md', 'B2.md', 'b10.md'])
  })

  it('keeps a folder that holds no file yet', () => {
    const t = buildTree(paths(['docs/a.md']), ['docs/new', 'other'])
    expect(t.map((n) => `${n.kind}:${n.path}`)).toEqual(['dir:docs', 'dir:other'])
    const docs = t[0]
    if (docs.kind !== 'dir') throw new Error('docs is a folder')
    expect(docs.children.map((n) => `${n.kind}:${n.path}`)).toEqual(['dir:docs/new', 'file:docs/a.md'])
  })
})

describe('visibleRows', () => {
  it('walks open folders only, with depth and parent', () => {
    const t = buildTree(paths(['docs/a.md', 'docs/pages/login.md', 'top.md']))
    const all = visibleRows(t, () => true)
    expect(all.map((r) => `${r.depth}:${r.node.path}`)).toEqual(['0:docs', '1:docs/pages', '2:docs/pages/login.md', '1:docs/a.md', '0:top.md'])
    expect(all[2].parent).toBe('docs/pages')
    const closed = visibleRows(t, (d) => d !== 'docs/pages')
    expect(closed.map((r) => r.node.path)).toEqual(['docs', 'docs/pages', 'docs/a.md', 'top.md'])
  })
})

describe('paths', () => {
  it('names the folders on the way', () => {
    expect(ancestors('a/b/c.md')).toEqual(['a', 'a/b'])
    expect(ancestors('c.md')).toEqual([])
    expect(dirOf('a/b/c.md')).toBe('a/b')
    expect(dirOf('c.md')).toBe('')
  })
})

describe('globs', () => {
  it('matches the way the compiler does', () => {
    expect(globToRegExp('docs/**/*.md').test('docs/a.md')).toBe(true)
    expect(globToRegExp('docs/**/*.md').test('docs/x/y/a.md')).toBe(true)
    expect(globToRegExp('docs/**/*.md').test('docs/a.txt')).toBe(false)
    expect(globToRegExp('docs/*.md').test('docs/x/a.md')).toBe(false)
    expect(globToRegExp('**/*.md').test('a.md')).toBe(true)
    expect(globToRegExp('a?.md').test('ab.md')).toBe(true)
    expect(globToRegExp('a?.md').test('a/.md')).toBe(false)
  })

  it('lets the last matching pattern win, with negation', () => {
    const g = ['**/*.md', '!fixtures/**']
    expect(matchesGlob('docs/a.md', g)).toBe(true)
    expect(matchesGlob('fixtures/a.md', g)).toBe(false)
    expect(matchesGlob('src/a.ts', g)).toBe(false)
  })
})

describe('closed folders', () => {
  it('are remembered per project and head', () => {
    localStorage.clear()
    localStorage.setItem('jazyk.open', 'demo:erp')
    expect(treeProjectKey('/p')).toBe('demo:erp')
    localStorage.removeItem('jazyk.open')
    expect(treeProjectKey('/p')).toBe('/p')
    writeClosedFolders('docs', 'demo:erp', new Set(['docs/pages']))
    expect([...readClosedFolders('docs', 'demo:erp')]).toEqual(['docs/pages'])
    expect([...readClosedFolders('product', 'demo:erp')]).toEqual([])
    expect([...readClosedFolders('docs', 'demo:shop')]).toEqual([])
  })
})
