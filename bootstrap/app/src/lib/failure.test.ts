import { describe, expect, it } from 'vitest'
import { batchSubject, failureLine, failureWhy, noClues, noteClue, parkedLine, refusalMessage, sectionTitle, stopWords, targetWords } from './failure'

const README = [{ kind: 'reconcile-section', target: 'docs/README.md#/hello-world' }]

describe('failure words', () => {
  it('names a section by its document and title', () => {
    expect(sectionTitle('/hello-world')).toBe('Hello world')
    expect(sectionTitle('/intro/getting_started')).toBe('Getting started')
    expect(targetWords('docs/README.md#/hello-world')).toBe('docs/README.md › Hello world')
    expect(targetWords('ent:order')).toBe('order')
    expect(batchSubject(README)).toBe('docs/README.md › Hello world')
    expect(batchSubject([...README, { kind: 'reconcile-section', target: 'docs/README.md#/usage' }])).toBe('docs/README.md (2 sections)')
  })

  it('says the stop in words, never a bare nothing committed', () => {
    expect(stopWords('nothing committed')).toBe('the model stopped without finishing')
    expect(stopWords('the model ended its turn with the batch still open')).toBe('the model stopped without finishing')
    expect(stopWords('the batch did not land (session stopped: end_turn)')).toBe('the model stopped without finishing')
    expect(stopWords('the session ran out of rounds after 40')).toBe('the model used up its rounds without finishing')
    expect(stopWords("the model call failed: This model's maximum context length is 4096 tokens.")).toBe("the model's context is too small for it")
    expect(stopWords('the model call failed: HTTP 500: upstream down')).toBe('the model failed (HTTP 500: upstream down)')
    expect(stopWords('the build was cancelled (session stopped: cancelled)')).toBe('the build was cancelled')
  })

  it('reads a refusal the native trace kept whole', () => {
    expect(refusalMessage('{"error":{"rule":"quote-not-found","message":"the quote is not in the section"}}')).toBe('the quote is not in the section')
    expect(refusalMessage('plain words')).toBe('plain words')
  })
})

describe('failureWhy', () => {
  const refused = noteClue(noClues(), { kind: 'toolError', rule: 'quote-not-found', message: 'the quote is not in the section' })
  const said = noteClue(noClues(), { kind: 'modelText', text: 'I could not find anything to extract.\nSecond line.' })

  it('prefers the refused step, then the last words, then the stop', () => {
    const both = noteClue(refused, { kind: 'modelText', text: 'giving up' })
    expect(failureWhy(both, 'nothing committed')).toBe('a step was refused (quote-not-found: the quote is not in the section)')
    expect(failureWhy(said, 'nothing committed')).toBe('the model said “I could not find anything to extract. Second line.” and stopped')
    expect(failureWhy(noClues(), 'nothing committed')).toBe('the model stopped without finishing')
  })

  it('lets a hard stop speak for itself', () => {
    expect(failureWhy(refused, 'the session ran out of rounds after 40')).toBe('the model used up its rounds without finishing')
    expect(failureWhy(said, 'the model call failed: HTTP 429 rate limit')).toBe('the model source is rate limited or out of quota')
  })

  it('starts over on a new attempt', () => {
    expect(noteClue(refused, { kind: 'sessionStart' })).toEqual(noClues())
  })

  it('trims long words to one line', () => {
    const long = noteClue(noClues(), { kind: 'modelText', text: 'x'.repeat(500) })
    const why = failureWhy(long, '')
    expect(why.length).toBeLessThan(160)
    expect(why).toContain('…')
  })
})

describe('failureLine', () => {
  it('says what was not read, why, and what happens next', () => {
    expect(failureLine(README, noClues(), 'the model ended its turn with the batch still open', 2)).toBe(
      'docs/README.md › Hello world was not read: the model stopped without finishing. It will be retried on the next build.',
    )
    expect(failureLine(README, noClues(), 'nothing committed', 1)).toBe(
      'docs/README.md › Hello world was not read: the model stopped without finishing. It will be retried.',
    )
    expect(failureLine(README, noClues(), 'nothing committed')).not.toContain('nothing committed')
  })

  it('words a goal left for later without its id', () => {
    const line = parkedLine('g:reconcile-section:docs/README.md#/hello-world', 'left open after its retry in session b2-1', 'the model stopped without finishing')
    expect(line).toBe('docs/README.md › Hello world left for later: the model stopped without finishing')
    expect(line).not.toContain('g:')
    expect(parkedLine('g:reconcile-section:docs/README.md#/hello-world', 'left open after its retry in session b2-1', null)).toBe(
      'docs/README.md › Hello world left for later: it was not finished after a retry',
    )
    expect(parkedLine('g:review-entity:ent:order', 'the build was cancelled', null)).toBe('order left for later: the build was cancelled')
  })
})
