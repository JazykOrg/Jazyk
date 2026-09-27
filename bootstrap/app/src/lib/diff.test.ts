import { describe, expect, it } from 'vitest'
import { lineMarks } from './diff'

// The marks as plain arrays, for readable comparisons.
function marks(baseline: string, current: string) {
  const m = lineMarks(baseline, current)
  return {
    added: [...m.added].sort((a, b) => a - b),
    modified: [...m.modified].sort((a, b) => a - b),
    deletedAbove: Object.fromEntries(m.deletedAbove),
  }
}

describe('lineMarks', () => {
  it('marks nothing when the text is unchanged', () => {
    expect(marks('a\nb\nc', 'a\nb\nc')).toEqual({ added: [], modified: [], deletedAbove: {} })
    expect(marks('', '')).toEqual({ added: [], modified: [], deletedAbove: {} })
  })

  it('marks an inserted line as added', () => {
    expect(marks('a\nc', 'a\nb\nc')).toEqual({ added: [2], modified: [], deletedAbove: {} })
  })

  it('marks a replaced line as modified', () => {
    expect(marks('a\nb\nc', 'a\nB\nc')).toEqual({ added: [], modified: [2], deletedAbove: {} })
  })

  it('anchors a deletion on the line that follows it', () => {
    expect(marks('a\nb\nc', 'a\nc')).toEqual({ added: [], modified: [], deletedAbove: { 2: 1 } })
    expect(marks('a\nb\nc\nd', 'a\nd')).toEqual({ added: [], modified: [], deletedAbove: { 2: 2 } })
  })

  it('anchors a deletion at the end on the line after the last', () => {
    expect(marks('a\nb\nc', 'a')).toEqual({ added: [], modified: [], deletedAbove: { 2: 2 } })
  })

  it('pairs deleted and added lines, the surplus is added', () => {
    expect(marks('a\nb\nc', 'a\nX\nY\nZ\nc')).toEqual({ added: [3, 4], modified: [2], deletedAbove: {} })
  })

  it('a deletion larger than the replacement marks the surplus above the next line', () => {
    expect(marks('a\nb\nc\nd\ne', 'a\nX\ne')).toEqual({ added: [], modified: [2], deletedAbove: { 3: 2 } })
  })

  it('handles a wholly new or wholly removed text', () => {
    expect(marks('', 'a\nb')).toEqual({ added: [2], modified: [1], deletedAbove: {} })
    expect(marks('a\nb', '')).toEqual({ added: [], modified: [1], deletedAbove: { 2: 1 } })
  })

  it('keeps edits apart when they are far from each other', () => {
    const base = ['h', '1', '2', '3', '4', '5', '6', 't'].join('\n')
    const cur = ['h', '1', 'two', '3', '4', '5', '6', 'new', 't'].join('\n')
    expect(marks(base, cur)).toEqual({ added: [8], modified: [3], deletedAbove: {} })
  })
})
