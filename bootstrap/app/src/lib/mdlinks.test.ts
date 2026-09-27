import { describe, expect, it } from 'vitest'
import type { Project } from './api'
import { dirname, hasScheme, locate, normPath, relTo, resolveRel, slug, splitHash } from './mdlinks'

describe('normPath', () => {
  it.each([
    ['a/b/c', 'a/b/c'],
    ['/a/b/c', '/a/b/c'],
    ['a//b/./c/', 'a/b/c'],
    ['a/b/../c', 'a/c'],
    ['a/b/../../c', 'c'],
    ['/a/../../c', '/c'],
    ['../product', 'product'],
    ['', ''],
    ['/', '/'],
  ])('%s', (p, out) => {
    expect(normPath(p)).toBe(out)
  })
})

describe('relTo', () => {
  it('is the path under the directory, or null', () => {
    expect(relTo('/p', '/p/a/b.md')).toBe('a/b.md')
    expect(relTo('/p/', '/p/./a.md')).toBe('a.md')
    expect(relTo('/p', '/p')).toBeNull()
    expect(relTo('/p', '/px/a.md')).toBeNull()
    expect(relTo('/p', '/q/a.md')).toBeNull()
  })
})

describe('dirname', () => {
  it.each([
    ['a/b/c.md', 'a/b'],
    ['/a.md', ''],
    ['a.md', ''],
    ['a/b/', 'a/b'],
  ])('%s', (p, d) => {
    expect(dirname(p)).toBe(d)
  })
})

describe('splitHash', () => {
  it('separates the fragment', () => {
    expect(splitHash('a.md#sec')).toEqual({ path: 'a.md', hash: 'sec' })
    expect(splitHash('a.md')).toEqual({ path: 'a.md', hash: '' })
    expect(splitHash('#top')).toEqual({ path: '', hash: 'top' })
    expect(splitHash('a.md#x#y')).toEqual({ path: 'a.md', hash: 'x#y' })
  })
})

describe('hasScheme', () => {
  it.each([
    ['https://x', true],
    ['mailto:a@b', true],
    ['git+ssh://x', true],
    ['a.md', false],
    ['/a.md', false],
    ['C:/a.md', true],
    ['1x:y', false],
  ])('%s', (href, yes) => {
    expect(hasScheme(href)).toBe(yes)
  })
})

describe('resolveRel', () => {
  it('resolves against the file directory, or from the root', () => {
    expect(resolveRel('/p/docs/a.md', 'b.md')).toBe('/p/docs/b.md')
    expect(resolveRel('/p/docs/a.md', '../x/b.md')).toBe('/p/x/b.md')
    expect(resolveRel('/p/docs/a.md', './b.md')).toBe('/p/docs/b.md')
    expect(resolveRel('/p/docs/a.md', '/q/b.md')).toBe('/q/b.md')
    expect(resolveRel('/p/docs/a.md', 'my%20file.md')).toBe('/p/docs/my file.md')
  })
})

describe('slug', () => {
  it.each([
    ['Building', 'building'],
    ['The model', 'the-model'],
    ['  A  --  B  ', 'a-b'],
    ["What's `this`?", 'what-s-this'],
    ['Čeština 2', 'čeština-2'],
    ['---', ''],
  ])('%s', (s, out) => {
    expect(slug(s)).toBe(out)
  })
})

describe('locate', () => {
  const project = { root: '/p', out: '/p/jazyk-out', deliverable: '/p/src' } as Project
  const docs = [{ path: 'docs/a.md' }, { path: 'README.md' }]

  it('the out directory wins, then a matched document, then the deliverable', () => {
    expect(locate('/p/jazyk-out/diagrams/x.svg', project, docs)).toEqual({ tree: 'out', rel: 'diagrams/x.svg' })
    expect(locate('/p/docs/a.md', project, docs)).toEqual({ tree: 'docs', rel: 'docs/a.md' })
    expect(locate('/p/README.md', project, docs)).toEqual({ tree: 'docs', rel: 'README.md' })
    expect(locate('/p/src/main.rs', project, docs)).toEqual({ tree: 'deliverable', rel: 'main.rs' })
  })

  it('is null for a file under the root that is not a document', () => {
    expect(locate('/p/docs/other.md', project, docs)).toBeNull()
    expect(locate('/elsewhere/a.md', project, docs)).toBeNull()
  })

  it('the deliverable may be the root itself', () => {
    const p = { ...project, deliverable: '/p' }
    expect(locate('/p/docs/a.md', p, docs)).toEqual({ tree: 'docs', rel: 'docs/a.md' })
    expect(locate('/p/docs/other.md', p, docs)).toEqual({ tree: 'deliverable', rel: 'docs/other.md' })
  })
})
