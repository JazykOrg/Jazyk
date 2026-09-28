import { describe, expect, it } from 'vitest'
import { buildMentionIndex, findMentions, findQuoteIn, lineOf, quoteLine } from './mentions'

const entities = {
  'ent:backend': { name: 'Backend', aliases: ['backend system'] },
  'ent:inventory-management': { name: 'Inventory Management' },
  'ent:user': { name: 'User' },
  'ent:user-management': { name: 'User Management' },
  'ent:x': { name: 'X' },
}

describe('findMentions', () => {
  const index = buildMentionIndex(entities)

  it('matches names and aliases case-insensitively on word boundaries', () => {
    const text = 'The backend system is a REST API. The BACKEND serves the backends.'
    const got = findMentions(text, index).map((m) => [m.id, m.name])
    expect(got).toEqual([
      ['ent:backend', 'backend system'],
      ['ent:backend', 'BACKEND'],
    ])
  })

  it('lets the longer name win where two overlap', () => {
    const text = '- [User Management](./subsystems/user.md) and a User.'
    const got = findMentions(text, index).map((m) => [m.id, text.slice(m.from, m.to)])
    expect(got).toEqual([
      ['ent:user-management', 'User Management'],
      ['ent:user', 'User'],
    ])
  })

  it('matches across a line break inside a name', () => {
    const text = 'the Inventory\nManagement subsystem'
    expect(findMentions(text, index).map((m) => m.id)).toEqual(['ent:inventory-management'])
  })

  it('drops one-character names and returns nothing without entities', () => {
    expect(findMentions('X marks the spot', index)).toEqual([])
    expect(findMentions('Backend', buildMentionIndex({}))).toEqual([])
  })
})

describe('quotes', () => {
  const text = '# Title\n\nThe backend   system is\na REST API.\n\nMore.'
  it('locate whitespace-insensitively', () => {
    const hit = findQuoteIn(text, 'The backend system is a REST API.')
    expect(hit).not.toBeNull()
    expect(text.slice(hit!.from, hit!.to)).toBe('The backend   system is\na REST API.')
    expect(findQuoteIn(text, 'absent')).toBeNull()
    expect(findQuoteIn(text, '   ')).toBeNull()
  })
  it('report their line', () => {
    expect(lineOf(text, 0)).toBe(1)
    expect(quoteLine(text, 'The backend system')).toBe(3)
    expect(quoteLine(text, 'More.')).toBe(6)
    expect(quoteLine(text, 'nothing here')).toBeNull()
  })
})
