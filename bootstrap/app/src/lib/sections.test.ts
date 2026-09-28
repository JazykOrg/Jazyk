import { describe, expect, it } from 'vitest'
import type { Section } from './api'
import { sectionAtLine, sectionBySlug, sectionLines, sortedSections } from './sections'

const sec = (title: string, kind: string, lines: [number, number], order = 0): Section => ({
  title,
  kind,
  order,
  raw: '',
  hash: '',
  lines,
})

// Records count lines from 0 with the end exclusive, as the compiler writes them.
const sections: Record<string, Section> = {
  '/backend-system': sec('Backend system', 'root', [0, 20]),
  '/backend-system/database': sec('Database', 'heading', [7, 12]),
  '/backend-system/database/code-block-1': sec('', 'code-block', [9, 11]),
  '/backend-system/sub-systems': sec('Sub-systems', 'heading', [12, 20]),
  '/backend-system/sub-systems/list-item-1': sec('User Management', 'list-item', [16, 17]),
}

describe('sections', () => {
  it('sort by start line, then order', () => {
    expect(sortedSections(sections).map(([r]) => r)).toEqual([
      '/backend-system',
      '/backend-system/database',
      '/backend-system/database/code-block-1',
      '/backend-system/sub-systems',
      '/backend-system/sub-systems/list-item-1',
    ])
  })

  it('name the deepest heading holding a 1-based line', () => {
    expect(sectionLines(sections['/backend-system/database'])).toEqual([8, 12])
    expect(sectionAtLine(sections, 1)).toBe('/backend-system')
    expect(sectionAtLine(sections, 7)).toBe('/backend-system')
    expect(sectionAtLine(sections, 8)).toBe('/backend-system/database')
    expect(sectionAtLine(sections, 12)).toBe('/backend-system/database')
    expect(sectionAtLine(sections, 13)).toBe('/backend-system/sub-systems')
    expect(sectionAtLine(sections, 17)).toBe('/backend-system/sub-systems')
    expect(sectionAtLine(sections, 40)).toBeNull()
  })

  it('find a heading by its slug', () => {
    expect(sectionBySlug(sections, 'sub-systems')).toBe('/backend-system/sub-systems')
    expect(sectionBySlug(sections, 'Database')).toBe('/backend-system/database')
    expect(sectionBySlug(sections, 'nothing')).toBeNull()
  })
})
