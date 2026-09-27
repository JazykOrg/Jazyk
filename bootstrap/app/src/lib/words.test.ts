import { describe, expect, it } from 'vitest'
import { compilerVerdict, coverageWord, provenanceWord, shapeWords, triageWords, verdictWords, words } from './words'

describe('words', () => {
  it('reads as product words, with the compiler word beside them in Developer mode', () => {
    expect(words(false).sync).toBe('Sync')
    expect(words(true).sync).toBe('Sync (compile)')
    expect(words(false).syncNumber(3)).toBe('Sync #3')
    expect(words(true).syncNumber(3)).toBe('Sync #3 (g3)')
    expect(words(false).both('A', 'b')).toBe('A')
    expect(words(true).both('A', 'b')).toBe('A (b)')
  })

  it('is one object per mode', () => {
    expect(words(false)).toBe(words(false))
    expect(words(true)).toBe(words(true))
    expect(words(false)).not.toBe(words(true))
  })
})

describe('verdictWords', () => {
  it('is not synced yet without a verdict', () => {
    expect(verdictWords(undefined, false)).toBe('Not synced yet')
    expect(verdictWords({ state: '' }, true)).toBe('Not synced yet')
  })

  it('reads a converged verdict with its suggestions', () => {
    expect(verdictWords({ state: 'converged' }, false)).toBe('Up to date')
    expect(verdictWords({ state: 'converged', optional: 1 }, false)).toBe('Up to date, 1 suggestion')
    expect(verdictWords({ state: 'converged', optional: 2, blocked: 1 }, false)).toBe('Up to date, 2 suggestions')
    expect(verdictWords({ state: 'converged', optional: 2, blocked: 1 }, true)).toBe('Up to date, 2 suggestions (converged, 1 blocked, 2 optional advised)')
  })

  it('counts what is left otherwise', () => {
    expect(verdictWords({ state: 'unconverged', open: 1 }, false)).toBe('Sync incomplete: 1 item left')
    expect(verdictWords({ state: 'unconverged', open: 2, failed: 1 }, false)).toBe('Sync incomplete: 3 items left, 1 failed')
    expect(verdictWords({ state: 'unconverged', open: 2, failed: 1 }, true)).toBe(
      'Sync incomplete: 3 items left, 1 failed (unconverged: 2 open, 1 failed, 0 blocked, 0 optional advised)',
    )
  })
})

describe('compilerVerdict', () => {
  it('reads the compiler line', () => {
    expect(compilerVerdict({ state: 'converged' })).toBe('converged')
    expect(compilerVerdict({ state: 'converged', blocked: 2, optional: 1 })).toBe('converged, 2 blocked, 1 optional advised')
    expect(compilerVerdict({ state: 'unconverged' })).toBe('unconverged: 0 open, 0 failed, 0 blocked, 0 optional advised')
  })
})

describe('shapeWords', () => {
  it.each([
    ['docs-and-product', 'documents and product'],
    ['docs-only', 'documents only'],
    ['product-only', 'product only'],
    ['empty', 'empty'],
    ['other', ''],
    [undefined, ''],
  ])('%s', (shape, w) => {
    expect(shapeWords(shape)).toBe(w)
  })
})

describe('the small word tables', () => {
  it('triage', () => {
    expect(triageWords(false)).toEqual([
      { value: 'acknowledged', label: 'Seen' },
      { value: 'suppressed', label: 'Hidden' },
      { value: 'wontfix', label: "Won't fix" },
    ])
    expect(triageWords(true)[2].label).toBe("Won't fix (wontfix)")
  })

  it('coverage', () => {
    expect(coverageWord('covered', false)).toBe('Read')
    expect(coverageWord('non-normative', false)).toBe('Informational')
    expect(coverageWord(undefined, false)).toBe('Not read yet')
    expect(coverageWord('covered', true)).toBe('Read (covered)')
  })

  it('provenance', () => {
    expect(provenanceWord('quote', false)).toBe('Quoted')
    expect(provenanceWord('derived', false)).toBe('Inferred')
    expect(provenanceWord('decree', true)).toBe('Your edit (decree)')
    expect(provenanceWord('other', true)).toBe('other')
  })
})
