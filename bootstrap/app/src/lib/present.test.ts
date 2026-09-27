import { describe, expect, it } from 'vitest'
import type { Graph } from './api'
import { criterionKindWord, kindWord, nodeName, relationPhrase, shorten, statusWord } from './present'

describe('statusWord', () => {
  it.each([
    ['verified', 'Passing', 'v-ok'],
    ['failing', 'Failing', 'v-bad'],
    ['partial', 'Partly passing', 'v-bad'],
    ['unimplemented', 'Not built yet', 'v-none'],
    ['stale-requirement', 'Outdated: the requirement changed', 'v-stale'],
    ['stale-test', 'Needs a rerun', 'v-stale'],
    ['stale-code', 'Needs a rerun', 'v-stale'],
    ['something-else', 'No test yet', 'v-none'],
    [undefined, 'No test yet', 'v-none'],
    [null, 'No test yet', 'v-none'],
  ])('%s', (status, word, cls) => {
    expect(statusWord(status)).toEqual({ word, cls })
  })
})

describe('relationPhrase', () => {
  it.each([
    ['generalization', 'is a kind of', 'has the kind'],
    ['realization', 'implements', 'is implemented by'],
    ['composition', 'is made of', 'is part of'],
    ['aggregation', 'has', 'belongs to'],
    ['association', 'is related to', 'is related to'],
    ['dependency', 'uses', 'is used by'],
    [undefined, 'uses', 'is used by'],
  ])('%s', (type, outgoing, incoming) => {
    expect(relationPhrase(type, true)).toBe(outgoing)
    expect(relationPhrase(type, false)).toBe(incoming)
  })
})

describe('criterionKindWord', () => {
  it.each([
    ['negative', 'Failure case'],
    ['boundary', 'Edge case'],
    ['quality', 'Measure'],
    ['positive', 'Example'],
    [undefined, 'Example'],
  ])('%s', (kind, word) => {
    expect(criterionKindWord(kind)).toBe(word)
  })
})

describe('kindWord', () => {
  it.each([
    ['cli', 'CLI'],
    ['api', 'API'],
    ['ui', 'UI'],
    ['component', 'Component'],
    ['use-case', 'Use case'],
    ['data_type', 'Data type'],
    ['System', 'System'],
    ['', ''],
    [undefined, ''],
    [null, ''],
  ])('%s', (stereotype, word) => {
    expect(kindWord(stereotype)).toBe(word)
  })
})

describe('shorten', () => {
  it('leaves what fits and ends a cut with an ellipsis inside the limit', () => {
    expect(shorten('short', 10)).toBe('short')
    expect(shorten('exactly ten', 11)).toBe('exactly ten')
    expect(shorten('a long sentence here', 10)).toBe('a long se…')
    expect(shorten('a long sentence here', 10)).toHaveLength(10)
    // Trailing space before the ellipsis is trimmed.
    expect(shorten('abcd efgh', 6)).toBe('abcd…')
  })
})

describe('nodeName', () => {
  const g: Graph = {
    generation: 1,
    entities: { 'e:user': { name: 'User' }, 'e:cart': { name: 'Cart' } },
    requirements: { 'r:1': { statement: 'A user owns one cart.' } },
    criteria: { 'c:1': { requirement: 'r:1', statement: 'Given a new user, one empty cart exists.' } },
    views: { 'v:titled': { kind: 'class', title: 'Shop' }, 'v:untitled': { kind: 'class', title: undefined as unknown as string } },
    relationships: { 'rel:1': { members: ['e:user', 'e:cart', 'e:gone'], contributions: [] } },
    stateMachines: { 'sm:cart': { subject: 'e:cart', states: [], transitions: [] }, 'sm:gone': { subject: 'e:gone', states: [], transitions: [] } },
    diagnostics: { 'd:1': { rule: 'ambiguity', severity: 'warning', message: 'Which cart?' } },
    redirects: {},
  }

  it.each([
    ['e:user', 'User'],
    ['r:1', 'A user owns one cart.'],
    ['c:1', 'Given a new user, one empty cart exists.'],
    ['v:titled', 'Shop'],
    ['v:untitled', 'v:untitled'],
    ['d:1', 'Which cart?'],
    ['sm:cart', 'States of Cart'],
    ['sm:gone', 'States of e:gone'],
    ['rel:1', 'User and Cart and e:gone'],
    ['x:unknown', 'x:unknown'],
  ])('%s', (id, name) => {
    expect(nodeName(g, id)).toBe(name)
  })

  it('is the id before the graph loads', () => {
    expect(nodeName(undefined, 'e:user')).toBe('e:user')
  })
})
